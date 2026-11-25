//! LAN discovery (UDP broadcast) and transfer (one TCP connection per send).
//! Wire format is shared by every platform: change it here, bump MAGIC, and both apps follow.
//!
//! Discovery packet: MAGIC | id u64 | kind u8 (0 announce, 1 reply) | name utf8
//! Transfer: sender -> MAGIC | name | count u32 | count * (is_text u8, path, size u64)
//!           receiver -> 1 (accept) / 0 (decline)
//!           sender -> raw bytes of every entry, in order
//!           receiver -> 1 (all saved)
//! Strings are u16 length + utf8. Integers are little endian.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

pub const PORT: u16 = 47821;
const MAGIC: &[u8; 4] = b"AKP1";
const ANNOUNCE: Duration = Duration::from_secs(2);
const EXPIRE: Duration = Duration::from_secs(7);
const CHUNK: u64 = 4 << 20; // progress update granularity
const BUF: usize = 1 << 20;
const MAX_TEXT: u64 = 16 << 20;

pub enum Item {
    /// File or folder on disk (desktop).
    Path(PathBuf),
    /// Already-open file plus its display name (Android hands out fds, not paths).
    Open(File, String),
    Text(String),
}

pub struct Peer {
    pub name: String,
    pub ip: IpAddr,
    seen: Instant,
}

pub fn discovery_socket() -> io::Result<UdpSocket> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PORT))?;
    sock.set_broadcast(true)?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    Ok(sock)
}

pub struct Entry {
    pub text: bool,
    pub name: String,
    pub size: u64,
}

pub struct Offer {
    pub from: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum State {
    Waiting,
    Running,
    Done,
    Failed(String),
}

pub struct Transfer {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub state: Mutex<State>,
    started: OnceLock<Instant>,
    ended: OnceLock<Instant>,
    sock: Mutex<Option<TcpStream>>,
    cancelled: std::sync::atomic::AtomicBool,
}

impl Default for Transfer {
    fn default() -> Self {
        Self::new()
    }
}

impl Transfer {
    pub fn new() -> Self {
        let (started, ended) = Default::default();
        Self { done: 0.into(), total: 0.into(), state: Mutex::new(State::Waiting), started, ended, sock: Mutex::new(None), cancelled: false.into() }
    }
    /// Bytes per second since the receiver accepted.
    pub fn speed(&self) -> f64 {
        let Some(s) = self.started.get() else { return 0.0 };
        let secs = self.ended.get().copied().unwrap_or_else(Instant::now).duration_since(*s).as_secs_f64();
        self.done.load(Relaxed) as f64 / secs.max(1e-3)
    }
    /// Unblocks any read/write on both ends; the worker thread then fails out.
    pub fn cancel(&self) {
        self.cancelled.store(true, Relaxed);
        if let Some(s) = &*self.sock.lock().unwrap() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }
    fn set(&self, s: State) {
        if s == State::Running {
            let _ = self.started.set(Instant::now());
        }
        *self.state.lock().unwrap() = s;
    }
    fn end<T>(&self, r: io::Result<T>) -> Option<T> {
        let _ = self.ended.set(Instant::now());
        *self.sock.lock().unwrap() = None;
        match r {
            Ok(v) => {
                self.set(State::Done);
                Some(v)
            }
            Err(e) => {
                self.set(State::Failed(if self.cancelled.load(Relaxed) { "Cancelled".into() } else { describe(&e) }));
                None
            }
        }
    }
}

/// The one-line reason both UIs show for a failed transfer: the problem and what to check.
fn describe(e: &io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => "Couldn't connect. Is Akhri Pasta open on the other device?".into(),
        TimedOut | WouldBlock => "Timed out. Are both devices on the same network?".into(),
        HostUnreachable | NetworkUnreachable | AddrNotAvailable => "Can't reach that device from this network.".into(),
        ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof => "Stopped: the connection closed.".into(),
        StorageFull => "Not enough storage space.".into(),
        PermissionDenied => "No permission to save there.".into(),
        _ if e.to_string() == "declined" => "Declined".into(),
        _ => e.to_string(),
    }
}

