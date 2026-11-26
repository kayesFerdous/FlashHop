//! The whole app minus the screens. Desktop (egui) and Android (Kotlin over JNI) are thin
//! views over `Engine`, so discovery, offers, transfers, naming and wording stay identical.

pub mod net;

use net::{Item, State, Transfer};
use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    io,
    net::{IpAddr, SocketAddr, TcpStream},
    path::PathBuf,
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
}

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
    reply: mpsc::Sender<bool>,
    job: Job,
}

pub struct Engine {
    pub name: Mutex<String>,
    pub dir: Mutex<PathBuf>,
    peers: Mutex<HashMap<u64, net::Peer>>,
    pending: Mutex<Vec<Pending>>,
    jobs: Mutex<Vec<Job>>,
    texts: Mutex<Vec<String>>,
    generation: Mutex<u64>,
    cv: Condvar,
    ids: AtomicU64,
}

impl Engine {
    /// Binds the port and starts discovery + the receiver. Fails if another copy is running.
    pub fn start(name: String, dir: PathBuf) -> io::Result<Arc<Self>> {
        let (udp, tcp) = (net::discovery_socket()?, net::listen()?);
        let e = Arc::new(Self {
            name: Mutex::new(name),
            dir: Mutex::new(dir),
            peers: Default::default(),
            pending: Default::default(),
            jobs: Default::default(),
            texts: Default::default(),
            generation: Mutex::new(0),
            cv: Condvar::new(),
            ids: AtomicU64::new(1),
        });
        let id = RandomState::new().hash_one(std::process::id());
        let e2 = e.clone();
        thread::spawn(move || net::discovery(udp, id, &e2.name, &e2.peers, || e2.changed()));
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
        let texts = net::receive(conn, &dir, &t, |o| {
            let (reply, decision) = mpsc::channel();
            let offer = Offer { id: self.next_id(), summary: summarize(o) };
            let job = Job { id: self.next_id(), label: format!("From {}", o.from), incoming: true, t: t.clone() };
            self.pending.lock().unwrap().push(Pending { offer, reply, job });
            self.changed();
            decision.recv().unwrap_or(false)
        });
        log_end("receive", &t);
        self.texts.lock().unwrap().extend(texts);
        self.changed();
    }

    pub fn send(self: &Arc<Self>, peer: u64, items: Vec<Item>) {
        let Some((name, ip)) = self.peers.lock().unwrap().get(&peer).map(|p| (p.name.clone(), p.ip)) else { return };
        let t = Arc::new(Transfer::new());
        self.jobs.lock().unwrap().push(Job { id: self.next_id(), label: format!("To {name}"), incoming: false, t: t.clone() });
        let (e, me) = (self.clone(), self.name.lock().unwrap().clone());
        thread::spawn(move || {
            net::send(SocketAddr::new(ip, net::PORT), &me, items, &t);
            log_end(&format!("send to {name} ({ip})"), &t);
            e.changed();
        });
        self.changed();
    }

    pub fn decide(&self, offer: u64, accept: bool) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(i) = pending.iter().position(|p| p.offer.id == offer) {
            let p = pending.remove(i);
            if accept {
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

    pub fn peers(&self) -> Vec<Peer> {
        let mut v: Vec<_> = self.peers.lock().unwrap().iter().map(|(&id, p)| Peer { id, name: p.name.clone(), ip: p.ip }).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
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

    /// Received text snippets, handed over once so the UI can put them on the clipboard.
    pub fn take_texts(&self) -> Vec<String> {
        std::mem::take(&mut self.texts.lock().unwrap())
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

pub fn summarize(o: &net::Offer) -> String {
    let total: u64 = o.entries.iter().map(|e| e.size).sum();
    let mut names: Vec<&str> = o.entries.iter().map(|e| if e.text { "text" } else { e.name.split('/').next().unwrap() }).collect();
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
