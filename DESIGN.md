---
name: Akhri Pasta
description: LAN sharing where the payload is a ticket on a cobalt dock.
colors:
  ground-lavender: "#E9EBF5"
  card-white: "#FFFFFF"
  raised-mist: "#EEF0F7"
  raised-mist-hover: "#E2E5F0"
  hairline: "#E6E8F0"
  ink: "#0E0F14"
  muted-slate: "#585D70"
  dock-cobalt: "#2F55E4"
  dock-cobalt-hot: "#4166F5"
  on-cobalt-muted: "#DCE3FD"
  go-lime: "#D6F55B"
  go-lime-hover: "#C8EA45"
  perforation-blue: "#B9C7F6"
  state-green: "#158A55"
  state-green-bg: "#DDF5E8"
  state-amber: "#A86A0A"
  state-amber-bg: "#FCF0D6"
  state-red: "#D6333A"
  state-red-bg: "#FCE4E5"
typography:
  headline:
    fontFamily: "Inter (desktop, opsz 32) / system sans (Android)"
    fontSize: "28px desktop / 30sp Android"
    fontWeight: 640
    lineHeight: 1.1
    letterSpacing: "-0.02em"
  numeral:
    fontFamily: "Inter (opsz 32) / system sans"
    fontSize: "32px / 34sp"
    fontWeight: 330
  title:
    fontFamily: "Inter / system sans"
    fontSize: "15.5px / 17sp"
    fontWeight: 600
  body:
    fontFamily: "Inter / system sans"
    fontSize: "14px / 15sp"
    fontWeight: 500
  label:
    fontFamily: "Inter / system sans"
    fontSize: "12.5px / 13sp"
    fontWeight: 400
rounded:
  pill: "19px (half the 38px height) / 22dp"
  ticket: "22px"
  card: "24px"
  dock: "28px"
spacing:
  page-gutter: "20px"
  section-gap: "14px"
  card-padding: "16px 14px desktop / 18dp 16dp Android"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.card-white}"
    rounded: "{rounded.pill}"
    height: "38px / 48dp"
  button-secondary:
    backgroundColor: "{colors.raised-mist}"
    textColor: "{colors.ink}"
    rounded: "{rounded.pill}"
  button-glass:
    backgroundColor: "rgba(255,255,255,0.125)"
    textColor: "{colors.card-white}"
    rounded: "{rounded.pill}"
  send-round:
    backgroundColor: "{colors.go-lime}"
    textColor: "{colors.ink}"
    size: "44px / 52dp"
  icon-round-ground:
    backgroundColor: "{colors.card-white}"
    textColor: "{colors.ink}"
    size: "42px / 48dp"
  card:
    backgroundColor: "{colors.card-white}"
    rounded: "{rounded.card}"
  dock:
    backgroundColor: "{colors.dock-cobalt}"
    rounded: "{rounded.dock}"
    padding: "14px"
  ticket:
    backgroundColor: "{colors.card-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.ticket}"
    padding: "18px 16px"
---

# Design System: Akhri Pasta

## Overview

**Creative North Star: "The Boarding Pass"**

What you send or receive is a ticket: a white card with side notches and a dashed perforation, held on a cobalt dock. Everything else is quiet: a lavender-grey ground with a soft cobalt wash at the top, white cards for devices and activity, and one lime "go" control per device. The look comes from the user's reference images (a cobalt event ticket, a lavender event app with lime arrow buttons, a soft-glass focus app with a black pill).

It is an Operate surface: one screen, scanned in seconds, used many times a day by people who know what an IP is. Density stays moderate; the headline always names the current step.

**Key Characteristics:**
- Light only. Lavender ground, white cards, cobalt dock, lime go, black primary pill.
- The ticket is the signature: outgoing items live on one, incoming offers arrive as one.
- Two-tone headline: the state in ink, the next step in muted slate.
- Large light numerals plus a tick bar for live progress.
- Round icon buttons and pills; no square buttons anywhere.

## Colors

