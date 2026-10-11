//! Demo UI logic. Only depends on `liquid_rust` (no wgpu), all units are points.
//! Slider tracks are NOT glass (cheap): they are drawn as plain rects into the content
//! texture (see `content_rects`), only the thumbs are glass.

use liquid_rust::{Color, ContainerId, Glass, GlassId, Material, Rect, Scene, Shape, Vec2};

pub const BG_NAMES: [&str; 7] = ["AURORA", "OCEAN", "SUNSET", "NEON", "STRIPES", "DOTS", "LIGHT"];
pub const BG_LIGHT: usize = 6;
pub const TEXT_DOT: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub frost: f32,
    pub refraction: f32,
    pub depth: f32,
    pub dispersion: f32,
    pub specular: f32,
    pub tint: f32,
    pub merge: f32,
    pub corner: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self { frost: 6.0, refraction: 0.7, depth: 30.0, dispersion: 0.2, specular: 0.4, tint: 0.45, merge: 30.0, corner: 26.0 }
    }
}

pub struct SliderDef {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
}

pub const SLIDERS: [SliderDef; 8] = [
    SliderDef { name: "FROST", min: 0.0, max: 40.0 },
    SliderDef { name: "REFRACTION", min: 0.0, max: 1.0 },
    SliderDef { name: "DEPTH", min: 0.0, max: 50.0 },
    SliderDef { name: "DISPERSION", min: 0.0, max: 1.0 },
    SliderDef { name: "SPECULAR", min: 0.0, max: 1.0 },
    SliderDef { name: "TINT", min: 0.0, max: 1.0 },
    SliderDef { name: "MERGE", min: 0.0, max: 60.0 },
    SliderDef { name: "CORNER", min: 0.0, max: 26.0 },
];

impl Params {
    pub fn get(&self, i: usize) -> f32 {
        match i {
            0 => self.frost,
            1 => self.refraction,
            2 => self.depth,
            3 => self.dispersion,
            4 => self.specular,
            5 => self.tint,
            6 => self.merge,
            _ => self.corner,
        }
    }
    pub fn set(&mut self, i: usize, v: f32) {
        match i {
            0 => self.frost = v,
            1 => self.refraction = v,
            2 => self.depth = v,
            3 => self.dispersion = v,
            4 => self.specular = v,
            5 => self.tint = v,
            6 => self.merge = v,
            _ => self.corner = v,
        }
    }
}

struct Item {
    id: GlassId,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct Demo {
    item: Item,
    base: Material,
    color: Color,
    tinted: bool,
    pill: bool,
    label: &'static str,
}

struct Row {
    y0: f32,
    track_y: f32,
    track_h: f32,
    thumb: Item,
}

pub struct Text {
    pub x: f32,
    pub y: f32,
    pub s: String,
    pub panel: bool,
}

/// A plain (non-glass) rounded rect drawn into the content texture.
pub struct ContentRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub color: [f32; 4],
}

pub struct Ui {
    pub scene: Scene,
    pub params: Params,
    pub bg: usize,
    w: f32,
    h: f32,
    top: f32,
    panel_y: f32,
    row_h: f32,
    track_x: f32,
    track_w: f32,
    prev: Item,
    next: Item,
    demos: Vec<Demo>,
    demo_container: ContainerId,
    rows: Vec<Row>,
    active_slider: Option<usize>,
    drag: Option<(usize, f32, f32)>,
    down: Option<(GlassId, f32, f32)>,
}

pub fn text_w(s: &str) -> f32 {
    (s.chars().count() as f32) * 6.0 * TEXT_DOT - TEXT_DOT
}

