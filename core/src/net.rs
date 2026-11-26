//! LAN discovery (UDP broadcast) and transfer (one TCP connection per send).
//! Wire format is shared by every platform: change it here, bump MAGIC, and both apps follow.
//!
//! Discovery packet: MAGIC | id u64 | kind u8 (0 announce, 1 reply) | public key [32] | name utf8
//! Transfer: sender -> MAGIC (plain), then a Noise_XX_25519_ChaChaPoly_BLAKE2s handshake
//!           (3 messages, each a u16 length + bytes; prologue = MAGIC) that proves both devices'
//!           permanent keys: the sender aborts unless the receiver's key is the one it announced.
//!           Message 1 carries BLAKE2s(sender key | nonce), message 3 reveals the 32-byte nonce, so
//!           neither side can steer the handshake hash. Its first 4 bytes (u32 LE % 1e6) are the
//!           6-digit code both screens show when the devices haven't verified each other.
//!           Everything after is
//!           encrypted in frames of u16 length + ChaCha20-Poly1305 ciphertext (≤ 65535 bytes):
//!           sender -> name | check u8 | count u32 | count * (kind u8, path, size u64)
//!           kind: 0 file, 1 text, 2 clipboard image (a PNG file)
//!           receiver -> check u8 (1: either device hasn't verified the other; both show the code)
//!           receiver -> 1 (accept) / 0 (decline)
//!           if check: sender -> 1 (its user confirmed the code)
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
        mpsc,
    },
    time::{Duration, Instant},
};

use snow::{
    params::HashChoice,
    resolvers::{CryptoResolver, DefaultResolver},
};

pub const PORT: u16 = 47821;
const MAGIC: &[u8; 4] = b"AKP3";
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
    /// Clipboard image: an open PNG plus its name. Saved like a file, and also put on the
    /// receiver's clipboard.
    Image(File, String),
}

/// What a transfer hands the receiver's clipboard.
#[derive(Debug, PartialEq)]
pub enum Clip {
    Text(String),
    /// A received clipboard image (PNG), already saved at this path.
    Image(PathBuf),
}

pub struct Peer {
    pub name: String,
    pub ip: IpAddr,
    /// Its permanent public key, as announced. The transfer handshake must prove it.
    pub key: [u8; 32],
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
    pub image: bool,
    pub name: String,
    pub size: u64,
}

