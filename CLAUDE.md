# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

FlashHop: LAN file/folder/text sharing between devices on the same router. Desktop app (Linux, macOS, Windows; egui) and Android app (Kotlin) built on **one shared Rust engine**. Priorities are a small binary, low idle cost, and the fewest user actions per send.

## Commands

```bash
cargo run --release -p flashhop                    # desktop app
cargo test --workspace                                # all tests
cargo test -p flashhop_core folder_text_and_unsafe_names # the protocol test (loopback folder+text+fd send, path-traversal checks)
cargo clippy --workspace                              # keep at zero warnings

android/setup-tools.sh   # once: Rust 1.100.0 + Android targets and kotlinc into ~/.local/share/flashhop-android
android/build.sh         # -> android/flashhop.apk (no Gradle; ~15 s)
adb install -r android/flashhop.apk

# Headless peer: same Engine, auto-accepts. Use it as the "other device" in tests.
cargo run --release -p flashhop_core --example peer -- recv /tmp/inbox
cargo run --release -p flashhop_core --example peer -- send <ip:port> <path>... "text:hello"
```

Never build Android targets with plain `cargo`. The system Rust (Arch package, no rustup) has no Android std, and `~/.cargo/bin` sits ahead of `/usr/bin` in PATH. `build.sh` sets `RUSTC` and uses the isolated toolchain's `cargo` explicitly.

Only one instance can run per machine, because UDP and TCP both bind the fixed port 47821. Stop the desktop app before starting the headless peer.

## Architecture

```
core/          flashhop_core: std + `snow` (Noise, pure Rust) only. net.rs = wire protocol, lib.rs = Engine (all app behavior)
desktop/       flashhop binary: egui screens over Engine; ui.rs = tokens, Inter weights, custom widgets
android/rust/  cdylib "flashhop": JNI bridge only, no logic
android/src/   Kotlin screens; Core.kt declares the JNI functions and parses snapshots; Ui.kt = tokens + custom views
```

**Sync rule:** behavior and every user-visible string live in `core`: discovery, offers, transfers, progress, file naming, `summarize()`, `Job::status()`, `human()`, `headline()`, `Peer::detail()`, recent-device order (`Engine::recent`). Platform code only renders and does platform I/O (clipboard, notifications, file pickers, config). If both UIs would need the same change, it belongs in `core`.