impl Ui {
    pub fn new(w: f32, h: f32, dispersion: f32) -> Self {
        let mut scene = Scene::new();
        let mut params = Params::default();
        params.dispersion = dispersion;
        let top = 56.0;

        // --- background switcher
        let nav = scene.add_container(0.0, 0);
        let mk_nav = |scene: &mut Scene, x: f32| {
            let id = scene.add(
                Glass::new(Rect::new(x, top, 52.0, 52.0))
                    .shape(Shape::Rounded(26.0))
                    .material(Material::regular())
                    .interactive()
                    .container(nav),
            );
            Item { id, x, y: top, w: 52.0, h: 52.0 }
        };
        let prev = mk_nav(&mut scene, 16.0);
        let next = mk_nav(&mut scene, w - 68.0);

        // --- demo glass: round buttons share a container so they merge
        let demo_container = scene.add_container(params.merge, 0);
        let mut demos = Vec::new();
        for (i, label) in ["1", "2", "3"].into_iter().enumerate() {
            let (x, y) = (30.0 + 80.0 * i as f32, top + 92.0);
            let id = scene.add(
                Glass::new(Rect::new(x, y, 56.0, 56.0))
                    .shape(Shape::Rounded(28.0))
                    .material(Material::clear())
                    .interactive()
                    .container(demo_container),
            );
            demos.push(Demo {
                item: Item { id, x, y, w: 56.0, h: 56.0 },
                base: Material::clear(),
                color: Color::WHITE,
                tinted: false,
                pill: false,
                label,
            });
        }

        // --- pills: one per preset
        let pills = [
            ("REGULAR", Material::regular(), Color::WHITE, false, 24.0, top + 172.0),
            ("TINTED", Material::tinted(Color::BLUE), Color::BLUE, true, w - 174.0, top + 172.0),
            ("CLEAR", Material::clear(), Color::WHITE, false, 24.0, top + 242.0),
            ("BAR", Material::clear_bar(), Color::WHITE, false, w - 174.0, top + 242.0),
        ];
        for (label, base, color, tinted, x, y) in pills {
            let id = scene.add(
                Glass::new(Rect::new(x, y, 150.0, 52.0))
                    .shape(Shape::Rounded(26.0))
                    .material(base)
                    .interactive(),
            );
            demos.push(Demo { item: Item { id, x, y, w: 150.0, h: 52.0 }, base, color, tinted, pill: true, label });
        }

        // --- slider panel (tracks are plain rects, only thumbs are glass)
        let panel_y = top + 312.0;
        let first = panel_y + 20.0;
        let bottom = h - 32.0;
        let n = SLIDERS.len() as f32;
        let row_h = ((bottom - first) / n).min(56.0);
        let track_h = (row_h - 20.0).clamp(18.0, 30.0);
        let thumb = track_h + 10.0;
        let (track_x, track_w) = (32.0, w - 64.0);
        let thumbs = scene.add_container(0.0, 1);
        let mut rows = Vec::new();
        for (i, d) in SLIDERS.iter().enumerate() {
            let y0 = first + row_h * i as f32;
            let ty = y0 + 20.0;
            let t = (params.get(i) - d.min) / (d.max - d.min);
            let tx = track_x + t.clamp(0.0, 1.0) * (track_w - thumb);
            let thumb_y = ty + track_h / 2.0 - thumb / 2.0;
            let id = scene.add(
                Glass::new(Rect::new(tx, thumb_y, thumb, thumb))
                    .shape(Shape::Rounded(thumb / 2.0))
                    .material(Material::regular())
                    .interactive()
                    .container(thumbs),
            );
            rows.push(Row { y0, track_y: ty, track_h, thumb: Item { id, x: tx, y: thumb_y, w: thumb, h: thumb } });
        }

        let mut ui = Self {
            scene, params, bg: 0, w, h, top, panel_y, row_h, track_x, track_w,
            prev, next, demos, demo_container, rows,
            active_slider: None, drag: None, down: None,
        };
        ui.apply_params();
        ui
    }

    /// Push the current slider values into every demo glass (Material fields are public).
    pub fn apply_params(&mut self) {
        let p = self.params;
        for d in &self.demos {
            let b = d.base;
            let m = Material {
                frost: p.frost,
                refraction: p.refraction,
                depth: p.depth,
                dispersion: p.dispersion,
                specular: p.specular,
                tint: if d.tinted { Some(d.color.with_alpha(p.tint)) } else { b.tint },
                ..b
            };
            self.scene.set_material(d.item.id, m);
            if d.pill {
                self.scene.set_shape(d.item.id, Shape::Rounded(p.corner.min(d.item.h / 2.0)));
            }
        }
        self.scene.set_container_spacing(self.demo_container, p.merge);
    }

    /// Returns true while the scene is still animating (needs more frames).
    pub fn update(&mut self, dt: f32) -> bool {
        self.scene.update(dt)
    }

