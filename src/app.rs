//! Editor window: view, simulate, recolor, transform, merge and export designs.

use crate::formats;
use crate::pattern::{Cmd, Pattern};
use crate::threads::Chart;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::path::PathBuf;

/// Common hoops, in mm.
const HOOPS: &[(&str, f32, f32)] = &[
    ("None", 0.0, 0.0),
    ("4x4 (100x100)", 100.0, 100.0),
    ("5x7 (130x180)", 130.0, 180.0),
    ("6x10 (160x260)", 160.0, 260.0),
    ("8x8 (200x200)", 200.0, 200.0),
    ("8x12 (200x300)", 200.0, 300.0),
];

pub fn run(files: Vec<PathBuf>) -> anyhow::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]).with_title("Stitchery"),
        ..Default::default()
    };
    eframe::run_native("Stitchery", opts, Box::new(move |_| Ok(Box::new(App::new(files)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

struct App {
    design: Pattern,
    path: Option<PathBuf>,
    undo: Vec<Pattern>,
    redo: Vec<Pattern>,
    hidden: Vec<bool>,
    charts: Vec<Chart>,
    chart: usize,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    show_jumps: bool,
    realistic: bool,
    hoop: usize,
    sim: usize,
    playing: bool,
    resize_pct: f64,
    rotate_deg: f64,
    status: String,
}

impl App {
    fn new(files: Vec<PathBuf>) -> Self {
        let mut a = App {
            design: Pattern::default(),
            path: None,
            undo: vec![],
            redo: vec![],
            hidden: vec![],
            charts: vec![Chart::pec()],
            chart: 0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
            show_jumps: false,
            realistic: true,
            hoop: 2,
            sim: usize::MAX,
            playing: false,
            resize_pct: 100.0,
            rotate_deg: 0.0,
            status: "Open a design with File ▸ Open".into(),
        };
        for f in files {
            a.load(f, true);
        }
        a
    }

    fn edit(&mut self, f: impl FnOnce(&mut Pattern)) {
        self.undo.push(self.design.clone());
        self.redo.clear();
        f(&mut self.design);
        self.design.ensure_threads();
        self.hidden.resize(self.design.color_count(), false);
    }

    fn load(&mut self, path: PathBuf, merge: bool) {
        match formats::read(&path) {
            Ok(p) => {
                let n = p.stitch_count();
                if merge && !self.design.stitches.is_empty() {
                    self.edit(|d| d.merge(&p));
                    self.status = format!("Merged {} ({n} stitches)", path.display());
                } else {
                    self.edit(|d| *d = p);
                    self.path = Some(path.clone());
                    self.status = format!("Opened {} ({n} stitches)", path.display());
                }
                self.hidden = vec![false; self.design.color_count()];
                self.fit = true;
                self.sim = usize::MAX;
            }
            Err(e) => self.status = format!("Error: {e:#}"),
        }
    }

    fn file_dialog(&self) -> rfd::FileDialog {
        let mut d = rfd::FileDialog::new();
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        d
    }

    fn open(&mut self, merge: bool) {
        let files = self.file_dialog().add_filter("Embroidery", formats::READ_EXTS).pick_files();
        for f in files.unwrap_or_default() {
            self.load(f, merge);
        }
    }

    fn save_as(&mut self) {
        let mut d = self.file_dialog();
        for e in formats::WRITE_EXTS {
            d = d.add_filter(e.to_uppercase(), &[*e]);
        }
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        let Some(mut out) = d.set_file_name(stem.unwrap_or_else(|| "design".into())).save_file() else { return };
        if out.extension().is_none() {
            out.set_extension("pes");
        }
        self.status = match formats::write(&out, &self.design) {
            Ok(()) => format!("Saved {}", out.display()),
            Err(e) => format!("Error: {e:#}"),
        };
    }

    fn load_chart(&mut self) {
        let Some(f) = rfd::FileDialog::new().add_filter("Thread chart (CSV)", &["csv"]).pick_file() else { return };
        match Chart::load_csv(&f) {
            Ok(c) => {
                self.status = format!("Loaded chart {} ({} threads)", c.name, c.threads.len());
                self.charts.push(c);
                self.chart = self.charts.len() - 1;
            }
            Err(e) => self.status = format!("Error: {e:#}"),
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…").clicked() {
                    ui.close_menu();
                    self.open(false);
                }
                if ui.button("Merge…").clicked() {
                    ui.close_menu();
                    self.open(true);
                }
                if ui.button("Save As…").clicked() {
                    ui.close_menu();
                    self.save_as();
                }
                ui.separator();
                if ui.button("Load thread chart (CSV)…").clicked() {
                    ui.close_menu();
                    self.load_chart();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo")).clicked() {
                    self.undo_step();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).clicked() {
                    self.redo_step();
                }
            });
            ui.menu_button("Design", |ui| {
                if ui.button("Center").clicked() {
                    self.edit(Pattern::center);
                }
                if ui.button("Rotate 90° right").clicked() {
                    self.edit(|d| d.rotate(90.0));
                }
                if ui.button("Rotate 90° left").clicked() {
                    self.edit(|d| d.rotate(-90.0));
                }
                if ui.button("Mirror horizontally").clicked() {
                    self.edit(Pattern::mirror_horizontal);
                }
                if ui.button("Mirror vertically").clicked() {
                    self.edit(Pattern::mirror_vertical);
                }
                if ui.button("Remove hidden colors").clicked() {
                    let hidden = self.hidden.clone();
                    self.edit(|d| remove_colors(d, &hidden));
                    self.hidden.fill(false);
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_jumps, "Show jumps");
                ui.checkbox(&mut self.realistic, "Thick thread");
                if ui.button("Zoom to fit").clicked() {
                    self.fit = true;
                }
            });
        });
    }

    fn undo_step(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.design, p));
            self.hidden.resize(self.design.color_count(), false);
        }
    }

    fn redo_step(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.design, p));
            self.hidden.resize(self.design.color_count(), false);
        }
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let d = &self.design;
        let b = d.bounds();
        ui.heading(if d.label.is_empty() { "No design" } else { &d.label });
        egui::Grid::new("info").show(ui, |ui| {
            ui.label("Size");
            ui.label(format!("{:.1} × {:.1} mm", b.width() / 10.0, b.height() / 10.0));
            ui.end_row();
            ui.label("Stitches");
            ui.label(d.stitch_count().to_string());
            ui.end_row();
            ui.label("Jumps / trims");
            ui.label(format!("{} / {}", d.jump_count(), d.trim_count()));
            ui.end_row();
            ui.label("Colors");
            ui.label(d.color_count().to_string());
            ui.end_row();
        });

        ui.separator();
        ui.label("Transform");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.resize_pct).range(10.0..=500.0).suffix(" %"));
            if ui.button("Resize").on_hover_text("Scales stitch positions; density changes with size").clicked() {
                let s = self.resize_pct / 100.0;
                self.edit(|d| d.scale(s, s));
                self.resize_pct = 100.0;
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.rotate_deg).range(-360.0..=360.0).suffix("°"));
            if ui.button("Rotate").clicked() {
                let r = self.rotate_deg;
                self.edit(|d| d.rotate(r));
                self.rotate_deg = 0.0;
            }
        });
        egui::ComboBox::from_label("Hoop").selected_text(HOOPS[self.hoop].0).show_ui(ui, |ui| {
            for (i, h) in HOOPS.iter().enumerate() {
                ui.selectable_value(&mut self.hoop, i, h.0);
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("chart").selected_text(&self.charts[self.chart].name).show_ui(ui, |ui| {
                for (i, c) in self.charts.iter().enumerate() {
                    ui.selectable_value(&mut self.chart, i, &c.name);
                }
            });
            if ui.button("Match all").on_hover_text("Replace each color with the nearest chart thread").clicked() {
                let chart = self.charts[self.chart].clone();
                self.edit(|d| {
                    for t in &mut d.threads {
                        if let Some(n) = chart.nearest(t.rgb) {
                            *t = n.clone();
                        }
                    }
                });
            }
        });

        let n = self.design.color_count();
        self.hidden.resize(n, false);
        let mut action: Option<Box<dyn FnOnce(&mut Pattern)>> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for i in 0..n {
                let t = &self.design.threads[i];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.hidden[i], "").on_hover_text("Hide");
                    let mut rgb = t.rgb;
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        action = Some(Box::new(move |d: &mut Pattern| {
                            d.threads[i].rgb = rgb;
                            d.threads[i].code.clear();
                        }));
                    }
                    ui.label(format!("{}. {}", i + 1, t.name));
                    if !t.code.is_empty() {
                        ui.weak(&t.code);
                    }
                });
            }
        });
        if let Some(f) = action {
            self.edit(f);
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(236, 232, 224));
        let b = self.design.bounds();
        let hoop = HOOPS[self.hoop];
        if self.fit {
            let w = (b.width() as f32).max(hoop.1 * 10.0).max(10.0);
            let h = (b.height() as f32).max(hoop.2 * 10.0).max(10.0);
            self.zoom = (rect.width() / w).min(rect.height() / h) * 0.9;
            self.pan = Vec2::ZERO;
            self.fit = false;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let f = (scroll / 300.0).exp();
                if let Some(m) = resp.hover_pos() {
                    let c = rect.center() + self.pan;
                    self.pan += (m - c) * (1.0 - f);
                }
                self.zoom *= f;
            }
        }
        self.pan += resp.drag_delta();

        let (cx, cy) = (((b.min_x + b.max_x) / 2.0) as f32, ((b.min_y + b.max_y) / 2.0) as f32);
        let center = rect.center() + self.pan;
        let z = self.zoom;
        let to = |x: f64, y: f64| Pos2::new(center.x + (x as f32 - cx) * z, center.y + (y as f32 - cy) * z);

        if hoop.1 > 0.0 {
            let half = Vec2::new(hoop.1 * 5.0, hoop.2 * 5.0) * z;
            let r = Rect::from_center_size(to(cx as f64, cy as f64), half * 2.0);
            painter.rect_stroke(r.expand(6.0), 18.0, Stroke::new(6.0_f32, Color32::from_gray(150)));
            painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(0.5_f32, Color32::from_gray(190)));
            painter.line_segment([r.left_center(), r.right_center()], Stroke::new(0.5_f32, Color32::from_gray(190)));
        }

        let width = if self.realistic { (4.0 * z).clamp(1.0, 6.0) } else { 1.0 };
        let ci = self.design.color_indices();
        let limit = self.sim.min(self.design.stitches.len());
        let mut prev: Option<Pos2> = None;
        for (i, s) in self.design.stitches[..limit].iter().enumerate() {
            let p = to(s.x, s.y);
            let c = ci[i];
            if let Some(q) = prev {
                if !self.hidden.get(c).copied().unwrap_or(false) {
                    match s.cmd {
                        Cmd::Stitch => {
                            let rgb = self.design.threads[c].rgb;
                            let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            painter.line_segment([q, p], Stroke::new(width, col));
                        }
                        Cmd::Jump if self.show_jumps => {
                            painter.line_segment([q, p], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(80, 80, 80, 120)));
                        }
                        _ => {}
                    }
                }
            }
            prev = Some(p);
        }
        if limit < self.design.stitches.len() {
            if let Some(p) = prev {
                painter.circle_stroke(p, 6.0, Stroke::new(2.0_f32, Color32::RED));
            }
        }
    }
}

