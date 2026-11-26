//! The whole app minus the screens. Desktop (egui) and Android (Kotlin over JNI) are thin
//! views over `Engine`, so discovery, offers, transfers, naming and wording stay identical.

pub mod net;

use net::{Clip, Item, State, Transfer};
use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    fs, io,
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering::Relaxed},
        mpsc,
    },
    thread,
    time::Duration,
};

pub struct Peer {
    pub id: u64,
    pub name: String,
    pub ip: IpAddr,
    /// Sent to or accepted from lately; these sort first.
    pub recent: bool,
    pub trust: Trust,
}

/// What this device knows about a peer's key (trust on first use, pinned per device name).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Trust {
    /// Never exchanged anything with a device of this name.
    New,
    /// Same key as every earlier transfer.
    Verified,
    /// A device of this name is known, but with a different key: reinstalled, a second device
    /// with the same name, or an impostor. Sending to it is refused until it is accepted once.
    Changed,
}

impl Peer {
    /// The line under the name in both UIs.
    pub fn detail(&self) -> String {
        match self.trust {
            Trust::Changed => format!("Not verified: new key · {}", self.ip),
            Trust::Verified => format!("Verified · {}", self.ip),
            Trust::New if self.recent => format!("Recent · {}", self.ip),
            Trust::New => self.ip.to_string(),
        }
    }
}

/// How many recent device names `Engine::recent` keeps.
const RECENT: usize = 5;

#[derive(Clone)]
pub struct Offer {
    pub id: u64,
    pub summary: String,
}

#[derive(Clone)]
pub struct Job {
    pub id: u64,
    pub label: String,
    /// Received (true) or sent (false).
    pub incoming: bool,
    pub t: Arc<Transfer>,
}

impl Job {
    pub fn active(&self) -> bool {
        matches!(self.t.state(), State::Waiting | State::Running)
    }
    /// One line both UIs show under the label.
    pub fn status(&self) -> String {
        let (done, total) = (self.t.done.load(Relaxed), self.t.total.load(Relaxed));
        match self.t.state() {
            State::Waiting if self.incoming => "Starting…".into(),
            State::Waiting => "Waiting for the other device to accept…".into(),
            State::Running => format!("{} / {}  {}/s", human(done), human(total), human(self.t.speed() as u64)),
            State::Done => format!("{}, {}", if self.incoming { "Received" } else { "Sent" }, human(total)),
            State::Failed(e) => e,
        }
    }
    /// 0.0..=1.0
    pub fn fraction(&self) -> f32 {
        let total = self.t.total.load(Relaxed);
        if total == 0 { 1.0 } else { self.t.done.load(Relaxed) as f32 / total as f32 }
    }
}

struct Pending {
    offer: Offer,
    from: String,
    key: [u8; 32],
    reply: mpsc::Sender<bool>,
    job: Job,
}

pub struct Engine {
    pub name: Mutex<String>,
    pub dir: Mutex<PathBuf>,
    /// Names of devices sent to or accepted from, newest first. Platforms persist it.
    pub recent: Mutex<Vec<String>>,
    me: net::Identity,
    /// (device name, key) pairs seen in completed or accepted transfers; saved in `state/known`.
    known: Mutex<Vec<(String, [u8; 32])>>,
    state: PathBuf,
    peers: Mutex<HashMap<u64, net::Peer>>,
    pending: Mutex<Vec<Pending>>,
    jobs: Mutex<Vec<Job>>,
    clips: Mutex<Vec<Clip>>,
    generation: Mutex<u64>,
    cv: Condvar,
    ids: AtomicU64,
}

impl Engine {
    /// Binds the port and starts discovery + the receiver. Fails if another copy is running.
    /// `state` is a private folder for this device's key (`identity`) and trusted keys (`known`).
    pub fn start(name: String, dir: PathBuf, state: PathBuf) -> io::Result<Arc<Self>> {
        let (udp, tcp) = (net::discovery_socket()?, net::listen()?);
        fs::create_dir_all(&state)?;
        let me = net::Identity::load_or_create(&state.join("identity"))?;
        let e = Arc::new(Self {
            name: Mutex::new(name),
            dir: Mutex::new(dir),
            recent: Default::default(),
            me,
            known: Mutex::new(load_known(&state.join("known"))),
            state,
            peers: Default::default(),
            pending: Default::default(),
            jobs: Default::default(),
            clips: Default::default(),
            generation: Mutex::new(0),
            cv: Condvar::new(),
            ids: AtomicU64::new(1),
        });
        let id = RandomState::new().hash_one(std::process::id());
        let e2 = e.clone();
        thread::spawn(move || net::discovery(udp, id, e2.me.public, &e2.name, &e2.peers, || e2.changed()));
        let e2 = e.clone();
        thread::spawn(move || {
            for conn in tcp.incoming().flatten() {
                let e = e2.clone();
                thread::spawn(move || e.receive(conn));
            }
        });
        Ok(e)
    }