**Wire protocol** (`core/src/net.rs`, format documented at the top of the file). If you change it, bump `MAGIC`, so old and new builds refuse each other.
- Discovery (packet carries the device's public key): each device broadcasts every 2 s to 255.255.255.255:47821. A receiver replies unicast to every announce, so newcomers see everyone instantly. Peers expire after 7 s.
- Transfer: one TCP connection per send. The sender sends MAGIC in the clear, then a Noise NN handshake (`Secure` in net.rs); after that every byte is ChaCha20-Poly1305 frames. Inside: a header, the receiver's accept/decline, raw bytes in entry order, and the receiver's final ack.
- Device verification is trust on first use (the user chose no codes to compare). Each device has a permanent X25519 key (`Identity`, saved in `<state>/identity`, owner-only) that discovery announces and the Noise XX handshake proves. The sender aborts unless the receiver proves the key it announced. `Engine` pins (device name, key) pairs in `<state>/known` after a completed send or an accepted offer; a name can hold several keys (two phones both called "Pixel 8a"). `Trust::Changed` (a known name with an unknown key) shows in `Peer::detail`, refuses sends to it, and adds a warning to the offer summary; accepting that offer trusts the new key. The first contact is unverified; adding a code comparison there is the upgrade path. Keep the crypto crates pure Rust (no `ring`/C) so the Android cross-build stays simple.
- `net::receive` is a trust boundary. Every incoming name must pass `safe_rel`. A top-level folder that already exists becomes `Name (1)`. A partial file is deleted on error.
- `pump()` copies in 4 MB `io::copy` chunks so progress updates. Encryption means no zero-copy `sendfile`; release builds still do ~900 MB/s over loopback, far above any LAN.
- Cancel works by shutting down the socket clone stored in `Transfer`, which unblocks both ends.
- Send items: `Item::Path` (desktop; folders are walked, symlinked dirs skipped, empty dirs not sent), `Item::Open(File, name)` (Android: detached fds from content URIs; a `name` that passes `safe_rel`, like `Folder/sub/a.jpg` from an Android folder send, keeps its folders, anything else has `/` flattened), `Item::Text` (≤16 MB; lands in the receiver's clipboard), `Item::Image(File, name)` (a pasted clipboard image as PNG: saved like a file and also put on the receiver's clipboard; wire kind 2, which older builds just save as a file).
- `receive` returns `Clip`s (texts and saved clipboard-image paths); `Engine::take_clips` hands them to the UI once.

**Engine** (`core/src/lib.rs`): owns peers, pending offers, jobs, received texts, this device's key and the trusted keys. `Engine::start(name, dir, state)`: `state` is a private folder (desktop `~/.config/flashhop`, Android `filesDir`). An incoming offer blocks its receive thread on an mpsc channel until `Engine::decide`. Every state change bumps a generation counter and Condvar. UIs never poll the network; they block on `Engine::wait(seen, timeout)` from one watcher thread:
- Desktop `watch()`: repaints, and raises `notify-send`/`osascript` notifications for new offer ids. The message always goes in argv, never into a script.
- Android `MainActivity.startWatcher`: calls `Core.waitChange` → `Core.snapshot()`, renders, copies texts, and notifies when no activity is resumed.

**JNI contract** (`android/rust/src/lib.rs` ↔ `android/src/app/flashhop/Core.kt`). Change both sides together:
- Function names are `Java_app_flashhop_Core_<method>`, with Kotlin `object Core` and the package `app.flashhop`.
- `snapshot()` returns `Array<String>`, one record per element, fields split by U+001F: `P id name ip detail`, `O id summary`, `J id label state incoming fraction status` (state is `w`/`r`/`d`/`f`), `T text`, `I path` (a received clipboard image, already saved). The `Snap` class parses these.
- `send(peer, fds, names, texts, images)`: the first `images` fds are clipboard images.
- `snapshot()` drains received texts and images (`take_clips`), so only the watcher thread may call it.
- `headline(peers, items)` returns core's headline as strong and rest parts joined by U+001F.
- `start(name, dir, recent, state)` also changes name/dir of a running engine; `recent` (names joined by U+001F) and `state` (`filesDir` path) only seed a new one. `recent()` returns the current list; the app saves it to prefs after sending or accepting.
- u64 ids cross JNI as `jlong` bit patterns.
- The cdylib sets SIGPIPE to ignore itself, because a cdylib doesn't get Rust's usual runtime setup.

**Desktop specifics:**
- eframe uses `glow` (not wgpu) for size.
- Fonts: eframe's `default_fonts` is off; `desktop/assets/Inter.ttf` (variable, OFL) is the only font. Weights come from its `wght`/`opsz` axes as named families (`ui::W`). No emoji/CJK fallback.
- The window icon is rasterized at startup (`ui::app_icon`) from the same geometry as `android/res/drawable/ic_launcher_fg.xml`. On macOS it gets Apple's icon-grid margin, because eframe also puts it in the Dock. The release workflow writes the `.app` icon, the Linux menu icon and the Windows shortcut `.ico` from the same function (`ICON_PNG=<path>` / `ICON_ICO=<path> cargo test -p flashhop icon`).
- Linux is forced onto X11/XWayland through `NativeOptions::event_loop_builder`, because winit has no Wayland drag-and-drop. Dropping eframe's `wayland` feature isn't enough: `accesskit_winit` turns winit's wayland feature back on.
- Paste order: clipboard file list → text → image (re-encoded to an unlinked temp PNG, sent as `Item::Image`). A received clipboard image is decoded (`read_png`) and set with arboard.
- **Files…/Folder…** use `rfd` with only the `xdg-portal` feature (Linux: the desktop's own file manager dialog over D-Bus, no GTK; macOS: NSOpenPanel; Windows: the native IFileDialog). The dialog future is created on the UI thread (macOS needs that) and awaited on a worker thread with `pollster`, so the window keeps repainting.
- Windows: release builds use the `windows` subsystem (no console). Notifications are a PowerShell toast with the message passed in the `FLASHHOP_MSG` env var; `home()` falls back to `USERPROFILE`, the hostname comes from `COMPUTERNAME`. CI builds with `+crt-static` so no VC++ runtime is needed. Check Windows compile from Linux with the isolated toolchain: `rustup target add x86_64-pc-windows-msvc`, then `cargo clippy -p flashhop --target x86_64-pc-windows-msvc` (check only, it doesn't link).
- Config lives in `~/.config/flashhop/config` as `name=` / `dir=` lines, plus one `recent=` line per recent device (newest first). The same folder holds core's `identity` and `known`.

**Android specifics:**
- No Gradle, no AndroidX, no Compose. Only platform widgets, built in code.
- `build.sh` runs cargo → kotlinc → aapt2 (compiles `res/`, which holds the adaptive app icon and `ic_stat`, the monochrome mark used for notifications and the themed-icon layer; there is no `R` class, so Kotlin looks drawables up with `getIdentifier`) → R8 (from build-tools `d8.jar`) → `jar` → zipalign `-P 16` → apksigner. Native libs are stored uncompressed (`extractNativeLibs=false`).
- R8 keep rules are in `android/rules.pro`.
- minSdk 30 lets Rust write straight into `Download/FlashHop` with plain paths. targetSdk 36. Settings can move it to any folder inside `Download/` or `Documents/` (the only places Android 11+ allows any file type by path); the folder picker's tree URI is turned back into a plain path. Name, dir and recents live in SharedPreferences `flashhop`.
- Paste: a clipboard uri with an `image/*` type is sent as a clipboard image (non-PNG is re-encoded to PNG through an unlinked cache file). A received one goes on the clipboard through `ClipProvider` (`content://app.flashhop.clip/<name>`, not exported, grant-only), which serves only the last received image; its path is kept in prefs because the clipboard outlives the process.
- The Wi-Fi multicast lock is held only while the activity is visible.
- **Folder** uses `ACTION_OPEN_DOCUMENT_TREE`; `stageFolder` walks it off the UI thread with `DocumentsContract` child queries (no AndroidX `DocumentFile`) and stages one item holding every file as (uri, `Folder/sub/name`). At send time each file becomes a detached fd. Android won't let apps pick the root of Download or the storage root, only folders inside them.

**Design:** light only. Palette, radii and components are mirrored between `desktop/src/ui.rs` and `android/src/app/flashhop/Ui.kt` (same names: `Tone`, ticket, ticks, icons on a 24-unit grid); change both together. `DESIGN.md` records the system, `PRODUCT.md` the product facts.

## Testing on the emulator

The emulator is behind NAT, so broadcasts don't cross. Bridge it manually:
- `adb emu redir add udp:47823:47821`, then send announce packets (`AKP2` + id u64 LE + `0` + the 32-byte key `peer recv` prints on its first line + name; a wrong key makes sends fail verification, which is also how to test that; re-run `redir add` if the app never shows the PC, it is lost when added before the emulator's network is up) to `127.0.0.1:47823`. The app then lists the PC at `10.0.2.2`.
- `adb forward tcp:47822 tcp:47821`, then `peer send 127.0.0.1:47822 …` for PC → phone.
- Files that `adb push` puts in the app's `Android/data` dir fail with EACCES. Put test files in `/sdcard/Download` and pick them through the app's **Files** button, which is the real content-URI path.
- Drive the UI with `uiautomator dump` + `input tap`, and check results with `screencap`.

## Releases

Push a tag `vX.Y.Z` to run `.github/workflows/release.yml`. It builds the APK, a Linux x86_64 tarball, a Windows x64 zip (`flashhop.exe` + `flashhop.ico`), and a macOS universal `.app` zip, and attaches them to a GitHub Release.
- Asset names carry no version, so `releases/latest/download/<name>` links in the README stay valid.
- `install.sh` (repo root, POSIX sh) installs the latest Linux/macOS asset; it depends on those names and on the Linux tarball holding `flashhop` + `flashhop.png`. Test it with `FLASHHOP_URL=file:///dir/with/assets HOME=/tmp/x sh install.sh`. `install.ps1` is the Windows twin (installs to `%LOCALAPPDATA%\Programs\FlashHop` plus a Start menu shortcut) and honors `FLASHHOP_URL` too.
- To bump the version, update `desktop/Cargo.toml`, `core/Cargo.toml`, `android/rust/Cargo.toml`, and `versionCode`/`versionName` in `android/AndroidManifest.xml`.
- Rust is pinned to 1.100.0 in both `rust-toolchain.toml` and `android/setup-tools.sh`. Keep them equal.
