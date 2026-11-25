# Akhri Pasta

Instant file, folder, photo, video, and text sharing between devices on the same router.
It needs no internet, no account, and no server.

## Download

| Device | Download | First run |
|---|---|---|
| **Android** 11+ | [akhri_pasta-android.apk](https://github.com/kayesFerdous/akhri_pasta/releases/latest/download/akhri_pasta-android.apk) | Open it on the phone and allow "install unknown apps" when asked. |
| **macOS** 11+ (Intel + Apple Silicon) | [akhri_pasta-macos.zip](https://github.com/kayesFerdous/akhri_pasta/releases/latest/download/akhri_pasta-macos.zip) | Unzip and move to Applications. It isn't signed by Apple, so on first open go to System Settings → Privacy & Security → **Open Anyway**. |
| **Linux** x86_64 | [akhri_pasta-linux-x86_64.tar.gz](https://github.com/kayesFerdous/akhri_pasta/releases/latest/download/akhri_pasta-linux-x86_64.tar.gz) | `tar xzf akhri_pasta-linux-x86_64.tar.gz && ./akhri_pasta` |

All releases: [github.com/kayesFerdous/akhri_pasta/releases](https://github.com/kayesFerdous/akhri_pasta/releases)

| App | Size | UI |
|---|---|---|
| Desktop (Linux, macOS) | ~9 MB single binary | egui |
| Android 11+ | ~1 MB APK (both ABIs), ~500 KB phone-only | Kotlin, plain Android widgets |

## How the two apps stay in sync

Both apps run **the same Rust engine**. The screens are the only code each platform has to itself.

```
core/            shared: wire protocol (net.rs) + app engine (lib.rs): discovery, offers,
                 transfers, progress, file naming, every message text. No dependencies.
desktop/         egui window -> calls core::Engine
android/rust/    ~150-line JNI bridge -> calls core::Engine
android/src/     Kotlin screens -> call the bridge (Core.kt)
```

Any change to the protocol or behavior goes in `core/`, and both apps get it on their next
build. One Cargo workspace and one `Cargo.lock` keep the dependency versions identical too.
The protocol has a version tag (`MAGIC` in `core/src/net.rs`). If you change the wire
format, bump it, and old and new builds will refuse each other instead of corrupting files.

## Use

**Desktop:** drop files or folders on the window, pick them with **Files…** / **Folder…**, or press
Ctrl+V / Cmd+V. Paste works with copied files, screenshots, and text. Then click the lime arrow next to the device.
**Android:** tap **Share → Akhri Pasta** in any app (or use **Files** / **Paste**), then tap the lime arrow next to the device.

The receiver gets a notification and clicks **Accept**. Files go to `~/Downloads` on desktop
and `Download/AkhriPasta` on Android. Received text goes straight to the receiver's clipboard.

## Build

```bash
cargo run --release -p akhri_pasta     # desktop app
cargo test --workspace                 # protocol test: folder + text + fd, path-traversal checks
```

Android needs the Android SDK and NDK (Android Studio installs both) plus a JDK. It does
**not** use Gradle: `build.sh` calls the SDK tools directly and finishes in about 15 seconds.

```bash
android/setup-tools.sh   # once: Rust Android targets + kotlinc in ~/.local/share/akhri-android
android/build.sh         # -> android/akhri_pasta.apk
adb install -r android/akhri_pasta.apk
```

`setup-tools.sh` uses its own `RUSTUP_HOME`/`CARGO_HOME`, so your normal Rust install is untouched.

### Testing without a second device

`core/examples/peer.rs` is a headless peer that runs the same engine and accepts every offer automatically:

```bash
cargo run --release -p akhri_core --example peer -- recv /tmp/inbox
cargo run --release -p akhri_core --example peer -- send 192.168.1.50:47821 ./folder "text:hello"
```

## Network

Port **47821** has to be open for UDP (discovery) and TCP (transfer).
- **Linux firewall:** `sudo ufw allow 47821`
- **macOS:** click **Allow** for local network and incoming connections when asked.

Every 2 seconds, each device broadcasts its name over UDP, and devices that hear it reply directly.
Each transfer is one TCP connection with a header first, then accept/decline, then raw bytes.
On Linux and Android, files are sent with zero-copy `sendfile`.

## Not done yet

- **No encryption.** Fine on a home network, not on public Wi-Fi. The next step is TLS with a pairing code.
- **Android only receives while the app is alive.** A foreground service would keep it ready in the background.
- **Android sends files, not whole folders.**
- Empty folders are not sent. There's no tray icon and no auto-accept for trusted devices.
- Linux runs through X11/XWayland on purpose, because winit has no Wayland drag-and-drop.