// ---------- discovery ----------

/// Runs forever: broadcasts our name every 2s, records peers, answers announces
/// directly so newcomers see everyone instantly. `changed` fires when the list changes.
// ponytail: 255.255.255.255 leaves via the default route only; per-interface broadcast if multi-NIC matters.
pub fn discovery(sock: UdpSocket, id: u64, name: &Mutex<String>, peers: &Mutex<HashMap<u64, Peer>>, changed: impl Fn()) {
    let packet = |kind: u8| [MAGIC.as_slice(), &id.to_le_bytes(), &[kind], name.lock().unwrap().as_bytes()].concat();
    let mut next = Instant::now();
    let mut buf = [0u8; 512];
    loop {
        if Instant::now() >= next {
            next = Instant::now() + ANNOUNCE;
            let _ = sock.send_to(&packet(0), (Ipv4Addr::BROADCAST, PORT));
            let mut p = peers.lock().unwrap();
            let before = p.len();
            p.retain(|_, peer| peer.seen.elapsed() < EXPIRE);
            if p.len() != before {
                eprintln!("peer: {} went away", before - p.len());
                changed();
            }
        }
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let b = &buf[..n];
        if n < 13 || &b[..4] != MAGIC {
            continue;
        }
        let pid = u64::from_le_bytes(b[4..12].try_into().unwrap());
        if pid == id {
            continue;
        }
        if b[12] == 0 {
            let _ = sock.send_to(&packet(1), from);
        }
        let peer = Peer { name: String::from_utf8_lossy(&b[13..]).into_owned(), ip: from.ip(), seen: Instant::now() };
        let key = (peer.name.clone(), peer.ip);
        let old = peers.lock().unwrap().insert(pid, peer);
        if old.is_none_or(|o| (o.name, o.ip) != key) {
            eprintln!("peer: {} at {}", key.0, key.1);
            changed();
        }
    }
}

pub fn listen() -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT))
}

// ---------- send ----------

enum Src {
    File(PathBuf),
    Open(File),
    Text(String),
}

fn expand(items: Vec<Item>) -> io::Result<Vec<(Entry, Src)>> {
    let mut out = vec![];
    for it in items {
        match it {
            Item::Text(t) => out.push((Entry { text: true, name: String::new(), size: t.len() as u64 }, Src::Text(t))),
            Item::Open(f, name) => {
                let size = f.metadata()?.len();
                let name = if name.is_empty() { "file".into() } else { name.replace('/', "_") };
                out.push((Entry { text: false, name, size }, Src::Open(f)));
            }
            Item::Path(p) => {
                let name = p.file_name().map_or("item".into(), |n| n.to_string_lossy().into_owned());
                walk(&p, name, &mut out)?;
            }
        }
    }
    Ok(out)
}

// ponytail: empty folders are not sent; add a dir entry kind if anyone misses them.
fn walk(p: &Path, name: String, out: &mut Vec<(Entry, Src)>) -> io::Result<()> {
    let meta = fs::metadata(p)?;
    if meta.is_dir() {
        for e in fs::read_dir(p)? {
            let e = e?;
            if e.file_type()?.is_symlink() && e.path().is_dir() {
                continue; // symlinked folders could loop forever
            }
            walk(&e.path(), format!("{name}/{}", e.file_name().to_string_lossy()), out)?;
        }
    } else if meta.is_file() {
        out.push((Entry { text: false, name, size: meta.len() }, Src::File(p.into())));
    }
    Ok(())
}

pub fn send(addr: SocketAddr, me: &str, items: Vec<Item>, t: &Transfer) {
    t.end(send_inner(addr, me, items, t));
}

