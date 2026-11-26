mod ui;

use akhri_core::{
    Engine, Job, Offer, Peer, human,
    net::{Item, State},
};
use eframe::egui::{self, Align, Label, Layout};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use ui::{Icon, Tone, W};

const APP: &str = "Akhri Pasta";
/// Content column width; wider windows center it.
const MAX_W: f32 = 560.0;
/// Staged rows shown before "+N more".
const MAX_ROWS: usize = 6;
const PASTE_KEY: &str = if cfg!(target_os = "macos") { "Cmd+V" } else { "Ctrl+V" };

enum Kind {
    File,
    Folder,
    Text,
}

/// An item waiting to be sent, with what the ticket shows about it.
struct Staged {
    /// Taken when sent; the rest stays for the ticket's slide-out.
    item: Option<Item>,
    name: String,
    kind: Kind,
    size: Option<u64>,
}

impl Staged {
    fn new(item: Item) -> Self {
        let (name, kind, size) = match &item {
            Item::Path(p) => {
                let md = fs::metadata(p).ok();
                let dir = md.as_ref().is_some_and(fs::Metadata::is_dir);
                (file_name(p), if dir { Kind::Folder } else { Kind::File }, md.filter(|m| !m.is_dir()).map(|m| m.len()))
            }
            Item::Open(_, n) => (n.clone(), Kind::File, None),
            Item::Text(t) => (t.chars().take(80).collect::<String>().replace(['\n', '\r', '\t'], " "), Kind::Text, Some(t.len() as u64)),
        };
        Self { item: Some(item), name, kind, size }
    }
}

enum Pick {
    Files,
    Folders,
    SaveDir,
}

struct App {
    e: Arc<Engine>,
    staged: Vec<Staged>,
    clip: Option<arboard::Clipboard>,
    error: Option<String>,
    /// Filled by file-picker threads, drained into `staged` each frame.
    picked: Arc<Mutex<Vec<PathBuf>>>,
    /// Filled by the "Change…" folder picker in settings.
    save_dir: Arc<Mutex<Option<PathBuf>>>,
    settings: bool,
    /// Where the outgoing ticket was last drawn.
    ticket_rect: egui::Rect,
    /// The ticket just sent (when, where, its rows), drawn sliding out over the page.
    sent: Option<(f64, egui::Rect, Vec<Staged>)>,
}

fn main() -> eframe::Result {
    let (name, dir) = load_config();
    // `akhri_pasta file1 folder2` stages them, so file-manager "Open with" works.
    let staged = std::env::args_os().skip(1).map(|a| Staged::new(Item::Path(a.into()))).collect();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 720.0])
            .with_min_inner_size([360.0, 520.0])
            .with_app_id("akhri_pasta")
            .with_icon(ui::app_icon(256)),
        // winit has no Wayland drag-and-drop; XWayland bridges drops from Wayland apps.
        #[cfg(target_os = "linux")]
        event_loop_builder: Some(Box::new(|b| {
            winit::platform::x11::EventLoopBuilderExtX11::with_x11(b);
        })),
        ..Default::default()
    };
    eframe::run_native(
        APP,
        opts,
        Box::new(|cc| {
            ui::setup(&cc.egui_ctx);
            let e = Engine::start(name, dir).map_err(|e| format!("Network error: {e} (is it already running?)"))?;
            watch(e.clone(), cc.egui_ctx.clone());
            Ok(Box::new(App {
                e,
                staged,
                clip: arboard::Clipboard::new().ok(),
                error: None,
                picked: Default::default(),
                save_dir: Default::default(),
                settings: false,
                ticket_rect: egui::Rect::NOTHING,
                sent: None,
            }))
        }),
    )
}

