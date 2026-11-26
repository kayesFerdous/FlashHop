# TODO (after v0.2.0)

## Check first
- [x] Release workflow for `v0.2.0` finished green, and the GitHub Release has the APK, Linux tarball, and macOS zip
- [x] Install the new APK on a real phone and send a file phone ↔ laptop both ways
- [x] Android: compact dock row (nothing staged, offer live) is centered. Checked on the emulator
- [ ] macOS: the next release's `.app` shows the new icon in Finder and the Dock (the icon step in the workflow has not run on CI yet)

## Decisions
- [x] Dark mode: no, stay light only
- [x] Barcode removed from the outgoing ticket. The stub now shows Items and Total

## Code
- [x] Headline wording lives in `core` (`akhri_core::headline`, JNI `Core.headline`)
- [x] Desktop: long save paths truncate at the start, with the full path on hover (settings and the offer ticket)
- [x] Android notification small icon is the app's own monochrome mark (`res/drawable/ic_stat.xml`), also used as the adaptive icon's `<monochrome>` layer. The themed launcher icon was not checked on a device
- [x] macOS `.app` icon: the release workflow renders it with `ui::app_icon` and builds `AppIcon.icns`

## Nice to have
- [x] One motion moment: the ticket slides out when you hit send (both apps; checked on the emulator and on desktop)
- [ ] Received transfers could show a small ticket stub too
- [ ] Emoji/CJK file names show boxes on desktop (Inter only, no fallback font)
- [ ] Run `/impeccable doctor` (setup warnings) and `npx impeccable update` (v4.11.0 available)