    fn receive(&self, conn: TcpStream) {
        let t = Arc::new(Transfer::new());
        let dir = self.dir.lock().unwrap().clone();
        let clips = net::receive(conn, &dir, &self.me, &t, |o| {
            let (reply, decision) = mpsc::channel();
            let mut summary = summarize(o);
            if self.trust(&o.from, &o.key) == Trust::Changed {
                summary += &format!(". This isn't the {} you used before; accept only if it's new or reinstalled.", o.from);
            }
            let offer = Offer { id: self.next_id(), summary };
            let job = Job { id: self.next_id(), label: format!("From {}", o.from), incoming: true, t: t.clone() };
            self.pending.lock().unwrap().push(Pending { offer, from: o.from.clone(), key: o.key, reply, job });
            self.changed();
            decision.recv().unwrap_or(false)
        });
        log_end("receive", &t);
        self.clips.lock().unwrap().extend(clips);
        self.changed();
    }

    pub fn send(self: &Arc<Self>, peer: u64, items: Vec<Item>) {
        let Some((name, ip, key)) = self.peers.lock().unwrap().get(&peer).map(|p| (p.name.clone(), p.ip, p.key)) else { return };
        let t = Arc::new(Transfer::new());
        self.jobs.lock().unwrap().push(Job { id: self.next_id(), label: format!("To {name}"), incoming: false, t: t.clone() });
        if self.trust(&name, &key) == Trust::Changed {
            // The offer card on the other side explains; here one short line has to fit a phone.
            t.end::<()>(Err(io::Error::other("Not sent: unrecognized key. Accept a send from it first.")));
            return self.changed();
        }
        self.touch(&name);
        let (e, me) = (self.clone(), self.name.lock().unwrap().clone());
        thread::spawn(move || {
            net::send(SocketAddr::new(ip, net::PORT), &me, &e.me, Some(key), items, &t);
            log_end(&format!("send to {name} ({ip})"), &t);
            if t.state() == State::Done {
                e.pin(&name, key);
            }
            e.changed();
        });
        self.changed();
    }

    pub fn decide(&self, offer: u64, accept: bool) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(i) = pending.iter().position(|p| p.offer.id == offer) {
            let p = pending.remove(i);
            if accept {
                self.touch(&p.from);
                self.pin(&p.from, p.key);
                self.jobs.lock().unwrap().push(p.job);
            }
            let _ = p.reply.send(accept);
        }
        drop(pending);
        self.changed();
    }

    pub fn cancel(&self, job: u64) {
        if let Some(j) = self.jobs.lock().unwrap().iter().find(|j| j.id == job) {
            j.t.cancel();
        }
    }

    pub fn clear_finished(&self) {
        self.jobs.lock().unwrap().retain(Job::active);
        self.changed();
    }

    /// Recent devices first (newest first), then by name.
    pub fn peers(&self) -> Vec<Peer> {
        let all = self.peers.lock().unwrap().iter()
            .map(|(&id, p)| Peer { id, name: p.name.clone(), ip: p.ip, recent: false, trust: self.trust(&p.name, &p.key) })
            .collect();
        by_recent(all, &self.recent.lock().unwrap())
    }

    /// This device's public key (what discovery announces).
    pub fn key(&self) -> [u8; 32] {
        self.me.public
    }

    fn trust(&self, name: &str, key: &[u8; 32]) -> Trust {
        trust(&self.known.lock().unwrap(), name, key)
    }

    /// Remembers `key` for `name` (several keys per name are fine: two phones called "Pixel 8a").
    fn pin(&self, name: &str, key: [u8; 32]) {
        let mut known = self.known.lock().unwrap();
        if trust(&known, name, &key) != Trust::Verified {
            known.push((name.to_owned(), key));
            let txt: String = known.iter().map(|(n, k)| format!("{} {}\n", hex(k), n.replace('\n', " "))).collect();
            let _ = fs::write(self.state.join("known"), txt);
        }
    }

    fn touch(&self, name: &str) {
        push_recent(&mut self.recent.lock().unwrap(), name);
    }

    pub fn offers(&self) -> Vec<Offer> {
        self.pending.lock().unwrap().iter().map(|p| p.offer.clone()).collect()
    }

    /// Newest first.
    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().iter().rev().cloned().collect()
    }

    pub fn busy(&self) -> bool {
        self.jobs.lock().unwrap().iter().any(Job::active)
    }

    /// Received texts and clipboard images, handed over once so the UI can put them on the clipboard.
    pub fn take_clips(&self) -> Vec<Clip> {
        std::mem::take(&mut self.clips.lock().unwrap())
    }

    pub fn changed(&self) {
        *self.generation.lock().unwrap() += 1;
        self.cv.notify_all();
    }

    /// Blocks until anything changes after generation `seen` (or `timeout`); returns the current one.
    pub fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let g = self.generation.lock().unwrap();
        *self.cv.wait_timeout_while(g, timeout, |g| *g == seen).unwrap().0
    }

    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Relaxed)
    }
}