/// Repaints on engine changes and raises a system notification for each new offer,
/// even while the window is minimized.
fn watch(e: Arc<Engine>, ctx: egui::Context) {
    thread::spawn(move || {
        let (mut seen, mut last_offer) = (0, 0);
        loop {
            seen = e.wait(seen, Duration::from_secs(60));
            let fresh: Vec<_> = e.offers().into_iter().filter(|o| o.id > last_offer).collect();
            for o in fresh {
                last_offer = o.id;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));
                notify(&format!("Incoming: {}", o.summary));
            }
            ctx.request_repaint();
        }
    });
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.handle_input(&ctx);
        self.staged.extend(self.picked.lock().unwrap().drain(..).map(|p| Staged::new(Item::Path(p))));
        if let Some(d) = self.save_dir.lock().unwrap().take() {
            *self.e.dir.lock().unwrap() = d;
            self.persist();
        }
        for text in self.e.take_texts() {
            if let Some(c) = &mut self.clip {
                let _ = c.set_text(text);
            }
        }
        let (peers, offers, jobs) = (self.e.peers(), self.e.offers(), self.e.jobs());
        let hot = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::CentralPanel::default().frame(egui::Frame::new().fill(ui::BG)).show(root, |ui| {
            // The glow is the network's pulse: brighter while something is offered or moving.
            let target = if !offers.is_empty() {
                0.5
            } else if self.e.busy() {
                0.42
            } else if peers.is_empty() {
                0.24
            } else {
                0.32
            };
            let strength = ctx.animate_value_with_time(egui::Id::unique("glow"), target, 0.6);
            ui::glow(&ctx, ctx.content_rect(), strength);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                column(ui, |ui| {
                    if self.settings {
                        self.settings_ui(ui, frame);
                    } else {
                        self.main_ui(ui, frame, &peers, &offers, &jobs, hot);
                    }
                });
            });
        });
        if let Some((at, rect, rows)) = &self.sent {
            // Ease-in slide to the right with a fade; mirrored in MainActivity.send().
            let t = ((ctx.input(|i| i.time) - at) / 0.32).min(1.0) as f32;
            egui::Area::new(egui::Id::unique("sent-ticket"))
                .order(egui::Order::Foreground)
                .interactable(false)
                .constrain(false)
                .fixed_pos(rect.min + egui::vec2(t * t * rect.width() * 0.6, 0.0))
                .show(&ctx, |ui| {
                    ui.set_width(rect.width());
                    ui.set_opacity(1.0 - t);
                    ticket_ui(ui, rows, egui::Color32::TRANSPARENT); // notch color would be wrong off the dock
                });
            if t < 1.0 {
                ctx.request_repaint();
            } else {
                self.sent = None;
            }
        }
        if self.e.busy() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

/// The outgoing ticket over `rows`. Returns (clear all, remove row) clicks; `under` fills the notches.
fn ticket_ui(ui: &mut egui::Ui, rows: &[Staged], under: egui::Color32) -> (bool, Option<usize>) {
    let (mut clear, mut remove) = (false, None);
    let n = rows.len();
    let total: u64 = rows.iter().filter_map(|s| s.size).sum();
    let partial = rows.iter().any(|s| s.size.is_none());
    ui::ticket(
        ui,
        Some(under),
        |ui| {
            ui.horizontal(|ui| {
                ui.label(ui::text("Outgoing", 19.0, W::Semibold, ui::INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::round(ui, Icon::Close, "Clear all", 32.0, Tone::Card, true).clicked() {
                        clear = true;
                    }
                });
            });
            ui.add_space(2.0);
            for (i, s) in rows.iter().enumerate().take(MAX_ROWS) {
                ui.horizontal(|ui| {
                    let icon = match s.kind {
                        Kind::File => Icon::File,
                        Kind::Folder => Icon::Folder,
                        Kind::Text => Icon::Text,
                    };
                    ui::glyph(ui, icon, 18.0, ui::MUTED, ui::WHITE);
                    ui.add_space(2.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::round(ui, Icon::Close, "Remove", 28.0, Tone::Ground, true).clicked() {
                            remove = Some(i);
                        }
                        let size = match (&s.kind, s.size) {
                            (Kind::Folder, _) => "Folder".into(),
                            (_, Some(b)) => human(b),
                            _ => String::new(),
                        };
                        ui.label(ui::text(size, 12.5, W::Regular, ui::MUTED));
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(Label::new(ui::text(&s.name, 14.0, W::Medium, ui::INK)).truncate());
                        });
                    });
                });
            }
            if n > MAX_ROWS {
                ui.label(ui::text(format!("+{} more", n - MAX_ROWS), 13.0, W::Medium, ui::MUTED));
            }
        },
        |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(ui::text("Items", 11.5, W::Regular, ui::MUTED));
                    ui.label(ui::text(n.to_string(), 19.0, W::Semibold, ui::INK));
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(ui::text(if partial { "Total, plus folders" } else { "Total" }, 11.5, W::Regular, ui::MUTED));
                        let size = if total > 0 { human(total) } else { "—".into() };
                        ui.label(ui::text(size, 19.0, W::Semibold, ui::INK));
                    });
                });
            });
        },
    );
    (clear, remove)
}

