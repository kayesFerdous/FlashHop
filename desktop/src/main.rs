use akhri_core::{Engine, net::Item};
use eframe::egui;
use std::{fs, path::PathBuf, process::Command, sync::Arc, thread, time::Duration};

const APP: &str = "Akhri Pasta";

struct App {
    e: Arc<Engine>,
    staged: Vec<Item>,
    clip: Option<arboard::Clipboard>,
    error: Option<String>,
}

fn main() -> eframe::Result {
    let (name, dir) = load_config();
    // `akhri_pasta file1 folder2` stages them, so file-manager "Open with" works.
    let staged = std::env::args_os().skip(1).map(|a| Item::Path(a.into())).collect();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 560.0]).with_app_id("akhri_pasta"),
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
            let e = Engine::start(name, dir).map_err(|e| format!("Network error: {e} (is it already running?)"))?;
            watch(e.clone(), cc.egui_ctx.clone());
            Ok(Box::new(App { e, staged, clip: arboard::Clipboard::new().ok(), error: None }))
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_input(&ctx);
        for text in self.e.take_texts() {
            if let Some(c) = &mut self.clip {
                let _ = c.set_text(text);
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                self.offers_ui(ui);
                self.staged_ui(ui);
                ui.separator();
                self.devices_ui(ui);
                ui.separator();
                self.jobs_ui(ui);
                ui.separator();
                self.settings_ui(ui);
            });
        });
        if self.e.busy() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

impl App {
    fn handle_input(&mut self, ctx: &egui::Context) {
        let (dropped, pasted) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect();
            let pasted = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
            (dropped, pasted)
        });
        self.staged.extend(dropped.into_iter().map(Item::Path));
        if pasted && !ctx.egui_wants_keyboard_input() {
            self.paste();
        }
    }

    /// Clipboard -> staged item. Order: copied files, then text, then image.
    fn paste(&mut self) {
        let Some(c) = &mut self.clip else { return };
        if let Ok(files) = c.get().file_list()
            && !files.is_empty()
        {
            self.staged.extend(files.into_iter().map(Item::Path));
        } else if let Ok(text) = c.get_text()
            && !text.is_empty()
        {
            self.staged.push(Item::Text(text));
        } else if let Ok(img) = c.get_image() {
            match save_png(img.width as u32, img.height as u32, &img.bytes) {
                Ok(p) => self.staged.push(Item::Path(p)),
                Err(e) => self.error = Some(format!("Could not save pasted image: {e}")),
            }
        }
    }

    fn offers_ui(&mut self, ui: &mut egui::Ui) {
        for o in self.e.offers() {
            egui::Frame::group(ui.style()).fill(ui.visuals().selection.bg_fill.gamma_multiply(0.3)).show(ui, |ui| {
                ui.label(egui::RichText::new(&o.summary).strong());
                ui.horizontal(|ui| {
                    if ui.button("Accept").clicked() {
                        self.e.decide(o.id, true);
                    }
                    if ui.button("Decline").clicked() {
                        self.e.decide(o.id, false);
                    }
                });
            });
        }
    }

    fn staged_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("To send");
            if ui.button("Paste").on_hover_text("Ctrl+V / Cmd+V").clicked() {
                self.paste();
            }
            if !self.staged.is_empty() && ui.button("Clear").clicked() {
                self.staged.clear();
            }
        });
        if self.staged.is_empty() {
            ui.weak("Drop files or folders here, or press Ctrl+V.");
        }
        let mut remove = None;
        for (i, item) in self.staged.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("x").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
                ui.label(match item {
                    Item::Path(p) if p.is_dir() => format!("{}/", file_name(p)),
                    Item::Path(p) => file_name(p),
                    Item::Open(_, name) => name.clone(),
                    Item::Text(t) => format!("Text: {}", t.chars().take(50).collect::<String>().replace('\n', " ")),
                });
            });
        }
        if let Some(i) = remove {
            self.staged.remove(i);
        }
    }

    fn devices_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Devices");
        let peers = self.e.peers();
        if peers.is_empty() {
            ui.weak("Looking for devices on this network…");
        }
        for p in peers {
            let btn = egui::Button::new(format!("Send to {}", p.name)).min_size(egui::vec2(ui.available_width(), 32.0));
            if ui.add_enabled(!self.staged.is_empty(), btn).on_hover_text(p.ip.to_string()).clicked() {
                self.e.send(p.id, std::mem::take(&mut self.staged));
            }
        }
    }

    fn jobs_ui(&mut self, ui: &mut egui::Ui) {
        let jobs = self.e.jobs();
        ui.horizontal(|ui| {
            ui.heading("Transfers");
            if ui.button("Open downloads").clicked() {
                open(&self.e.dir.lock().unwrap());
            }
            if jobs.iter().any(|j| !j.active()) && ui.button("Clear finished").clicked() {
                self.e.clear_finished();
            }
        });
        for j in jobs {
            ui.horizontal(|ui| {
                ui.label(&j.label);
                let status = j.status();
                match j.t.state() {
                    akhri_core::net::State::Running => {
                        ui.add(egui::ProgressBar::new(j.fraction()).desired_width(200.0).text(status));
                    }
                    akhri_core::net::State::Failed(_) => {
                        ui.colored_label(ui.visuals().error_fg_color, status);
                    }
                    _ => {
                        ui.label(status);
                    }
                }
                if j.active() && ui.small_button("Cancel").clicked() {
                    self.e.cancel(j.id);
                }
            });
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Settings").show(ui, |ui| {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Device name");
                changed |= ui.text_edit_singleline(&mut *self.e.name.lock().unwrap()).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Save to");
                let mut d = self.e.dir.lock().unwrap().display().to_string();
                if ui.text_edit_singleline(&mut d).changed() {
                    *self.e.dir.lock().unwrap() = d.into();
                    changed = true;
                }
            });
            if changed {
                save_config(&self.e.name.lock().unwrap(), &self.e.dir.lock().unwrap());
            }
            ui.weak(format!("Port {} (UDP + TCP) must be open in your firewall.", akhri_core::net::PORT));
        });
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn save_png(w: u32, h: u32, rgba: &[u8]) -> std::io::Result<PathBuf> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let path = std::env::temp_dir().join(format!("clipboard-{secs}.png"));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(fs::File::create(&path)?), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)?;
    wr.finish().map_err(std::io::Error::other)?;
    Ok(path)
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

fn open(p: &std::path::Path) {
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

fn save_config(name: &str, dir: &std::path::Path) {
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