- **Ground Lavender** `#E9EBF5`: the window/activity background. A radial cobalt wash from the top-left sits over it; its strength follows activity (searching 0.24, idle 0.32, transfer 0.42, offer 0.5 on desktop; matching view alpha on Android).
- **Card White** `#FFFFFF`: device and activity cards, tickets.
- **Raised Mist** `#EEF0F7` (hover `#E2E5F0`): secondary pills and round buttons on white, text inputs, avatars of other devices.
- **Ink** `#0E0F14`: text, primary pill, this device's avatar, the incoming-offer badge.
- **Muted Slate** `#585D70`: secondary text (≥4.5:1 on white, on the ground and on the glow).
- **Dock Cobalt** `#2F55E4` (drag-hover `#4166F5`): the sending block, running-transfer badge, progress ticks, focus ring, selection.
- **Go Lime** `#D6F55B`: send buttons, the "+" on the dock, this device's initial. Ink sits on it, never white.
- **State pairs** (text on tint): green `#158A55`/`#DDF5E8` done, amber `#A86A0A`/`#FCF0D6` waiting, red `#D6333A`/`#FCE4E5` failed.

**The one-go rule.** Lime is reserved for "go" (send, add). Cobalt is the sending surface and progress. Neither appears as decoration.

## Typography

Desktop embeds Inter variable (`desktop/assets/Inter.ttf`, OFL); weights are named families built from its `wght`/`opsz` axes (`ui::W`: Regular 400, Medium 500, Semibold 600, Display 640 at opsz 32, Light 330 at opsz 32). Android uses the system sans with `Typeface.create(DEFAULT, weight, false)` and sp sizes so it follows the user's font scale.

- **Headline** 28px/30sp, 640, tracking -0.02em, two-tone.
- **Numeral** 32px/34sp, light, followed by a small muted unit ("%").
- **Title** 15.5–20, 600: section titles ("Devices", "Activity"), ticket title ("Outgoing").
- **Body** 14–16, 500: names, file names, offer summary.
- **Label** 11.5–13, 400: IPs, sizes, statuses, helper text.

## Layout

One column. Desktop: max 560px wide, centered, 20px side gutters, default window 440×720 (min 360×520). Android: 20dp gutters, edge to edge; the glow runs under the status bar, content is clipped below it. Order: top bar (this device + received-files + settings round buttons), headline, incoming offer tickets, dock, Devices card, Activity card. Settings is a separate view on desktop (Esc goes back) and a rename dialog on Android.

## Elevation & Depth

Flat. No shadows and no borders on cards; depth comes from tone (lavender → white → mist) and the cobalt dock. The ticket reads as separate because of its notches and perforation, not a shadow.

## Shapes

Radii: pills are fully round; ticket 22; card 24; dock 28. Ticket notches are 11px circles at the perforation line, cut as true holes on Android and painted in the colour underneath on desktop. Icons are drawn on a 24-unit grid with one 1.9-unit stroke (`ui::paint` / `IconDrawable`): plus, close, check, up, down, up-right, left, folder, file, text, paste, sliders.

## Components

- **Pill**: 38px (desktop) / 48dp (Android) high, 13.5–14.5 semibold, optional leading icon. Tones: Ink (primary), Card (secondary on white), Ground (white on lavender), Glass (on cobalt), Lime (go). Hover lerps over 120ms; disabled drops to 35% fill.
- **Round button**: same tones as pills. Disabled send shows a mist circle with a faded arrow. On Android the touch target is never under 48dp, even when the circle is drawn smaller.
- **Dock**: cobalt block. Empty: lime "+" button (opens the file picker), "Drop files or folders" / "Share or pick files", helper line, glass pills (Files…, Folder…, Paste on desktop; Files, Paste on Android). Staged: the outgoing ticket, then the glass pills. Compact (nothing staged while an offer or transfer is live): one row of lime + and glass pills, and Activity moves above Devices so the live percentage stays in view. On narrow desktop windows the pills drop their icons instead of overflowing.
- **Ticket**: top = title or offer summary with a badge; perforation; stub = item count and total (outgoing) or "Saves to" plus Decline/Accept (incoming). On send, the outgoing ticket is the one motion moment: it slides right by 60% of its width with ease-in and fades out over 320 ms, above the page, while the dock re-renders underneath.
- **Device row**: avatar initial, name and IP, lime send round button.
- **Job row**: state badge (amber waiting, cobalt running, green done, red failed), label, core status string ("Sent, 2.0 GB", "Couldn't connect. Is Akhri Pasta open on the other device?"), cancel while active; running rows add the big percentage and a tick bar.

## Do's and Don'ts

- Do keep wording for offers and status in `core` (`summarize()`, `Job::status()`); the UIs only lay it out.
- Do change `desktop/src/ui.rs` and `android/src/app/akhripasta/Ui.kt` together.
- Do keep lime for go and cobalt for sending; one of each per control.
- Don't add shadows, borders on cards, gradients beyond the top wash, or emoji/glyph icons.
- Don't put a small label above a heading; headings carry themselves.