fn send_inner(addr: SocketAddr, me: &str, items: Vec<Item>, t: &Transfer) -> io::Result<()> {
    let entries = expand(items)?;
    t.total.store(entries.iter().map(|(e, _)| e.size).sum(), Relaxed);
    let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    *t.sock.lock().unwrap() = Some(sock.try_clone()?);
    let mut w = BufWriter::with_capacity(BUF, &sock);
    w.write_all(MAGIC)?;
    put_str(&mut w, me)?;
    w.write_all(&u32::try_from(entries.len()).map_err(|_| bad("too many files"))?.to_le_bytes())?;
    for (e, _) in &entries {
        w.write_all(&[e.text as u8])?;
        put_str(&mut w, &e.name)?;
        w.write_all(&e.size.to_le_bytes())?;
    }
    w.flush()?;
    if get::<1>(&mut &sock)? != [1] {
        return Err(io::Error::other("declined"));
    }
    t.set(State::Running);
    sock.set_write_timeout(Some(Duration::from_secs(30)))?;
    for (e, src) in &entries {
        match src {
            Src::Text(s) => {
                w.write_all(s.as_bytes())?;
                t.done.fetch_add(e.size, Relaxed);
            }
            Src::File(p) => pump(&File::open(p)?, &mut w, e.size, &t.done)?,
            Src::Open(f) => pump(f, &mut w, e.size, &t.done)?,
        }
    }
    w.flush()?;
    sock.set_read_timeout(Some(Duration::from_secs(30)))?;
    if get::<1>(&mut &sock)? != [1] {
        return Err(bad("receiver failed to save"));
    }
    Ok(())
}

// ---------- receive ----------

/// Reads the offer, asks `ask` (blocks until the user decides), then saves into `dir`.
/// Returns received text snippets.
pub fn receive(sock: TcpStream, dir: &Path, t: &Transfer, ask: impl FnOnce(&Offer) -> bool) -> Vec<String> {
    t.end(receive_inner(sock, dir, t, ask)).unwrap_or_default()
}

fn receive_inner(sock: TcpStream, dir: &Path, t: &Transfer, ask: impl FnOnce(&Offer) -> bool) -> io::Result<Vec<String>> {
    *t.sock.lock().unwrap() = Some(sock.try_clone()?);
    sock.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::with_capacity(BUF, &sock);
    if &get::<4>(&mut r)? != MAGIC {
        return Err(bad("not an akhri_pasta sender"));
    }
    let from = get_str(&mut r)?;
    let n = u32::from_le_bytes(get(&mut r)?);
    let mut entries = vec![];
    for _ in 0..n {
        let e = Entry { text: get::<1>(&mut r)? == [1], name: get_str(&mut r)?, size: u64::from_le_bytes(get(&mut r)?) };
        // Trust boundary: names come from the network.
        if e.text && e.size > MAX_TEXT || !e.text && !safe_rel(&e.name) {
            return Err(bad("rejected unsafe entry"));
        }
        entries.push(e);
    }
    let offer = Offer { from, entries };
    t.total.store(offer.entries.iter().map(|e| e.size).sum(), Relaxed);
    if !ask(&offer) {
        let _ = (&sock).write_all(&[0]);
        return Err(io::Error::other("declined"));
    }
    (&sock).write_all(&[1])?;
    t.set(State::Running);
    sock.set_read_timeout(Some(Duration::from_secs(30)))?;
    fs::create_dir_all(dir)?;

    let mut texts = vec![];
    let mut tops: HashMap<String, PathBuf> = HashMap::new(); // incoming folder -> unique local folder
    for e in &offer.entries {
        if e.text {
            let mut b = vec![0; e.size as usize];
            r.read_exact(&mut b)?;
            texts.push(String::from_utf8_lossy(&b).into_owned());
            t.done.fetch_add(e.size, Relaxed);
            continue;
        }
        let mut comps = Path::new(&e.name).components().map(|c| c.as_os_str().to_string_lossy().into_owned());
        let top = comps.next().unwrap();
        let rest: PathBuf = comps.collect();
        let path = if rest.as_os_str().is_empty() {
            unique(dir, &top)
        } else {
            tops.entry(top).or_insert_with_key(|k| unique(dir, k)).join(rest)
        };
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        let mut f = BufWriter::with_capacity(BUF, File::create(&path)?);
        if let Err(err) = pump(&mut r, &mut f, e.size, &t.done).and_then(|_| f.flush()) {
            drop(f);
            let _ = fs::remove_file(&path); // no half-written files left behind
            return Err(err);
        }
    }
    (&sock).write_all(&[1])?;
    Ok(texts)
}