pub struct Offer {
    pub from: String,
    /// The sender's permanent public key, proven by the handshake.
    pub key: [u8; 32],
    /// The sender hasn't verified this device yet and wants the code compared.
    pub check: bool,
    /// The 6-digit code for this connection, "123 456". Both ends compute the same one.
    pub code: String,
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
    pub(crate) fn end<T>(&self, r: io::Result<T>) -> Option<T> {
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
        ConnectionRefused => "Couldn't connect. Is FlashHop open on the other device?".into(),
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
pub fn discovery(sock: UdpSocket, id: u64, key: [u8; 32], name: &Mutex<String>, peers: &Mutex<HashMap<u64, Peer>>, changed: impl Fn()) {
    let packet = |kind: u8| [MAGIC.as_slice(), &id.to_le_bytes(), &[kind], &key, name.lock().unwrap().as_bytes()].concat();
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
        if n < 45 || &b[..4] != MAGIC {
            continue;
        }
        let pid = u64::from_le_bytes(b[4..12].try_into().unwrap());
        if pid == id {
            continue;
        }
        if b[12] == 0 {
            let _ = sock.send_to(&packet(1), from);
        }
        let peer = Peer { name: String::from_utf8_lossy(&b[45..]).into_owned(), ip: from.ip(), key: b[13..45].try_into().unwrap(), seen: Instant::now() };
        let what = (peer.name.clone(), peer.ip, peer.key);
        let old = peers.lock().unwrap().insert(pid, peer);
        if old.is_none_or(|o| (o.name, o.ip, o.key) != what) {
            eprintln!("peer: {} at {}", what.0, what.1);
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
        let image = matches!(it, Item::Image(..));
        match it {
            Item::Text(t) => out.push((Entry { text: true, image: false, name: String::new(), size: t.len() as u64 }, Src::Text(t))),
            Item::Open(f, name) | Item::Image(f, name) => {
                let size = f.metadata()?.len();
                // A relative path ("Folder/sub/a.jpg", Android folder sends) keeps its folders.
                let name = if name.is_empty() { "file".into() } else if safe_rel(&name) { name } else { name.replace('/', "_") };
                out.push((Entry { text: false, image, name, size }, Src::Open(f)));
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
        out.push((Entry { text: false, image: false, name, size: meta.len() }, Src::File(p.into())));
    }
    Ok(())
}

/// Sends to `addr`, which must prove it holds the private half of `expect` (its announced key).
/// `None` skips that check; only the headless test peer uses it. `check`: this device hasn't
/// verified the receiver. If either side wants the code compared, `confirm` gets it and the
/// data waits until this device's user answers through the returned channel.
#[allow(clippy::too_many_arguments)]
pub fn send(
    addr: SocketAddr,
    me: &str,
    id: &Identity,
    expect: Option<[u8; 32]>,
    check: bool,
    items: Vec<Item>,
    t: &Transfer,
    confirm: impl FnOnce(&str) -> mpsc::Receiver<bool>,
) {
    t.end(send_inner(addr, me, id, expect, check, items, t, confirm));
}

#[allow(clippy::too_many_arguments)]
fn send_inner(
    addr: SocketAddr,
    me: &str,
    id: &Identity,
    expect: Option<[u8; 32]>,
    check: bool,
    items: Vec<Item>,
    t: &Transfer,
    confirm: impl FnOnce(&str) -> mpsc::Receiver<bool>,
) -> io::Result<()> {
    let entries = expand(items)?;
    t.total.store(entries.iter().map(|(e, _)| e.size).sum(), Relaxed);
    let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    *t.sock.lock().unwrap() = Some(sock.try_clone()?);
    sock.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (secure, theirs, code) = Secure::connect(&sock, id)?;
    if expect.is_some_and(|k| k != theirs) {
        return Err(io::Error::other("Not sent: it couldn't prove it's that device."));
    }
    let mut w = BufWriter::with_capacity(BUF, secure);
    put_str(&mut w, me)?;
    w.write_all(&[check as u8])?;
    w.write_all(&u32::try_from(entries.len()).map_err(|_| bad("too many files"))?.to_le_bytes())?;
    for (e, _) in &entries {
        w.write_all(&[if e.text { 1 } else if e.image { 2 } else { 0 }])?;
        put_str(&mut w, &e.name)?;
        w.write_all(&e.size.to_le_bytes())?;
    }
    w.flush()?;
    let asked = get::<1>(w.get_mut())? == [1]; // read it even when ours already says check
    let check = check || asked;
    sock.set_read_timeout(None)?; // the receiver may take its time to accept
    let mut accepted = None;
    if check {
        // Both users answer in any order: the receiver's accept may arrive while ours is pending.
        let answer = confirm(&code);
        loop {
            match wait_user(&sock, &answer)? {
                Some(true) => break,
                Some(false) => return Err(io::Error::other("Cancelled")),
                None if accepted.is_none() => accepted = Some(get::<1>(w.get_mut())? == [1]),
                None => return Err(io::ErrorKind::ConnectionAborted.into()),
            }
            if accepted == Some(false) {
                return Err(io::Error::other("declined"));
            }
        }
    }
    let accepted = match accepted {
        Some(a) => a,
        None => get::<1>(w.get_mut())? == [1],
    };
    if !accepted {
        return Err(io::Error::other("declined"));
    }
    if check {
        w.write_all(&[1])?;
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
    if get::<1>(w.get_mut())? != [1] {
        return Err(bad("receiver failed to save"));
    }
    Ok(())
}

/// Waits for this device's user to answer while watching the connection. `None`: the other
/// device sent something or hung up first.
fn wait_user(sock: &TcpStream, answer: &mpsc::Receiver<bool>) -> io::Result<Option<bool>> {
    loop {
        match answer.recv_timeout(Duration::from_millis(200)) {
            Ok(yes) => return Ok(Some(yes)),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Some(false)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        sock.set_nonblocking(true)?;
        let r = sock.peek(&mut [0]);
        sock.set_nonblocking(false)?;
        if !matches!(r, Err(e) if e.kind() == io::ErrorKind::WouldBlock) {
            return Ok(None);
        }
    }
}

// ---------- receive ----------

/// Reads the offer and hands it to `ask`, which returns whether this device wants the code
/// compared plus a channel for the user's answer, then saves into `dir`.
/// Returns what goes on the clipboard: received texts and clipboard images.
pub fn receive(sock: TcpStream, dir: &Path, id: &Identity, t: &Transfer, ask: impl FnOnce(&Offer) -> (bool, mpsc::Receiver<bool>)) -> Vec<Clip> {
    t.end(receive_inner(sock, dir, id, t, ask)).unwrap_or_default()
}

fn receive_inner(sock: TcpStream, dir: &Path, id: &Identity, t: &Transfer, ask: impl FnOnce(&Offer) -> (bool, mpsc::Receiver<bool>)) -> io::Result<Vec<Clip>> {
    *t.sock.lock().unwrap() = Some(sock.try_clone()?);
    sock.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (secure, key, code) = Secure::accept(&sock, id)?;
    let mut r = BufReader::with_capacity(BUF, secure);
    let from = get_str(&mut r)?;
    let check = get::<1>(&mut r)? == [1];
    let n = u32::from_le_bytes(get(&mut r)?);
    let mut entries = vec![];
    for _ in 0..n {
        let kind = get::<1>(&mut r)?[0];
        let e = Entry { text: kind == 1, image: kind == 2, name: get_str(&mut r)?, size: u64::from_le_bytes(get(&mut r)?) };
        // Trust boundary: names come from the network.
        if e.text && e.size > MAX_TEXT || !e.text && !safe_rel(&e.name) {
            return Err(bad("rejected unsafe entry"));
        }
        entries.push(e);
    }
    let offer = Offer { from, key, check, code, entries };
    t.total.store(offer.entries.iter().map(|e| e.size).sum(), Relaxed);
    let (mine, answer) = ask(&offer);
    let check = check || mine;
    r.get_mut().write_all(&[check as u8])?;
    let Some(yes) = wait_user(&sock, &answer)? else {
        return Err(io::Error::other("Cancelled on the other device."));
    };
    if !yes {
        let _ = r.get_mut().write_all(&[0]);
        return Err(io::Error::other("declined"));
    }
    r.get_mut().write_all(&[1])?;
    if check {
        sock.set_read_timeout(None)?; // the sender's user is still comparing the code
        if get::<1>(&mut r).ok() != Some([1]) {
            return Err(io::Error::other("Cancelled on the other device."));
        }
    }
    t.set(State::Running);
    sock.set_read_timeout(Some(Duration::from_secs(30)))?;
    fs::create_dir_all(dir)?;

    let mut clips = vec![];
    let mut tops: HashMap<String, PathBuf> = HashMap::new(); // incoming folder -> unique local folder
    for e in &offer.entries {
        if e.text {
            let mut b = vec![0; e.size as usize];
            r.read_exact(&mut b)?;
            clips.push(Clip::Text(String::from_utf8_lossy(&b).into_owned()));
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
        if e.image {
            clips.push(Clip::Image(path));
        }
    }
    r.get_mut().write_all(&[1])?;
    Ok(clips)
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

// ---------- encryption ----------

const NOISE: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const FRAME: usize = 65535; // Noise's message limit
const TAG: usize = 16;

/// This device's permanent X25519 key pair. The public half is its identity on the network.
pub struct Identity {
    secret: Vec<u8>,
    pub public: [u8; 32],
}

impl Identity {
    pub fn generate() -> io::Result<Self> {
        let k = noise()?.generate_keypair().map_err(crypto)?;
        Ok(Self { secret: k.private, public: k.public.try_into().map_err(|_| bad("bad key size"))? })
    }

    /// Reads `path` (secret ‖ public, 64 bytes), or creates it (owner-only) on first run.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        if let Ok(b) = fs::read(path)
            && b.len() == 64
        {
            return Ok(Self { secret: b[..32].to_vec(), public: b[32..].try_into().unwrap() });
        }
        let id = Self::generate()?;
        let mut o = fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        o.open(path)?.write_all(&[id.secret.as_slice(), &id.public].concat())?;
        Ok(id)
    }
}

/// The transfer stream after MAGIC: a Noise XX handshake (fresh ephemeral keys plus both devices'
/// permanent keys), then ChaCha20-Poly1305 frames (u16 length + ciphertext) both ways.
struct Secure<'a> {
    sock: &'a TcpStream,
    noise: snow::TransportState,
    plain: Vec<u8>, // decrypted bytes not yet read
    pos: usize,
    frame: Vec<u8>,
}

impl<'a> Secure<'a> {
    /// Sender side: MAGIC, then the initiator half of the handshake.
    /// Returns the receiver's proven key and the connection's code.
    fn connect(sock: &'a TcpStream, me: &Identity) -> io::Result<(Self, [u8; 32], String)> {
        let mut hs = noise()?.local_private_key(&me.secret).map_err(crypto)?.build_initiator().map_err(crypto)?;
        let mut buf = vec![0; FRAME];
        let mut nonce = [0; 32];
        DefaultResolver.resolve_rng().ok_or_else(|| bad("no random source"))?.try_fill_bytes(&mut nonce).map_err(crypto)?;
        let n = hs.write_message(&commit(&me.public, &nonce), &mut buf).map_err(crypto)?; // -> e, commitment
        put_frame(sock, MAGIC, &buf[..n])?;
        hs.read_message(&get_frame(sock)?, &mut buf).map_err(crypto)?; // <- e, ee, s, es
        let n = hs.write_message(&nonce, &mut buf).map_err(crypto)?; // -> s, se, nonce
        put_frame(sock, &[], &buf[..n])?;
        Self::new(sock, hs)
    }

    /// Receiver side: checks MAGIC, then answers the handshake.
    /// Returns the sender's proven key and the connection's code.
    fn accept(sock: &'a TcpStream, me: &Identity) -> io::Result<(Self, [u8; 32], String)> {
        if &get::<4>(&mut &*sock)? != MAGIC {
            return Err(bad("not a FlashHop sender (or a different version)"));
        }
        let mut hs = noise()?.local_private_key(&me.secret).map_err(crypto)?.build_responder().map_err(crypto)?;
        let mut buf = vec![0; FRAME];
        let n = hs.read_message(&get_frame(sock)?, &mut buf).map_err(crypto)?;
        let promised = buf[..n].to_vec();
        let n = hs.write_message(&[], &mut buf).map_err(crypto)?;
        put_frame(sock, &[], &buf[..n])?;
        let n = hs.read_message(&get_frame(sock)?, &mut buf).map_err(crypto)?;
        let theirs = hs.get_remote_static().ok_or_else(|| bad("no device key"))?;
        if commit(theirs, &buf[..n]) != *promised {
            return Err(bad("encryption: the sender broke its commitment"));
        }
        Self::new(sock, hs)
    }

    fn new(sock: &'a TcpStream, hs: snow::HandshakeState) -> io::Result<(Self, [u8; 32], String)> {
        let theirs = hs.get_remote_static().and_then(|k| k.try_into().ok()).ok_or_else(|| bad("no device key"))?;
        let c = u32::from_le_bytes(hs.get_handshake_hash()[..4].try_into().unwrap()) % 1_000_000;
        let noise = hs.into_transport_mode().map_err(crypto)?;
        Ok((Self { sock, noise, plain: vec![], pos: 0, frame: vec![0; FRAME + 2] }, theirs, format!("{:03} {:03}", c / 1000, c % 1000)))
    }
}

/// The sender's commitment in handshake message 1: it can't change its key or nonce after
/// seeing the receiver's, so a man in the middle can't make two connections show one code.
fn commit(key: &[u8], nonce: &[u8]) -> [u8; 32] {
    let mut h = DefaultResolver.resolve_hash(&HashChoice::Blake2s).expect("blake2s is compiled in");
    h.input(key);
    h.input(nonce);
    let mut out = [0; 32];
    h.result(&mut out);
    out
}

impl Read for Secure<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.plain.len() {
            let mut s = self.sock;
            let len = u16::from_le_bytes(get(&mut s)?) as usize;
            s.read_exact(&mut self.frame[..len])?;
            self.plain.resize(len, 0);
            let n = self.noise.read_message(&self.frame[..len], &mut self.plain).map_err(crypto)?;
            self.plain.truncate(n);
            self.pos = 0;
        }
        let n = out.len().min(self.plain.len() - self.pos);
        out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Write for Secure<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(FRAME - TAG);
        if n == 0 {
            return Ok(0);
        }
        let len = self.noise.write_message(&buf[..n], &mut self.frame[2..]).map_err(crypto)?;
        self.frame[..2].copy_from_slice(&(len as u16).to_le_bytes());
        (&*self.sock).write_all(&self.frame[..len + 2])?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn noise() -> io::Result<snow::Builder<'static>> {
    snow::Builder::new(NOISE.parse().map_err(crypto)?).prologue(MAGIC).map_err(crypto)
}

fn put_frame(mut s: &TcpStream, prefix: &[u8], b: &[u8]) -> io::Result<()> {
    s.write_all(&[prefix, &(b.len() as u16).to_le_bytes(), b].concat())
}

fn get_frame(mut s: &TcpStream) -> io::Result<Vec<u8>> {
    let mut b = vec![0; u16::from_le_bytes(get(&mut s)?) as usize];
    s.read_exact(&mut b)?;
    Ok(b)
}

fn crypto(e: snow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("encryption: {e}"))
}

// ---------- wire helpers ----------

/// Copies exactly `len` bytes in chunks so progress can update.
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
        let root = std::env::temp_dir().join(format!("flashhop-test-{}", std::process::id()));
        let (src, dst) = (root.join("src/Photos"), root.join("dst"));
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.bin"), vec![7u8; 9 << 20]).unwrap(); // > CHUNK, exercises the loop
        fs::write(src.join("sub/b.txt"), "hi").unwrap();
        fs::create_dir_all(dst.join("Photos")).unwrap(); // existing folder -> must become "Photos (1)"

        let (pc, phone) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let (pc_key, phone_key) = (pc.public, phone.public);
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let rx = std::thread::spawn(move || {
            let t = Transfer::new();
            let clips = receive(l.accept().unwrap().0, &dst, &phone, &t, |o| answered(o.from == "pc" && o.key == pc_key && o.entries.len() == 6 && !o.check));
            (clips, t.state())
        });
        let t = Transfer::new();
        let open = File::open(src.join("sub/b.txt")).unwrap();
        let img = File::open(src.join("sub/b.txt")).unwrap();
        let nested = File::open(src.join("sub/b.txt")).unwrap(); // Android folder send: fd + relative path
        let items = vec![
            Item::Path(src.clone()),
            Item::Text("clip".into()),
            Item::Open(open, "../fd.txt".into()),
            Item::Image(img, "c.png".into()),
            Item::Open(nested, "Album/sub/n.txt".into()),
        ];
        send(addr, "pc", &pc, Some(phone_key), false, items, &t, |_| panic!("both sides trust each other: no code"));
        let (clips, rstate) = rx.join().unwrap();
        assert_eq!((t.state(), rstate), (State::Done, State::Done));
        assert_eq!(clips, [Clip::Text("clip".into()), Clip::Image(root.join("dst/c.png"))]);
        assert_eq!(fs::read_to_string(root.join("dst/c.png")).unwrap(), "hi"); // image is saved too
        assert_eq!(fs::read(root.join("dst/Photos (1)/a.bin")).unwrap().len(), 9 << 20);
        assert_eq!(fs::read_to_string(root.join("dst/Photos (1)/sub/b.txt")).unwrap(), "hi");
        assert_eq!(fs::read_to_string(root.join("dst/.._fd.txt")).unwrap(), "hi"); // fd item, '/' neutralized
        assert_eq!(fs::read_to_string(root.join("dst/Album/sub/n.txt")).unwrap(), "hi"); // safe path kept

        for evil in ["../x", "/etc/passwd", "a/../../x", "./a", ""] {
            assert!(!safe_rel(evil), "{evil}");
        }
        assert!(safe_rel("Photos/sub/b.txt"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn impostor_is_refused() {
        let (pc, real, impostor) = (Identity::generate().unwrap(), Identity::generate().unwrap(), Identity::generate().unwrap());
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let rx = std::thread::spawn(move || {
            let t = Transfer::new();
            let mut asked = false;
            receive(l.accept().unwrap().0, Path::new("/nonexistent"), &impostor, &t, |_| {
                asked = true;
                answered(true)
            });
            asked
        });
        let t = Transfer::new();
        send(addr, "pc", &pc, Some(real.public), true, vec![Item::Text("secret".into())], &t, |_| answered(true).1); // expects `real`
        assert!(matches!(t.state(), State::Failed(e) if e.contains("couldn't prove")), "{:?}", t.state());
        assert!(!rx.join().unwrap(), "the impostor never sees the offer");
    }

    /// An `ask` result: no code check from this side, and the user's answer already given.
    fn answered(yes: bool) -> (bool, mpsc::Receiver<bool>) {
        let (tx, rx) = mpsc::channel();
        tx.send(yes).unwrap();
        (false, rx)
    }

    /// A loopback text send with the sender asking for the code check. Each side's user answers
    /// `Some(yes)` (the sender after a delay, so the receiver's answer comes first) or never.
    fn checked(sender: Option<bool>, receiver: Option<bool>) -> (State, State, String, String) {
        let (pc, phone) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let phone_key = phone.public;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let rx = std::thread::spawn(move || {
            let (t, mut code, (tx, answer)) = (Transfer::new(), String::new(), mpsc::channel());
            if let Some(yes) = receiver {
                tx.send(yes).unwrap();
            }
            receive(l.accept().unwrap().0, &std::env::temp_dir(), &phone, &t, |o| {
                assert!(o.check);
                code = o.code.clone();
                (false, answer)
            });
            drop(tx); // a user who never answers keeps the channel open
            (t.state(), code)
        });
        let (t, mut code, (tx, answer)) = (Transfer::new(), String::new(), mpsc::channel());
        if let Some(yes) = sender {
            let tx = tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                let _ = tx.send(yes);
            });
        }
        send(addr, "pc", &pc, Some(phone_key), true, vec![Item::Text("hi".into())], &t, |c| {
            code = c.into();
            answer
        });
        drop(tx);
        let (rstate, rcode) = rx.join().unwrap();
        (t.state(), rstate, code, rcode)
    }

    #[test]
    fn code_check() {
        let (s, r, code, rcode) = checked(Some(true), Some(true));
        assert_eq!((s, r), (State::Done, State::Done));
        assert_eq!(code, rcode, "both screens show the same code");
        assert!(code.len() == 7 && code.chars().enumerate().all(|(i, c)| if i == 3 { c == ' ' } else { c.is_ascii_digit() }), "{code}");

        // The receiver declines while the sender's card is open: the sender stops without an answer.
        let (s, r, ..) = checked(None, Some(false));
        assert_eq!((s, r), (State::Failed("Declined".into()), State::Failed("Declined".into())));

        // The sender cancels (codes differ) while the offer is open: the offer goes away by itself.
        let (s, r, ..) = checked(Some(false), None);
        assert_eq!((s, r), (State::Failed("Cancelled".into()), State::Failed("Cancelled on the other device.".into())));
    }

    #[test]
    fn broken_commitment_is_refused() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let sock = TcpStream::connect(addr).unwrap();
            let me = Identity::generate().unwrap();
            let mut hs = noise().unwrap().local_private_key(&me.secret).unwrap().build_initiator().unwrap();
            let mut buf = vec![0; FRAME];
            let n = hs.write_message(&commit(&me.public, &[1; 32]), &mut buf).unwrap();
            put_frame(&sock, MAGIC, &buf[..n]).unwrap();
            hs.read_message(&get_frame(&sock).unwrap(), &mut buf).unwrap();
            let n = hs.write_message(&[2; 32], &mut buf).unwrap(); // not the nonce it promised
            put_frame(&sock, &[], &buf[..n]).unwrap();
        });
        let (sock, _) = l.accept().unwrap();
        assert!(Secure::accept(&sock, &Identity::generate().unwrap()).is_err());
    }

    #[test]
    fn wire_is_encrypted() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let tx = std::thread::spawn(move || {
            let sock = TcpStream::connect(addr).unwrap();
            let mut s = Secure::connect(&sock, &Identity::generate().unwrap()).unwrap().0;
            s.write_all(b"secret-marker").unwrap();
            let mut back = [0; 2];
            s.read_exact(&mut back).unwrap();
            back
        });
        let (sock, _) = l.accept().unwrap();
        let mut s = Secure::accept(&sock, &Identity::generate().unwrap()).unwrap().0;
        let mut raw = [0; 2 + 13 + TAG]; // one frame, read straight off the socket
        (&sock).read_exact(&mut raw).unwrap();
        assert!(!raw.windows(13).any(|w| w == b"secret-marker"), "plaintext on the wire");
        let mut out = [0; 64];
        let n = s.noise.read_message(&raw[2..], &mut out).unwrap();
        assert_eq!(&out[..n], b"secret-marker");
        s.write_all(b"ok").unwrap();
        assert_eq!(&tx.join().unwrap(), b"ok");
    }
}