/// Centers a column of at most `MAX_W` with side margins.
fn column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let full = ui.available_width();
    let w = (full - 40.0).min(MAX_W);
    ui.add_space(14.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space((full - w) / 2.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_width(w);
            add(ui);
            ui.add_space(28.0);
        });
    });
}

impl App {
    fn handle_input(&mut self, ctx: &egui::Context) {
        let (dropped, pasted, esc) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect();
            let pasted = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
            (dropped, pasted, i.key_pressed(egui::Key::Escape))
        });
        if !dropped.is_empty() {
            self.settings = false;
        }
        self.staged.extend(dropped.into_iter().map(|p| Staged::new(Item::Path(p))));
        if pasted && !ctx.egui_wants_keyboard_input() {
            self.paste();
        }
        if esc {
            self.settings = false;
        }
    }

    /// Clipboard -> staged item. Order: copied files, then text, then image.
    fn paste(&mut self) {
        let Some(c) = &mut self.clip else { return };
        if let Ok(files) = c.get().file_list()
            && !files.is_empty()
        {
            self.staged.extend(files.into_iter().map(|p| Staged::new(Item::Path(p))));
        } else if let Ok(text) = c.get_text()
            && !text.is_empty()
        {
            self.staged.push(Staged::new(Item::Text(text)));
        } else if let Ok(img) = c.get_image() {
            match save_png(img.width as u32, img.height as u32, &img.bytes) {
                Ok(p) => self.staged.push(Staged::new(Item::Path(p))),
                Err(e) => self.error = Some(format!("Could not save pasted image: {e}")),
            }
        }
    }

    /// Opens the system file manager's picker. The dialog future is created here on the UI
    /// thread (macOS requires that) and awaited on a worker so the window keeps repainting.
    fn pick(&self, ctx: &egui::Context, frame: &eframe::Frame, what: Pick) {
        type Fut = std::pin::Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>;
        let d = rfd::AsyncFileDialog::new().set_parent(frame);
        let fut: Fut = match what {
            Pick::Files => Box::pin(d.pick_files()),
            Pick::Folders => Box::pin(d.pick_folders()),
            Pick::SaveDir => {
                let f = d.set_directory(&*self.e.dir.lock().unwrap()).pick_folder();
                Box::pin(async move { f.await.map(|h| vec![h]) })
            }
        };
        let (files, dir, ctx) = (self.picked.clone(), self.save_dir.clone(), ctx.clone());
        let to_dir = matches!(what, Pick::SaveDir);
        thread::spawn(move || {
            let got: Vec<PathBuf> = pollster::block_on(fut).unwrap_or_default().iter().map(|f| f.path().to_path_buf()).collect();
            if to_dir {
                *dir.lock().unwrap() = got.into_iter().next();
            } else {
                files.lock().unwrap().extend(got);
            }
            ctx.request_repaint();
        });
    }

    fn persist(&self) {
        save_config(&self.e.name.lock().unwrap(), &self.e.dir.lock().unwrap());
    }

    fn main_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, peers: &[Peer], offers: &[Offer], jobs: &[Job], hot: bool) {
        self.top_bar(ui);
        ui.add_space(28.0);
        let (strong, rest) = akhri_core::headline(peers.len(), self.staged.len(), false);
        ui::headline(ui, &strong, rest);
        ui.add_space(20.0);
        for o in offers {
            self.offer_ui(ui, o);
            ui.add_space(14.0);
        }
        if let Some(err) = self.error.clone() {
            egui::Frame::new().fill(ui::RED.gamma_multiply(0.1)).corner_radius(16).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::round(ui, Icon::Close, "Dismiss", 30.0, Tone::Ground, true).clicked() {
                            self.error = None;
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| ui.add(Label::new(ui::text(err, 13.0, W::Medium, ui::RED)).wrap()));
                    });
                });
            });
            ui.add_space(12.0);
        }
        // While something is offered or moving and nothing is staged, the dock shrinks to one
        // row and live transfers take its place above Devices.
        let live = jobs.iter().any(Job::active);
        self.dock_ui(ui, frame, hot, self.staged.is_empty() && !hot && (live || !offers.is_empty()));
        ui.add_space(14.0);
        if live {
            self.activity_ui(ui, jobs);
            ui.add_space(14.0);
            self.devices_ui(ui, peers);
        } else {
            self.devices_ui(ui, peers);
            ui.add_space(14.0);
            self.activity_ui(ui, jobs);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let name = self.e.name.lock().unwrap().clone();
        ui.horizontal(|ui| {
            ui::avatar(ui, &name, 42.0, ui::INK, ui::LIME);
            ui.add_space(2.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::round(ui, Icon::Sliders, "Settings", 42.0, Tone::Ground, true).clicked() {
                    self.settings = true;
                }
                if ui::round(ui, Icon::Folder, "Open received files", 42.0, Tone::Ground, true).clicked() {
                    open(&self.e.dir.lock().unwrap());
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add_space(3.0);
                    ui.add(Label::new(ui::text(&name, 15.0, W::Semibold, ui::INK)).truncate());
                    ui.add(Label::new(ui::text("Visible on this network", 12.5, W::Medium, ui::INK.gamma_multiply(0.72))).truncate());
                });
            });
        });
    }

    fn offer_ui(&mut self, ui: &mut egui::Ui, o: &Offer) {
        let dest = tilde(&self.e.dir.lock().unwrap());
        ui::ticket(
            ui,
            None,
            |ui| {
                ui.horizontal(|ui| {
                    ui::badge(ui, Icon::Down, ui::LIME, ui::INK, 42.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add(Label::new(ui::text(&o.summary, 15.0, W::Medium, ui::INK)).wrap());
                    });
                });
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::pill(ui, "Accept", Some(Icon::Check), Tone::Ink, true).clicked() {
                            self.e.decide(o.id, true);
                        }
                        if ui::pill(ui, "Decline", None, Tone::Card, true).clicked() {
                            self.e.decide(o.id, false);
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add(Label::new(ui::text("Saves to", 11.5, W::Regular, ui::MUTED)).truncate());
                            ui::path_label(ui, &dest, 13.0, W::Medium, ui::INK);
                        });
                    });
                });
            },
        );
    }

    /// Files… / Folder… / Paste, as chosen by `add_buttons`.
    fn add(&mut self, ctx: &egui::Context, frame: &eframe::Frame, which: Option<usize>) {
        match which {
            Some(0) => self.pick(ctx, frame, Pick::Files),
            Some(1) => self.pick(ctx, frame, Pick::Folders),
            Some(_) => self.paste(),
            None => {}
        }
    }

    /// The cobalt sending block: a drop target when empty, the outgoing ticket when staged.
    fn dock_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, hot: bool, compact: bool) {
        ui::dock(hot).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if compact {
                ui.horizontal(|ui| {
                    if ui::round(ui, Icon::Plus, "Pick files", 38.0, Tone::Lime, true).clicked() {
                        self.pick(ui.ctx(), frame, Pick::Files);
                    }
                    let which = add_buttons(ui, Tone::Glass, false);
                    self.add(ui.ctx(), frame, which);
                });
                return;
            }
            if self.staged.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    if ui::round(ui, Icon::Plus, "Pick files", 56.0, Tone::Lime, true).clicked() {
                        self.pick(ui.ctx(), frame, Pick::Files);
                    }
                    ui.add_space(12.0);
                    ui.label(ui::text(if hot { "Release to add" } else { "Drop files or folders" }, 17.0, W::Semibold, ui::WHITE));
                    ui.add_space(-4.0);
                    ui.label(ui::text(format!("or paste anything with {PASTE_KEY}"), 13.0, W::Regular, ui::ON_COBALT_MUTED));
                    ui.add_space(12.0);
                });
            } else {
                let r = ui.scope(|ui| ticket_ui(ui, &self.staged, ui::COBALT));
                self.ticket_rect = r.response.rect;
                match r.inner {
                    (true, _) => self.staged.clear(),
                    (_, Some(i)) => drop(self.staged.remove(i)),
                    _ => {}
                }
                ui.add_space(12.0);
            }
            let which = add_buttons(ui, Tone::Glass, true);
            self.add(ui.ctx(), frame, which);
            ui.add_space(2.0);
        });
    }

    fn devices_ui(&mut self, ui: &mut egui::Ui, peers: &[Peer]) {
        ui::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            section(ui, "Devices", (!peers.is_empty()).then(|| peers.len().to_string()));
            if peers.is_empty() {
                ui.add(Label::new(ui::text("Nothing here yet. Open Akhri Pasta on another device on the same network and it appears within a few seconds.", 13.0, W::Regular, ui::MUTED)).wrap());
            }
            let ready = !self.staged.is_empty();
            for (i, p) in peers.iter().enumerate() {
                if i > 0 {
                    ui::hairline(ui);
                }
                ui.horizontal(|ui| {
                    ui::avatar(ui, &p.name, 40.0, ui::RAISED, ui::INK);
                    ui.add_space(2.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let tip = if ready { format!("Send to {}", p.name) } else { "Add files or text first".into() };
                        if ui::round(ui, Icon::UpRight, &tip, 44.0, Tone::Lime, ready).clicked() {
                            self.e.send(p.id, self.staged.iter_mut().filter_map(|s| s.item.take()).collect());
                            self.sent = Some((ui.input(|i| i.time), self.ticket_rect, std::mem::take(&mut self.staged)));
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add_space(3.0);
                            ui.add(Label::new(ui::text(&p.name, 14.5, W::Semibold, ui::INK)).truncate());
                            ui.add(Label::new(ui::text(p.ip.to_string(), 12.5, W::Regular, ui::MUTED)).truncate());
                        });
                    });
                });
            }
        });
    }

    fn activity_ui(&mut self, ui: &mut egui::Ui, jobs: &[Job]) {
        ui::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(ui::text("Activity", 15.5, W::Semibold, ui::INK));
                if jobs.iter().any(|j| !j.active()) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::link(ui, "Clear finished").clicked() {
                            self.e.clear_finished();
                        }
                    });
                }
            });
            ui.add_space(4.0);
            if jobs.is_empty() {
                ui.label(ui::text("Nothing sent or received yet.", 13.0, W::Regular, ui::MUTED));
            }
            for (i, j) in jobs.iter().enumerate() {
                if i > 0 {
                    ui::hairline(ui);
                }
                self.job_ui(ui, j);
            }
        });
    }

    fn job_ui(&self, ui: &mut egui::Ui, j: &Job) {
        let state = j.t.state();
        let arrow = if j.incoming { Icon::Down } else { Icon::Up };
        let (icon, fg, bg) = match state {
            State::Waiting => (arrow, ui::AMBER, egui::Color32::from_rgb(0xFC, 0xF0, 0xD6)),
            State::Running => (arrow, ui::WHITE, ui::COBALT),
            State::Done => (Icon::Check, ui::GREEN, egui::Color32::from_rgb(0xDD, 0xF5, 0xE8)),
            State::Failed(_) => (Icon::Close, ui::RED, egui::Color32::from_rgb(0xFC, 0xE4, 0xE5)),
        };
        let failed = matches!(state, State::Failed(_));
        ui.horizontal(|ui| {
            ui::badge(ui, icon, fg, bg, 40.0);
            ui.add_space(2.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if j.active() && ui::round(ui, Icon::Close, "Cancel", 32.0, Tone::Card, true).clicked() {
                    self.e.cancel(j.id);
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add_space(3.0);
                    ui.add(Label::new(ui::text(&j.label, 14.0, W::Semibold, ui::INK)).truncate());
                    ui.add(Label::new(ui::text(j.status(), 12.5, W::Regular, if failed { ui::RED } else { ui::MUTED })).truncate());
                });
            });
        });
        if state == State::Running {
            ui.horizontal(|ui| {
                ui::numeral(ui, &format!("{}", (j.fraction() * 100.0).floor() as u32), "%");
                ui.add_space(8.0);
                ui::ticks(ui, j.fraction());
            });
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.horizontal(|ui| {
            if ui::round(ui, Icon::Left, "Back (Esc)", 42.0, Tone::Ground, true).clicked() {
                self.settings = false;
            }
            ui.add_space(4.0);
            ui.label(ui::text("Settings", 24.0, W::Display, ui::INK));
        });
        ui.add_space(22.0);

        let mut changed = false;
        ui::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(ui::text("Device name", 13.0, W::Medium, ui::MUTED));
            let mut name = self.e.name.lock().unwrap();
            let edit = egui::TextEdit::singleline(&mut *name)
                .font(ui::font(14.5, W::Medium))
                .margin(egui::Margin::symmetric(12, 10))
                .desired_width(f32::INFINITY);
            changed |= ui.add(edit).changed();
            drop(name);
            ui.label(ui::text("Other devices see this name.", 12.5, W::Regular, ui::MUTED));
        });
        ui.add_space(12.0);
        ui::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(ui::text("Received files", 13.0, W::Medium, ui::MUTED));
            ui::path_label(ui, &tilde(&self.e.dir.lock().unwrap()), 14.5, W::Medium, ui::INK);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui::pill(ui, "Change…", Some(Icon::Folder), Tone::Ink, true).clicked() {
                    self.pick(ui.ctx(), frame, Pick::SaveDir);
                }
                if ui::pill(ui, "Open", None, Tone::Card, true).clicked() {
                    open(&self.e.dir.lock().unwrap());
                }
            });
        });
        ui.add_space(12.0);
        ui::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(ui::text("Network", 13.0, W::Medium, ui::MUTED));
            ui.label(ui::text(format!("Port {}, UDP and TCP", akhri_core::net::PORT), 14.5, W::Medium, ui::INK));
            ui.add(Label::new(ui::text("Allow both through your firewall so devices can find each other and connect.", 12.5, W::Regular, ui::MUTED)).wrap());
        });
        ui.add_space(16.0);
        ui.label(ui::text(format!("{APP} {}", env!("CARGO_PKG_VERSION")), 12.0, W::Regular, ui::MUTED));
        if changed {
            self.persist();
        }
    }
}

