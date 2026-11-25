# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Akhri Pasta: LAN file/folder/text sharing between devices on the same router. Desktop app (Linux, macOS; egui) and Android app (Kotlin) built on **one shared Rust engine**. Priorities are a small binary, low idle cost, and the fewest user actions per send.

## Commands

```bash
cargo run --release -p akhri_pasta                    # desktop app
cargo test --workspace                                # all tests
cargo test -p akhri_core folder_text_and_unsafe_names # the protocol test (loopback folder+text+fd send, path-traversal checks)
cargo clippy --workspace                              # keep at zero warnings

android/setup-tools.sh   # once: Rust 1.100.0 + Android targets and kotlinc into ~/.local/share/akhri-android
android/build.sh         # -> android/akhri_pasta.apk (no Gradle; ~15 s)
adb install -r android/akhri_pasta.apk

# Headless peer: same Engine, auto-accepts. Use it as the "other device" in tests.
cargo run --release -p akhri_core --example peer -- recv /tmp/inbox
cargo run --release -p akhri_core --example peer -- send <ip:port> <path>... "text:hello"
```

Never build Android targets with plain `cargo`. The system Rust (Arch package, no rustup) has no Android std, and `~/.cargo/bin` sits ahead of `/usr/bin` in PATH. `build.sh` sets `RUSTC` and uses the isolated toolchain's `cargo` explicitly.

Only one instance can run per machine, because UDP and TCP both bind the fixed port 47821. Stop the desktop app before starting the headless peer.

## Architecture

```
core/          akhri_core, std only. net.rs = wire protocol, lib.rs = Engine (all app behavior)
desktop/       akhri_pasta binary: egui screens over Engine
android/rust/  cdylib "akhri": JNI bridge only, no logic
android/src/   Kotlin screens; Core.kt declares the JNI functions and parses snapshots
```

**Sync rule:** behavior and every user-visible string live in `core`: discovery, offers, transfers, progress, file naming, `summarize()`, `Job::status()`, `human()`. Platform code only renders and does platform I/O (clipboard, notifications, file pickers, config). If both UIs would need the same change, it belongs in `core`.

**Wire protocol** (`core/src/net.rs`, format documented at the top of the file). If you change it, bump `MAGIC`, so old and new builds refuse each other.
- Discovery: each device broadcasts every 2 s to 255.255.255.255:47821. A receiver replies unicast to every announce, so newcomers see everyone instantly. Peers expire after 7 s.
- Transfer: one TCP connection per send. The sender sends a header, the receiver answers accept/decline, raw bytes follow in entry order, and the receiver sends a final ack.
- `net::receive` is a trust boundary. Every incoming name must pass `safe_rel`. A top-level folder that already exists becomes `Name (1)`. A partial file is deleted on error.
- `pump()` copies in 4 MB `io::copy` chunks, so Linux/Android keep zero-copy `sendfile` while progress still updates. Don't wrap the reader or writer in counting adapters; that disables the specialization.
- Cancel works by shutting down the socket clone stored in `Transfer`, which unblocks both ends.
- Send items: `Item::Path` (desktop; folders are walked, symlinked dirs skipped, empty dirs not sent), `Item::Open(File, name)` (Android: detached fds from content URIs), `Item::Text` (≤16 MB; lands in the receiver's clipboard).

**Engine** (`core/src/lib.rs`): owns peers, pending offers, jobs, and received texts. An incoming offer blocks its receive thread on an mpsc channel until `Engine::decide`. Every state change bumps a generation counter and Condvar. UIs never poll the network; they block on `Engine::wait(seen, timeout)` from one watcher thread:
- Desktop `watch()`: repaints, and raises `notify-send`/`osascript` notifications for new offer ids. The message always goes in argv, never into a script.
- Android `MainActivity.startWatcher`: calls `Core.waitChange` → `Core.snapshot()`, renders, copies texts, and notifies when no activity is resumed.

**JNI contract** (`android/rust/src/lib.rs` ↔ `android/src/app/akhripasta/Core.kt`). Change both sides together:
- Function names are `Java_app_akhripasta_Core_<method>`, with Kotlin `object Core` and the package `app.akhripasta`.
- `snapshot()` returns `Array<String>`, one record per element, fields split by U+001F: `P id name ip`, `O id summary`, `J id label active failed fraction status`, `T text`. The `Snap` class parses these.
- `snapshot()` drains received texts (`take_texts`), so only the watcher thread may call it.
- u64 ids cross JNI as `jlong` bit patterns.
- The cdylib sets SIGPIPE to ignore itself, because a cdylib doesn't get Rust's usual runtime setup.

**Desktop specifics:**
- eframe uses `glow` (not wgpu) for size.
- Linux is forced onto X11/XWayland through `NativeOptions::event_loop_builder`, because winit has no Wayland drag-and-drop. Dropping eframe's `wayland` feature isn't enough: `accesskit_winit` turns winit's wayland feature back on.
- Paste order: clipboard file list → text → image (re-encoded to a temp PNG).
- Config lives in `~/.config/akhri_pasta/config` as `name=` / `dir=` lines.

**Android specifics:**
- No Gradle, no AndroidX, no Compose. Only platform widgets, built in code.
- `build.sh` runs cargo → kotlinc → R8 (from build-tools `d8.jar`) → aapt2 → `jar` → zipalign `-P 16` → apksigner. Native libs are stored uncompressed (`extractNativeLibs=false`).
- R8 keep rules are in `android/rules.pro`.
- minSdk 30 lets Rust write straight into `Download/AkhriPasta` with plain paths. targetSdk 36.
- The Wi-Fi multicast lock is held only while the activity is visible.

## Testing on the emulator

The emulator is behind NAT, so broadcasts don't cross. Bridge it manually:
- `adb emu redir add udp:47823:47821`, then send announce packets (`AKP1` + id u64 LE + `0` + name) to `127.0.0.1:47823`. The app then lists the PC at `10.0.2.2`.
- `adb forward tcp:47822 tcp:47821`, then `peer send 127.0.0.1:47822 …` for PC → phone.
- Files that `adb push` puts in the app's `Android/data` dir fail with EACCES. Put test files in `/sdcard/Download` and pick them through the app's **Files** button, which is the real content-URI path.
- Drive the UI with `uiautomator dump` + `input tap`, and check results with `screencap`.

## Releases

Push a tag `vX.Y.Z` to run `.github/workflows/release.yml`. It builds the APK, a Linux x86_64 tarball, and a macOS universal `.app` zip, and attaches them to a GitHub Release.
- Asset names carry no version, so `releases/latest/download/<name>` links in the README stay valid.
- To bump the version, update `desktop/Cargo.toml`, `core/Cargo.toml`, `android/rust/Cargo.toml`, and `versionCode`/`versionName` in `android/AndroidManifest.xml`.
- Rust is pinned to 1.100.0 in both `rust-toolchain.toml` and `android/setup-tools.sh`. Keep them equal.
