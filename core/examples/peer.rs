//! Headless peer for testing the apps against each other. Same engine as the apps; auto-accepts.
//!   cargo run -p flashhop_core --example peer -- recv <save-dir>
//!   cargo run -p flashhop_core --example peer -- send <ip:port> <file-or-folder | text:hello>...
use flashhop_core::{Engine, human, net};
use std::{collections::HashSet, time::Duration};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("recv"), Some(dir)) => {
            let e = Engine::start("test-peer".into(), dir.into()).expect("port 47821 busy?");
            let (mut seen, mut printed) = (0, HashSet::new());
            loop {
                seen = e.wait(seen, Duration::from_secs(1));
                for o in e.offers() {
                    println!("offer: {}  -> accepting", o.summary);
                    e.decide(o.id, true);
                }
                for c in e.take_clips() {
                    println!("clipboard: {c:?}");
                }
                for j in e.jobs().into_iter().filter(|j| !j.active() && printed.insert(j.id)) {
                    println!("{}: {}", j.label, j.status());
                }
            }
        }
        (Some("send"), Some(addr)) => {
            let t = net::Transfer::new();
            let items = args[2..]
                .iter()
                .map(|a| {
                    if let Some(t) = a.strip_prefix("text:") {
                        net::Item::Text(t.into())
                    } else if let Some(p) = a.strip_prefix("image:") {
                        net::Item::Image(std::fs::File::open(p).expect("image file"), "clipboard.png".into())
                    } else {
                        net::Item::Path(a.into())
                    }
                })
                .collect();
            net::send(addr.parse().expect("ip:port"), "test-peer", items, &t);
            println!("{:?}, {} at {}/s", t.state(), human(t.done.load(std::sync::atomic::Ordering::Relaxed)), human(t.speed() as u64));
        }
        _ => eprintln!("usage: peer recv <dir> | peer send <ip:port> <path>|text:<s>|image:<png>..."),
    }
}
