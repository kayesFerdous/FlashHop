---
version: 1
slug: "desktop-src-main-rs"
primary_target: "desktop/src/main.rs"
related_targets: ["android/src/app/flashhop/MainActivity.kt"]
---

# Main screen (desktop window + Android activity)

Scope: the single working screen of FlashHop on desktop (egui) and Android (platform views), plus its settings view. Mode: Operate.

Audience and job: tech-savvy users moving files, folders, screenshots and text between their own devices on one router. Stage → pick device → other side accepts. States: searching, devices found, staged, incoming offer, waiting, running, done, failed.

Constraints: light-only, Inter variable on desktop, system sans on Android, no new runtime deps, wording for offers/status comes from core.

Pinned by user references (2026-11-25), final pick: a light soft-glass daily-focus app with a black pill CTA; a cobalt event ticket with notches, perforation, barcode and a lime circle button; a lavender event app with white cards, lime arrow circles and black circle buttons. (An earlier dark graphite pass was replaced at the user's request.)

## Direction contract

THESIS: A light pocket utility where the payload is a ticket on a cobalt dock. What you send or receive is the one ticket on screen; everything else recedes. Refuses the LAN-share default of radar rings and a blue list on white.

OWN-WORLD: Lavender ground #E9EBF5 with a soft cobalt wash at top-left, white cards (radius 24), a cobalt #2F55E4 dock (radius 28) that holds the drop target or the white outgoing ticket (notches, dashed perforation). Lime #D6F55B round arrow buttons for send, black pill primary buttons, ink #0E0F14 text, muted #62677B. Green/amber/red state pairs on tints. Flat: no shadows.

STORY: The user sees at once whether devices are present, drops or pastes something, sees it as a ticket, taps Send on a device row, and watches a large percentage climb. An incoming offer arrives as its own ticket with Accept.

FIRST VIEWPORT: Top bar with this device's initial (lime on ink), name and white round buttons (received folder, settings). A soft cobalt wash behind a two-tone 28 pt headline that states the current step. Below it the cobalt dock: lime + drop target when empty, the outgoing ticket when staged, glass Files/Folder/Paste pills. Then Devices rows with lime send circles, then Activity with tick-bar progress and big light numerals.

FORM: pinned whole by the user's three final reference images; seed: none (pinned). A direction roll (seed:ab601afea51fbb57) was dealt first; the user left that round and supplied references instead. Quoted pin: "actually I liked their vibe the most".

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