/// Files… / Folder… / Paste row; returns the index clicked.
fn add_buttons(ui: &mut egui::Ui, tone: Tone, center: bool) -> Option<usize> {
    let items = [("Files…", Icon::File), ("Folder…", Icon::Folder), ("Paste", Icon::Paste)];
    let width = |icons: bool| items.iter().map(|(s, _)| ui::pill_width(ui, s, icons)).sum::<f32>() + 16.0;
    // Narrow windows drop the icons rather than overflow the dock.
    let icons = width(true) <= ui.available_width();
    let total = width(icons);
    let mut clicked = None;
    ui.horizontal(|ui| {
        if center {
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        }
        for (i, (s, icon)) in items.into_iter().enumerate() {
            let r = ui::pill(ui, s, icons.then_some(icon), tone, true);
            if r.clicked() {
                clicked = Some(i);
            }
            if i == 2 {
                r.on_hover_text(PASTE_KEY);
            }
        }
    });
    clicked
}

fn section(ui: &mut egui::Ui, title: &str, count: Option<String>) {
    ui.horizontal(|ui| {
        ui.label(ui::text(title, 15.5, W::Semibold, ui::INK));
        if let Some(c) = count {
            ui.label(ui::text(c, 15.5, W::Regular, ui::MUTED));
        }
    });
    ui.add_space(4.0);
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// `/home/me/Downloads` -> `~/Downloads`
fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) if !home().as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