/// Only plain relative paths: no `..`, no root, no `.`.
fn safe_rel(name: &str) -> bool {
    !name.is_empty() && Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
}

/// `dir/name`, or `dir/name (1).ext` etc. if taken.
fn unique(dir: &Path, name: &str) -> PathBuf {
    let free = |p: &Path| fs::symlink_metadata(p).is_err();
    let p = dir.join(name);
    if free(&p) {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (1..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| free(p)).unwrap()
}

// ---------- wire helpers ----------

/// Copies exactly `len` bytes in chunks so progress can update. `io::copy` keeps
/// using sendfile on Linux for File -> socket.
fn pump(mut r: impl Read, w: &mut impl Write, mut len: u64, done: &AtomicU64) -> io::Result<()> {
    while len > 0 {
        let n = io::copy(&mut r.by_ref().take(len.min(CHUNK)), w)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        len -= n;
        done.fetch_add(n, Relaxed);
    }
    Ok(())
}

fn put_str(w: &mut impl Write, s: &str) -> io::Result<()> {
    let len = u16::try_from(s.len()).map_err(|_| bad("name too long"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(s.as_bytes())
}

fn get_str(r: &mut impl Read) -> io::Result<String> {
    let mut b = vec![0; u16::from_le_bytes(get(r)?) as usize];
    r.read_exact(&mut b)?;
    String::from_utf8(b).map_err(|_| bad("bad utf8"))
}

fn get<const N: usize>(r: &mut impl Read) -> io::Result<[u8; N]> {
    let mut b = [0; N];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn bad(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_text_and_unsafe_names() {
        let root = std::env::temp_dir().join(format!("akp-test-{}", std::process::id()));
        let (src, dst) = (root.join("src/Photos"), root.join("dst"));
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.bin"), vec![7u8; 9 << 20]).unwrap(); // > CHUNK, exercises the loop
        fs::write(src.join("sub/b.txt"), "hi").unwrap();
        fs::create_dir_all(dst.join("Photos")).unwrap(); // existing folder -> must become "Photos (1)"

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let rx = std::thread::spawn(move || {
            let t = Transfer::new();
            let texts = receive(l.accept().unwrap().0, &dst, &t, |o| o.from == "pc" && o.entries.len() == 4);
            (texts, t.state())
        });
        let t = Transfer::new();
        let open = File::open(src.join("sub/b.txt")).unwrap();
        send(addr, "pc", vec![Item::Path(src.clone()), Item::Text("clip".into()), Item::Open(open, "../fd.txt".into())], &t);
        let (texts, rstate) = rx.join().unwrap();
        assert_eq!((t.state(), rstate), (State::Done, State::Done));
        assert_eq!(texts, ["clip"]);
        assert_eq!(fs::read(root.join("dst/Photos (1)/a.bin")).unwrap().len(), 9 << 20);
        assert_eq!(fs::read_to_string(root.join("dst/Photos (1)/sub/b.txt")).unwrap(), "hi");
        assert_eq!(fs::read_to_string(root.join("dst/.._fd.txt")).unwrap(), "hi"); // fd item, '/' neutralized

        for evil in ["../x", "/etc/passwd", "a/../../x", "./a", ""] {
            assert!(!safe_rel(evil), "{evil}");
        }
        assert!(safe_rel("Photos/sub/b.txt"));
        fs::remove_dir_all(root).unwrap();
    }
}