    fn slider_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < 12.0 || x > self.w - 12.0 || y < self.panel_y {
            return None;
        }
        self.rows.iter().position(|r| y >= r.y0 && y < r.y0 + self.row_h)
    }

    fn set_slider_from_x(&mut self, i: usize, x: f32) {
        let d = &SLIDERS[i];
        let thumb = self.rows[i].thumb.w;
        let t = ((x - self.track_x - thumb / 2.0) / (self.track_w - thumb)).clamp(0.0, 1.0);
        self.params.set(i, d.min + t * (d.max - d.min));
        self.apply_params();
        let nx = self.track_x + t * (self.track_w - thumb);
        let it = &mut self.rows[i].thumb;
        it.x = nx;
        self.scene.set_frame(it.id, Rect::new(it.x, it.y, it.w, it.h));
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) {
        self.down = None;
        self.drag = None;
        self.active_slider = None;
        let took = self.scene.pointer_down(Vec2::new(x, y));
        if let Some(i) = self.slider_at(x, y) {
            self.active_slider = Some(i);
            self.set_slider_from_x(i, x);
            return;
        }
        if let Some(id) = took {
            self.down = Some((id, x, y));
            if let Some(k) = self.demos.iter().position(|d| d.item.id == id) {
                let it = &self.demos[k].item;
                self.drag = Some((k, x - it.x, y - it.y));
            }
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.scene.pointer_move(Vec2::new(x, y));
        if let Some(i) = self.active_slider {
            self.set_slider_from_x(i, x);
        } else if let Some((k, gx, gy)) = self.drag {
            let it = &mut self.demos[k].item;
            it.x = (x - gx).clamp(0.0, self.w - it.w);
            it.y = (y - gy).clamp(0.0, self.h - it.h);
            self.scene.set_frame(it.id, Rect::new(it.x, it.y, it.w, it.h));
        }
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) {
        self.scene.pointer_up();
        if let Some((id, sx, sy)) = self.down.take() {
            if (x - sx).hypot(y - sy) < 14.0 {
                let n = BG_NAMES.len();
                if id == self.prev.id {
                    self.bg = (self.bg + n - 1) % n;
                } else if id == self.next.id {
                    self.bg = (self.bg + 1) % n;
                }
            }
        }
        self.active_slider = None;
        self.drag = None;
    }

    /// Plain rects drawn into the content texture (under the glass): dark panel + slider tracks.
    pub fn content_rects(&self) -> Vec<ContentRect> {
        let mut v = Vec::new();
        v.push(ContentRect {
            x: 12.0,
            y: self.panel_y,
            w: self.w - 24.0,
            h: self.h - 16.0 - self.panel_y,
            r: 28.0,
            color: [0.0, 0.0, 0.0, 0.45],
        });
        for r in &self.rows {
            v.push(ContentRect {
                x: self.track_x,
                y: r.track_y,
                w: self.track_w,
                h: r.track_h,
                r: r.track_h / 2.0,
                color: [1.0, 1.0, 1.0, 0.22],
            });
        }
        v
    }

    /// All labels, in points. Drawn on top of the glass.
    pub fn texts(&self) -> Vec<Text> {
        let mut v = Vec::new();
        let center = |s: &str, cx: f32| cx - text_w(s) / 2.0;
        let mid = |y: f32, h: f32| y + h / 2.0 - 7.0 * TEXT_DOT / 2.0;
        let title = "LIQUID RUST DEMO";
        v.push(Text { x: center(title, self.w / 2.0), y: 30.0, s: title.into(), panel: false });
        let name = BG_NAMES[self.bg];
        v.push(Text { x: center(name, self.w / 2.0), y: mid(self.top, 52.0), s: name.into(), panel: false });
        for (it, s) in [(&self.prev, "<"), (&self.next, ">")] {
            v.push(Text { x: center(s, it.x + it.w / 2.0), y: mid(it.y, it.h), s: s.into(), panel: false });
        }
        let hint = "DRAG THE GLASS";
        v.push(Text { x: center(hint, self.w / 2.0), y: self.top + 66.0, s: hint.into(), panel: false });
        for d in &self.demos {
            v.push(Text {
                x: center(d.label, d.item.x + d.item.w / 2.0),
                y: mid(d.item.y, d.item.h),
                s: d.label.into(),
                panel: false,
            });
        }
        for (i, r) in self.rows.iter().enumerate() {
            let s = format!("{} {:.2}", SLIDERS[i].name, self.params.get(i));
            v.push(Text { x: self.track_x, y: r.y0 + 2.0, s, panel: true });
        }
        v
    }
}
