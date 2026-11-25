//! JNI bridge: `app.akhripasta.Core` (Kotlin) -> `akhri_core::Engine`. No logic lives here;
//! everything the desktop app does, the phone does through the same engine.

use akhri_core::{Engine, net::Item};
use jni::{
    JNIEnv,
    objects::{JIntArray, JObject, JObjectArray, JString},
    sys::{jboolean, jlong, jobjectArray, jstring},
};
use std::{
    fs::File,
    os::fd::FromRawFd,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
const SEP: char = '\u{1f}'; // field separator in snapshot records

fn engine() -> Option<&'static Arc<Engine>> {
    ENGINE.get()
}

fn string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn strings(env: &mut JNIEnv, arr: &JObjectArray) -> Vec<String> {
    let n = env.get_array_length(arr).unwrap_or(0);
    let mut out = vec![];
    for i in 0..n {
        if let Ok(o) = env.get_object_array_element(arr, i) {
            out.push(string(env, &JString::from(o)));
        }
    }
    out
}

/// Returns null on success, or an error message.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_start(mut env: JNIEnv, _: JObject, name: JString, dir: JString) -> jstring {
    let (name, dir) = (string(&mut env, &name), PathBuf::from(string(&mut env, &dir)));
    if let Some(e) = engine() {
        *e.name.lock().unwrap() = name;
        *e.dir.lock().unwrap() = dir;
        return std::ptr::null_mut();
    }
    // A cdylib doesn't get Rust's usual SIGPIPE=ignore; without it a closed socket kills the app.
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    unsafe { signal(13, 1) }; // SIGPIPE -> SIG_IGN
    match Engine::start(name, dir) {
        Ok(e) => {
            let _ = ENGINE.set(e);
            std::ptr::null_mut()
        }
        Err(err) => env.new_string(format!("Network error: {err}")).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_setName(mut env: JNIEnv, _: JObject, name: JString) {
    let name = string(&mut env, &name);
    if let Some(e) = engine() {
        *e.name.lock().unwrap() = name;
    }
}

/// Blocks until something changes (or the timeout). Call from a background thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_waitChange(_: JNIEnv, _: JObject, seen: jlong, timeout_ms: jlong) -> jlong {
    match engine() {
        Some(e) => e.wait(seen as u64, Duration::from_millis(timeout_ms as u64)) as jlong,
        None => {
            std::thread::sleep(Duration::from_millis(timeout_ms as u64));
            seen
        }
    }
}

/// One record per string, fields separated by U+001F:
/// `P id name ip` | `O id summary` | `J id label active failed fraction status` | `T text`
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_snapshot(mut env: JNIEnv, _: JObject) -> jobjectArray {
    let mut recs: Vec<String> = vec![];
    if let Some(e) = engine() {
        let clean = |s: &str| s.replace(SEP, " ");
        for p in e.peers() {
            recs.push(format!("P{SEP}{}{SEP}{}{SEP}{}", p.id as i64, clean(&p.name), p.ip));
        }
        for o in e.offers() {
            recs.push(format!("O{SEP}{}{SEP}{}", o.id as i64, clean(&o.summary)));
        }
        for j in e.jobs() {
            let failed = matches!(j.t.state(), akhri_core::net::State::Failed(_));
            recs.push(format!(
                "J{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}",
                j.id as i64,
                clean(&j.label),
                j.active() as u8,
                failed as u8,
                j.fraction(),
                clean(&j.status())
            ));
        }
        for t in e.take_texts() {
            recs.push(format!("T{SEP}{t}"));
        }
    }
    let build = |env: &mut JNIEnv| -> jni::errors::Result<jobjectArray> {
        let arr = env.new_object_array(recs.len() as i32, "java/lang/String", JObject::null())?;
        for (i, r) in recs.iter().enumerate() {
            let s = env.new_string(r)?;
            env.set_object_array_element(&arr, i as i32, s)?;
        }
        Ok(arr.into_raw())
    };
    build(&mut env).unwrap_or(std::ptr::null_mut())
}

/// `fds` are detached file descriptors (ownership moves to Rust), `names` their display names.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_send(
    mut env: JNIEnv,
    _: JObject,
    peer: jlong,
    fds: JIntArray,
    names: JObjectArray,
    texts: JObjectArray,
) {
    let n = env.get_array_length(&fds).unwrap_or(0) as usize;
    let mut raw = vec![0; n];
    let _ = env.get_int_array_region(&fds, 0, &mut raw);
    let names = strings(&mut env, &names);
    let mut items: Vec<Item> = raw.into_iter().zip(names).map(|(fd, name)| Item::Open(unsafe { File::from_raw_fd(fd) }, name)).collect();
    items.extend(strings(&mut env, &texts).into_iter().map(Item::Text));
    if let Some(e) = engine() {
        e.send(peer as u64, items);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_decide(_: JNIEnv, _: JObject, offer: jlong, accept: jboolean) {
    if let Some(e) = engine() {
        e.decide(offer as u64, accept != 0);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_cancel(_: JNIEnv, _: JObject, job: jlong) {
    if let Some(e) = engine() {
        e.cancel(job as u64);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_akhripasta_Core_clearFinished(_: JNIEnv, _: JObject) {
    if let Some(e) = engine() {
        e.clear_finished();
    }
}