fn save_png(w: u32, h: u32, rgba: &[u8]) -> std::io::Result<PathBuf> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let path = std::env::temp_dir().join(format!("clipboard-{secs}.png"));
    write_png(&path, w, h, rgba)?;
    Ok(path)
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(fs::File::create(path)?), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)?;
    wr.finish().map_err(std::io::Error::other)
}

/// System notification without extra crates. Message goes in argv, never into a script.
fn notify(msg: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("osascript");
    #[cfg(target_os = "macos")]
    cmd.args(["-e", "on run argv", "-e", "display notification (item 1 of argv) with title \"Akhri Pasta\"", "-e", "end run", msg]);
    #[cfg(not(target_os = "macos"))]
    let mut cmd = Command::new("notify-send");
    #[cfg(not(target_os = "macos"))]
    cmd.args(["-a", APP, "--", APP, msg]);
    let _ = cmd.status();
}

fn open(p: &Path) {
    let _ = fs::create_dir_all(p);
    let bin = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Ok(mut c) = Command::new(bin).arg(p).spawn() {
        thread::spawn(move || c.wait());
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn config_path() -> PathBuf {
    home().join(".config/akhri_pasta/config")
}

fn load_config() -> (String, PathBuf) {
    let txt = fs::read_to_string(config_path()).unwrap_or_default();
    let get = |k: &str| txt.lines().find_map(|l| l.strip_prefix(k)).map(str::to_string);
    (get("name=").unwrap_or_else(hostname), get("dir=").map_or_else(|| home().join("Downloads"), PathBuf::from))
}

fn save_config(name: &str, dir: &Path) {
    let p = config_path();
    let _ = fs::create_dir_all(p.parent().unwrap());
    let _ = fs::write(p, format!("name={}\ndir={}\n", name.replace('\n', " "), dir.display()));
}

fn hostname() -> String {
    #[cfg(target_os = "macos")]
    let raw = Command::new("scutil").args(["--get", "ComputerName"]).output().map(|o| o.stdout).unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let raw = fs::read("/proc/sys/kernel/hostname").unwrap_or_default();
    let s = String::from_utf8_lossy(&raw).trim().to_string();
    if s.is_empty() { "My device".into() } else { s }
}

#[cfg(test)]
mod tests {
    /// Renders the app icon. With ICON_PNG=<path> it also writes it there at 1024 px; the release
    /// workflow turns that into the macOS .icns, so the bundle icon and window icon match.
    #[test]
    fn icon() {
        let i = super::ui::app_icon(1024);
        let alpha = |x: usize, y: usize| i.rgba[(y * 1024 + x) * 4 + 3];
        assert_eq!((alpha(0, 0), alpha(512, 512)), (0, 255), "transparent corner, opaque tile");
        if let Some(p) = std::env::var_os("ICON_PNG") {
            super::write_png(p.as_ref(), 1024, 1024, &i.rgba).unwrap();
        }
    }

    #[test]
    fn long_path_keeps_its_end() {
        use eframe::egui;
        let ctx = egui::Context::default();
        super::ui::setup(&ctx);
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0))), ..Default::default() };
        let path = "~/some/very/long/folder/that/ends/in/AkhriPasta";
        let mut shown = String::new();
        for _ in 0..2 {
            // fonts load on the first frame
            let out = ctx.run_ui(input(), |ui| {
                super::ui::path_label(ui, path, 14.5, super::W::Medium, super::ui::INK);
            });
            shown = out.shapes.iter().find_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_owned()) } else { None }).unwrap();
            out.drop_without_applying_deltas();
        }
        assert!(shown.starts_with('…') && shown.ends_with("/AkhriPasta"), "{shown}");
    }
}