fn trust(known: &[(String, [u8; 32])], name: &str, key: &[u8; 32]) -> Trust {
    if known.iter().any(|(n, k)| n == name && k == key) {
        Trust::Verified
    } else if known.iter().any(|(n, _)| n == name) {
        Trust::Changed
    } else {
        Trust::New
    }
}

/// `known` file: one `<64 hex digits> <device name>` line per trusted key.
fn load_known(path: &Path) -> Vec<(String, [u8; 32])> {
    let txt = fs::read_to_string(path).unwrap_or_default();
    txt.lines()
        .filter_map(|l| {
            let (k, name) = l.split_once(' ')?;
            let b: Vec<u8> = (0..k.len()).step_by(2).map(|i| u8::from_str_radix(k.get(i..i + 2)?, 16).ok()).collect::<Option<_>>()?;
            Some((name.to_owned(), b.try_into().ok()?))
        })
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn push_recent(r: &mut Vec<String>, name: &str) {
    r.retain(|n| n != name);
    r.insert(0, name.to_owned());
    r.truncate(RECENT);
}

fn by_recent(mut v: Vec<Peer>, recent: &[String]) -> Vec<Peer> {
    let rank = |name: &str| recent.iter().position(|r| r == name).unwrap_or(usize::MAX);
    for p in &mut v {
        p.recent = rank(&p.name) != usize::MAX;
    }
    v.sort_by(|a, b| (rank(&a.name), &a.name).cmp(&(rank(&b.name), &b.name)));
    v
}

pub fn summarize(o: &net::Offer) -> String {
    let total: u64 = o.entries.iter().map(|e| e.size).sum();
    let mut names: Vec<&str> = o.entries.iter().map(|e| if e.text { "text" } else if e.image { "image" } else { e.name.split('/').next().unwrap() }).collect();
    names.dedup();
    let more = if names.len() > 3 { format!(" +{} more", names.len() - 3) } else { String::new() };
    names.truncate(3);
    format!("{} wants to send {} ({}){}", o.from, names.join(", "), human(total), more)
}

/// Two-tone headline naming the current step, as (strong, rest). `touch` picks the phone's verb.
pub fn headline(peers: usize, staged: usize, touch: bool) -> (String, &'static str) {
    let count = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    if peers == 0 {
        ("Searching".into(), " for devices on this network…")
    } else if staged == 0 {
        (count(peers, "device"), if touch { " nearby. Share something to send." } else { " nearby. Drop something to send." })
    } else {
        (count(staged, "item"), " ready. Pick a device.")
    }
}

pub fn human(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let i = (b.max(1).ilog(1024) as usize).min(units.len() - 1);
    if i == 0 { format!("{b} B") } else { format!("{:.1} {}", b as f64 / 1024f64.powi(i as i32), units[i]) }
}

fn log_end(what: &str, t: &Transfer) {
    eprintln!("{what}: {:?}, {} at {}/s", t.state(), human(t.done.load(Relaxed)), human(t.speed() as u64));
}

#[cfg(test)]
mod tests {
    #[test]
    fn recent_devices_sort_first() {
        let mut recent = vec![];
        for n in ["b", "a", "b", "c", "d", "e", "f"] {
            super::push_recent(&mut recent, n);
        }
        assert_eq!(recent, ["f", "e", "d", "c", "b"], "newest first, no repeats, capped");
        let peer = |name: &str| super::Peer { id: 0, name: name.into(), ip: [10, 0, 0, 1].into(), recent: false, trust: super::Trust::New };
        let v = super::by_recent(vec![peer("z"), peer("c"), peer("a"), peer("f")], &recent);
        let got: Vec<_> = v.iter().map(|p| (p.name.as_str(), p.recent)).collect();
        assert_eq!(got, [("f", true), ("c", true), ("a", false), ("z", false)]);
    }

    #[test]
    fn trust_on_first_use() {
        use super::Trust::*;
        let path = std::env::temp_dir().join(format!("flashhop-known-{}", std::process::id()));
        let (a, b) = ([1u8; 32], [2u8; 32]);
        std::fs::write(&path, format!("{} Pixel 8a\ngarbage line\n", super::hex(&a))).unwrap();
        let known = super::load_known(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(known, [("Pixel 8a".to_string(), a)], "hex round trip, bad lines skipped");
        assert_eq!(super::trust(&known, "Pixel 8a", &a), Verified);
        assert_eq!(super::trust(&known, "Pixel 8a", &b), Changed);
        assert_eq!(super::trust(&known, "Laptop", &b), New);
    }
}
