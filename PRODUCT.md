# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

Desktop (Linux, macOS, Windows) in egui; Android in Kotlin with platform widgets only (no AndroidX, no Compose). Both are thin views over one Rust engine.

## Users

Tech-savvy people moving files, folders, screenshots and text between their own devices (laptop ↔ phone, laptop ↔ laptop) on the same home or office router. They know what an IP and a port are, and want the send to take as few actions as possible.

## Product Purpose

Send anything to a nearby device without a cloud, an account, or a cable. Success is: drop or share, tap the device, the other side accepts, done. Received text and pasted images land straight in the clipboard.

## Positioning

Direct device-to-device over the LAN: no server, no sign-in, no upload limit, zero-copy `sendfile` speed. One shared engine, so desktop and Android behave and word things identically. Small binary, near-zero idle cost.

## Operating Context

- Desktop: a small utility window kept open beside a file manager; files arrive by drag-and-drop, Files…/Folder… pickers, Ctrl+V, or "Open with". Incoming offers raise a system notification.
- Android: mostly entered through Share → FlashHop from another app; also Files / Paste buttons. Incoming offers notify when the app is not in front.
- Devices appear automatically within ~2 s via UDP broadcast on port 47821. Devices sent to or accepted from lately sort first; from five devices on, a search field filters the list.

## Capabilities and Constraints

- Items: files, folders (desktop), text (≤16 MB, lands in the receiver's clipboard), pasted images (re-encoded to PNG; saved like a file and also put on the receiver's clipboard).
- States: searching for devices, devices found, staged items, incoming offer (accept/decline), transfer waiting/running (bytes, speed)/done/failed, cancel.
- All user-visible wording for offers and job status comes from `core` (`summarize()`, `Job::status()`, `human()`); UIs must not diverge.
- Binary size and idle cost are priorities. Desktop embeds one variable UI font (Inter) in place of egui's bundled fonts; no other new dependencies.
- Android: platform widgets built in code, minSdk 30, targetSdk 36.
- Both apps are light only, by the user's choice of references (2026-11-25). A dark scheme is an open decision.
- Only one instance per machine (fixed port 47821).

## Brand Commitments

- Name: FlashHop.
- An app icon is wanted (Android adaptive icon + desktop window icon), drawn as vectors, no raster files.

## Evidence on Hand

No logo, screenshots, testimonials, or metrics exist. Do not invent user counts, speeds, or claims.

## Product Principles

1. Fewest actions per send: the send target is always one click away once something is staged.
2. Same behavior and words on every platform; the engine owns the wording.
3. Show the truth of the network: who is here, what is moving, how fast, what failed and why.
4. Quiet when idle: no polling, no animation without state change.