/// Deletes the stitches of the given color blocks, joining neighbours with a jump.
fn remove_colors(d: &mut Pattern, hidden: &[bool]) {
    let ci = d.color_indices();
    let mut out = Pattern { label: d.label.clone(), ..Default::default() };
    let mut last_block = None;
    for (i, s) in d.stitches.iter().enumerate() {
        let c = ci[i];
        if hidden.get(c).copied().unwrap_or(false) || s.cmd == Cmd::ColorChange {
            continue;
        }
        if last_block != Some(c) {
            if last_block.is_some() {
                out.trim();
                out.color_change();
            }
            out.threads.push(d.threads[c].clone());
            out.push(s.x, s.y, Cmd::Jump);
            last_block = Some(c);
        }
        out.stitches.push(*s);
    }
    *d = out;
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for f in dropped {
            self.load(f, true);
        }
        let (undo, redo) = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
             i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))))
        });
        if undo {
            self.undo_step();
        }
        if redo {
            self.redo_step();
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let total = self.design.stitches.len();
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "⏸" } else { "▶" }).on_hover_text("Stitch-out simulation").clicked() {
                    self.playing = !self.playing;
                    if self.playing && self.sim >= total {
                        self.sim = 0;
                    }
                }
                let mut v = self.sim.min(total);
                ui.spacing_mut().slider_width = (ui.available_width() - 400.0).max(100.0);
                if ui.add(egui::Slider::new(&mut v, 0..=total).text("stitches")).changed() {
                    self.sim = v;
                    self.playing = false;
                }
                ui.label(&self.status);
            });
            if self.playing {
                self.sim = (self.sim + (total / 600).max(2)).min(total);
                if self.sim >= total {
                    self.playing = false;
                }
                ctx.request_repaint();
            }
        });
        egui::SidePanel::left("side").default_width(280.0).show(ctx, |ui| self.side_panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.canvas(ui));
    }
}
