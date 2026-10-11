//! Music player UI (UI only, no audio) built on liquid-rust glass. All units are points.
//!
//! Drawing model (kept cheap for the GPU):
//!  * content texture (under the glass): the animated/baked background + a static scrim panel
//!  * glass: buttons, seek/volume thumbs, "up next" cards (liquid-rust, or flat cards in AURORA)
//!  * overlay (over the glass, crisp): album art, progress bars, icons, text
//! so the background texture only changes when the track (and its background) changes.

use liquid_rust::{Color, Glass, GlassId, Material, Rect, Scene, Shadow, Shape, Spring, Vec2};

use crate::tier::{Tier, TIERS};

pub struct Track {
    pub title: &'static str,
    pub artist: &'static str,
    pub dur: f32,
    /// Background shader kind (see shader.wgsl).
    pub kind: usize,
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

pub const TRACKS: [Track; 5] = [
    Track { title: "MIDNIGHT DRIVE", artist: "NEON HORIZON", dur: 213.0, kind: 0, a: 0xFF4D8D, b: 0x3B1E8F, c: 0xFFD93D },
    Track { title: "OCEAN EYES",     artist: "BLUE LAGOON",  dur: 187.0, kind: 1, a: 0x14B8C4, b: 0x0B4F8A, c: 0xBDF7FF },
    Track { title: "GOLDEN HOUR",    artist: "SUNSET CLUB",  dur: 241.0, kind: 2, a: 0xFF8A3D, b: 0xC2185B, c: 0xFFE082 },
    Track { title: "CYBER RAIN",     artist: "GRID RUNNER",  dur: 198.0, kind: 3, a: 0xB400FF, b: 0x00E5FF, c: 0xFF2BD6 },
    Track { title: "CONFETTI",       artist: "PARTY ROBOTS", dur: 176.0, kind: 5, a: 0x7C4DFF, b: 0x00C853, c: 0xFFEA00 },
];

fn rgb(h: u32) -> [f32; 3] {
    [((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0]
}

fn rgba(h: u32, a: f32) -> [f32; 4] {
    let c = rgb(h);
    [c[0], c[1], c[2], a]
}

fn fmt_time(sec: f32) -> String {
    let s = sec.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn text_w(s: &str, dot: f32) -> f32 {
    s.chars().count() as f32 * 6.0 * dot - dot
}

pub struct Text {
    pub x: f32,
    pub y: f32,
    pub s: String,
    /// Size of one font pixel in points.
    pub dot: f32,
    pub dim: bool,
}

/// A plain rounded rect, in points, colour = [r, g, b, a] (sRGB-ish).
pub struct ContentRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub color: [f32; 4],
}

fn rr(v: &mut Vec<ContentRect>, x: f32, y: f32, w: f32, h: f32, r: f32, color: [f32; 4]) {
    v.push(ContentRect { x, y, w, h, r, color });
}

fn circle(v: &mut Vec<ContentRect>, cx: f32, cy: f32, d: f32, color: [f32; 4]) {
    v.push(ContentRect { x: cx - d / 2.0, y: cy - d / 2.0, w: d, h: d, r: d / 2.0, color });
}

/// Triangle (pointing right or left) built from horizontal slices.
fn tri(v: &mut Vec<ContentRect>, x: f32, y: f32, w: f32, h: f32, right: bool, color: [f32; 4]) {
    let n = 14;
    let sh = h / n as f32;
    for i in 0..n {
        let t = (i as f32 + 0.5) / n as f32;
        let ww = (w * (1.0 - (2.0 * t - 1.0).abs())).max(0.5);
        let xx = if right { x } else { x + w - ww };
        v.push(ContentRect { x: xx, y: y + i as f32 * sh, w: ww, h: sh + 0.6, r: 0.0, color });
    }
}

/// Progress bar with a gap around the (glass) thumb.
fn bar(v: &mut Vec<ContentRect>, x: f32, cy: f32, w: f32, frac: f32, gap: f32, fill: [f32; 4], rest: [f32; 4]) {
    let cx = x + frac * w;
    let hh = 5.0;
    let fw = (cx - gap - x).max(0.0);
    let rx = cx + gap;
    let rw = (x + w - rx).max(0.0);
    if fw > 0.5 {
        rr(v, x, cy - hh / 2.0, fw, hh, hh / 2.0, fill);
    }
    if rw > 0.5 {
        rr(v, rx, cy - hh / 2.0, rw, hh, hh / 2.0, rest);
    }
}

/// Flat "AURORA" card: rim + fill.
fn flat(v: &mut Vec<ContentRect>, x: f32, y: f32, w: f32, h: f32, r: f32, c: [f32; 3], a: f32) {
    rr(v, x, y, w, h, r, [c[0], c[1], c[2], 0.38]);
    rr(v, x + 1.5, y + 1.5, (w - 3.0).max(1.0), (h - 3.0).max(1.0), (r - 1.5).max(0.0), [c[0], c[1], c[2], a]);
}

/// Apply the tier's cost limits to a material (zeroed effects are skipped by the shader).
fn adjust(b: Material, t: &Tier) -> Material {
    Material {
        frost: b.frost * t.frost,
        dispersion: if t.disp { b.dispersion } else { 0.0 },
        rim: if t.rim { b.rim } else { 0.0 },
        rim_shade: if t.rim { b.rim_shade } else { 0.0 },
        hairline: if t.hairline { b.hairline } else { 0.0 },
        edge: if t.edge { b.edge } else { 0.0 },
        shadow: if t.shadow { b.shadow } else { Shadow { offset_y: 0.0, blur: 0.0, opacity: 0.0 } },
        ..b
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Back,
    Menu,
    Shuffle,
    Prev,
    Play,
    Next,
    Repeat,
    Row(usize),
}

struct G {
    id: GlassId,
    kind: Kind,
    base: Material,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    /// Flat (AURORA) look: colour + alpha.
    flat: ([f32; 3], f32),
}

struct Layout {
    art: (f32, f32, f32),
    title_y: f32,
    artist_y: f32,
    seek_x: f32,
    seek_y: f32,
    seek_w: f32,
    seek_th: f32,
    times_y: f32,
    vol_x: f32,
    vol_y: f32,
    vol_w: f32,
    vol_th: f32,
    label_y: f32,
    panel_y: f32,
    top: f32,
}

pub struct Ui {
    pub scene: Scene,
    /// Background shader kind of the current track.
    pub bg: usize,
    tier: usize,
    w: f32,
    h: f32,
    lay: Layout,
    glass: Vec<G>,
    seek_thumb: GlassId,
    vol_thumb: GlassId,
    track: usize,
    playing: bool,
    pos0: f32,
    t0: f32,
    now: f32,
    last_applied: f32,
    shuffle: bool,
    repeat: bool,
    vol: f32,
    drag_seek: bool,
    drag_vol: bool,
    down: Option<(GlassId, f32, f32)>,
    rng: u32,
}

#[allow(clippy::too_many_arguments)]
fn add_glass(
    scene: &mut Scene,
    glass: &mut Vec<G>,
    kind: Kind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    base: Material,
    flat: ([f32; 3], f32),
) {
    let id = scene.add(Glass::new(Rect::new(x, y, w, h)).shape(Shape::Rounded(r)).material(base).interactive());
    glass.push(G { id, kind, base, x, y, w, h, r, flat });
}

impl Ui {
    pub fn new(w: f32, h: f32, tier: usize) -> Self {
        let mut scene = Scene::new();
        let u = (h / 857.0).clamp(0.8, 1.15);
        let sp = (w / 392.0).min(1.0);
        let top = 48.0;

        // ---- layout
        let art_size = (w - 96.0).min(232.0 * u).max(120.0);
        let art = ((w - art_size) / 2.0, top + 56.0 * u, art_size);
        let title_y = art.1 + art_size + 20.0 * u;
        let artist_y = title_y + 28.0 * u;
        let (seek_x, seek_w, seek_th) = (28.0, w - 56.0, 30.0);
        let seek_y = artist_y + 14.0 + 30.0 * u;
        let times_y = seek_y + 18.0 * u;
        let ctrl_y = seek_y + 86.0 * u;
        let (vol_x, vol_th) = (72.0, 24.0);
        let vol_w = w - vol_x - 28.0;
        let vol_y = ctrl_y + 37.0 * u + 30.0 * u;
        let label_y = vol_y + 26.0 * u;
        let rows_y0 = label_y + 20.0 * u;
        let row_h = 52.0 * u;
        let gap = 8.0 * u;
        let avail = h - 24.0 - rows_y0;
        let rows_n = (((avail + gap) / (row_h + gap)).floor().max(0.0) as usize).min(3);
        let panel_y = vol_y - 32.0 * u;

        // ---- glass
        let white = [1.0, 1.0, 1.0];
        let mut glass: Vec<G> = Vec::new();
        add_glass(&mut scene, &mut glass, Kind::Back, 16.0, top, 44.0, 44.0, 22.0, Material::regular(), (white, 0.16));
        add_glass(&mut scene, &mut glass, Kind::Menu, w - 60.0, top, 44.0, 44.0, 22.0, Material::regular(), (white, 0.16));

        let cx = w / 2.0;
        let accent = Color::hex(TRACKS[0].a);
        let big = 74.0 * u.min(1.0);
        add_glass(
            &mut scene, &mut glass, Kind::Play, cx - big / 2.0, ctrl_y - big / 2.0, big, big, big / 2.0,
            Material::tinted(accent), (rgb(TRACKS[0].a), 0.65),
        );
        let mid = 54.0 * u.min(1.0);
        add_glass(&mut scene, &mut glass, Kind::Prev, cx - 88.0 * sp - mid / 2.0, ctrl_y - mid / 2.0, mid, mid, mid / 2.0, Material::clear(), (white, 0.14));
        add_glass(&mut scene, &mut glass, Kind::Next, cx + 88.0 * sp - mid / 2.0, ctrl_y - mid / 2.0, mid, mid, mid / 2.0, Material::clear(), (white, 0.14));
        let sm = 44.0 * u.min(1.0);
        add_glass(&mut scene, &mut glass, Kind::Shuffle, cx - 152.0 * sp - sm / 2.0, ctrl_y - sm / 2.0, sm, sm, sm / 2.0, Material::clear(), (white, 0.14));
        add_glass(&mut scene, &mut glass, Kind::Repeat, cx + 152.0 * sp - sm / 2.0, ctrl_y - sm / 2.0, sm, sm, sm / 2.0, Material::clear(), (white, 0.14));

        for i in 0..rows_n {
            let y = rows_y0 + i as f32 * (row_h + gap);
            add_glass(&mut scene, &mut glass, Kind::Row(i), 24.0, y, w - 48.0, row_h, 20.0 * u.min(1.0), Material::regular(), (white, 0.12));
        }

        let seek_thumb = scene.add(
            Glass::new(Rect::new(seek_x - seek_th / 2.0, seek_y - seek_th / 2.0, seek_th, seek_th))
                .shape(Shape::Rounded(seek_th / 2.0))
                .material(Material::clear())
                .interactive(),
        );
        let vol0 = 0.7;
        let vol_thumb = scene.add(
            Glass::new(Rect::new(vol_x + vol0 * vol_w - vol_th / 2.0, vol_y - vol_th / 2.0, vol_th, vol_th))
                .shape(Shape::Rounded(vol_th / 2.0))
                .material(Material::clear())
                .interactive(),
        );

        let lay = Layout {
            art, title_y, artist_y, seek_x, seek_y, seek_w, seek_th, times_y, vol_x, vol_y, vol_w, vol_th,
            label_y, panel_y, top,
        };
        let mut ui = Self {
            scene, bg: TRACKS[0].kind, tier: tier.min(TIERS.len() - 1), w, h, lay, glass, seek_thumb, vol_thumb,
            track: 0, playing: false, pos0: 0.0, t0: 0.0, now: 0.0, last_applied: 0.0,
            shuffle: false, repeat: false, vol: vol0, drag_seek: false, drag_vol: false, down: None, rng: 12345,
        };
        ui.apply_materials();
        ui
    }

    pub fn set_tier(&mut self, tier: usize) {
        self.tier = tier.min(TIERS.len() - 1);
        self.apply_materials();
    }

    /// Materials = preset (+ track accent for the play button) limited by the tier.
    pub fn apply_materials(&mut self) {
        let t = TIERS[self.tier];
        let accent = Color::hex(TRACKS[self.track].a);
        for g in &self.glass {
            let base = if g.kind == Kind::Play { Material::tinted(accent) } else { g.base };
            self.scene.set_material(g.id, adjust(base, &t));
        }
        self.scene.set_material(self.seek_thumb, adjust(Material::clear(), &t));
        self.scene.set_material(self.vol_thumb, adjust(Material::clear(), &t));
    }

    /// Returns true while the glass is still animating (needs more frames).
    pub fn update(&mut self, dt: f32) -> bool {
        self.scene.update(dt)
    }

    pub fn set_now(&mut self, t: f32) {
        self.now = t;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    // ------------------------------------------------------------ playback (fake, UI only)

    fn dur(&self) -> f32 {
        TRACKS[self.track].dur
    }

    fn pos(&self) -> f32 {
        let p = if self.playing { self.pos0 + (self.now - self.t0) } else { self.pos0 };
        p.clamp(0.0, self.dur())
    }

    fn place_seek_thumb(&mut self) {
        let f = (self.pos() / self.dur()).clamp(0.0, 1.0);
        let th = self.lay.seek_th;
        let cx = self.lay.seek_x + f * self.lay.seek_w;
        self.scene.set_frame_with(
            self.seek_thumb,
            Rect::new(cx - th / 2.0, self.lay.seek_y - th / 2.0, th, th),
            Spring::new(0.01, 0.0),
        );
        self.last_applied = self.now;
    }

    fn place_vol_thumb(&mut self) {
        let th = self.lay.vol_th;
        let cx = self.lay.vol_x + self.vol * self.lay.vol_w;
        self.scene.set_frame_with(
            self.vol_thumb,
            Rect::new(cx - th / 2.0, self.lay.vol_y - th / 2.0, th, th),
            Spring::new(0.01, 0.0),
        );
    }

    fn set_pos(&mut self, p: f32) {
        self.pos0 = p.clamp(0.0, self.dur());
        self.t0 = self.now;
        self.place_seek_thumb();
    }

    fn toggle_play(&mut self) {
        self.pos0 = self.pos();
        self.t0 = self.now;
        self.playing = !self.playing;
    }

    fn set_track(&mut self, i: usize) {
        self.track = i % TRACKS.len();
        self.pos0 = 0.0;
        self.t0 = self.now;
        self.bg = TRACKS[self.track].kind;
        self.apply_materials();
        self.place_seek_thumb();
    }

    fn next_track(&mut self) {
        let n = TRACKS.len();
        let i = if self.shuffle {
            self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
            (self.track + 1 + (self.rng >> 16) as usize % (n - 1)) % n
        } else {
            (self.track + 1) % n
        };
        self.set_track(i);
    }

    fn prev_track(&mut self) {
        if self.pos() > 3.0 {
            self.set_pos(0.0);
        } else {
            let n = TRACKS.len();
            self.set_track((self.track + n - 1) % n);
        }
    }

    /// Advance the fake playback. Returns true if something visible changed.
    pub fn tick(&mut self) -> bool {
        if !self.playing {
            return false;
        }
        if self.pos() >= self.dur() - 0.001 {
            if self.repeat {
                self.set_pos(0.0);
            } else {
                self.next_track();
            }
            return true;
        }
        if self.now - self.last_applied >= 0.2 {
            self.place_seek_thumb();
            true
        } else {
            false
        }
    }

    // ------------------------------------------------------------ touch

    fn seek_to_x(&mut self, x: f32) {
        let f = ((x - self.lay.seek_x) / self.lay.seek_w).clamp(0.0, 1.0);
        self.set_pos(f * self.dur());
    }

    fn vol_to_x(&mut self, x: f32) {
        self.vol = ((x - self.lay.vol_x) / self.lay.vol_w).clamp(0.0, 1.0);
        self.place_vol_thumb();
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) {
        self.down = None;
        self.drag_seek = false;
        self.drag_vol = false;
        let took = self.scene.pointer_down(Vec2::new(x, y));
        let (sx, sy, sw) = (self.lay.seek_x, self.lay.seek_y, self.lay.seek_w);
        let (vx, vy, vw) = (self.lay.vol_x, self.lay.vol_y, self.lay.vol_w);
        if (y - sy).abs() < 28.0 && x > sx - 16.0 && x < sx + sw + 16.0 {
            self.drag_seek = true;
            self.seek_to_x(x);
            return;
        }
        if (y - vy).abs() < 24.0 && x > vx - 16.0 && x < vx + vw + 16.0 {
            self.drag_vol = true;
            self.vol_to_x(x);
            return;
        }
        if let Some(id) = took {
            self.down = Some((id, x, y));
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.scene.pointer_move(Vec2::new(x, y));
        if self.drag_seek {
            self.seek_to_x(x);
        } else if self.drag_vol {
            self.vol_to_x(x);
        }
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) {
        self.scene.pointer_up();
        self.drag_seek = false;
        self.drag_vol = false;
        if let Some((id, sx, sy)) = self.down.take() {
            if (x - sx).hypot(y - sy) < 14.0 {
                self.tap(id);
            }
        }
    }

    fn tap(&mut self, id: GlassId) {
        let Some(kind) = self.glass.iter().find(|g| g.id == id).map(|g| g.kind) else { return };
        match kind {
            Kind::Back | Kind::Menu => {}
            Kind::Shuffle => self.shuffle = !self.shuffle,
            Kind::Repeat => self.repeat = !self.repeat,
            Kind::Prev => self.prev_track(),
            Kind::Next => self.next_track(),
            Kind::Play => self.toggle_play(),
            Kind::Row(i) => {
                let t = (self.track + 1 + i) % TRACKS.len();
                self.set_track(t);
                if !self.playing {
                    self.playing = true;
                    self.pos0 = 0.0;
                    self.t0 = self.now;
                }
            }
        }
    }

    // ------------------------------------------------------------ drawing lists

    /// Static scrim under the lower half (drawn into the content texture, refracted by the glass).
    pub fn content_rects(&self) -> Vec<ContentRect> {
        let mut v = Vec::new();
        rr(&mut v, 12.0, self.lay.panel_y, self.w - 24.0, self.h - 16.0 - self.lay.panel_y, 30.0, [0.0, 0.0, 0.0, 0.28]);
        v
    }

    /// AURORA tier: glass drawn as flat translucent cards (liquid-rust is not called).
    pub fn flat_glass(&self) -> Vec<ContentRect> {
        let mut v = Vec::new();
        let tr = &TRACKS[self.track];
        for g in &self.glass {
            let (c, a) = if g.kind == Kind::Play { (rgb(tr.a), 0.65) } else { g.flat };
            flat(&mut v, g.x, g.y, g.w, g.h, g.r, c, a);
        }
        let l = &self.lay;
        let f = (self.pos() / self.dur()).clamp(0.0, 1.0);
        let cx = l.seek_x + f * l.seek_w;
        flat(&mut v, cx - l.seek_th / 2.0, l.seek_y - l.seek_th / 2.0, l.seek_th, l.seek_th, l.seek_th / 2.0, [1.0; 3], 0.35);
        let cx = l.vol_x + self.vol * l.vol_w;
        flat(&mut v, cx - l.vol_th / 2.0, l.vol_y - l.vol_th / 2.0, l.vol_th, l.vol_th, l.vol_th / 2.0, [1.0; 3], 0.35);
        v
    }

    /// Everything that is drawn crisp on top of the glass: album art, bars, icons.
    pub fn overlay_rects(&self) -> Vec<ContentRect> {
        let mut v = Vec::new();
        let tr = &TRACKS[self.track];
        let l = &self.lay;

        // album art with a soft shadow
        let (ax, ay, a) = l.art;
        for (grow, al) in [(18.0f32, 0.05f32), (12.0, 0.07), (6.0, 0.09)] {
            rr(&mut v, ax - grow, ay - grow + 10.0, a + 2.0 * grow, a + 2.0 * grow, 30.0 + grow, [0.0, 0.0, 0.0, al]);
        }
        rr(&mut v, ax, ay, a, a, 28.0, rgba(tr.a, 1.0));
        circle(&mut v, ax + 0.5 * a, ay + 0.5 * a, 0.84 * a, [1.0, 1.0, 1.0, 0.10]);
        circle(&mut v, ax + 0.64 * a, ay + 0.36 * a, 0.62 * a, rgba(tr.b, 0.90));
        circle(&mut v, ax + 0.30 * a, ay + 0.74 * a, 0.40 * a, rgba(tr.c, 0.90));
        rr(&mut v, ax + 0.10 * a, ay + 0.08 * a, 0.45 * a, 0.05 * a, 0.025 * a, [1.0, 1.0, 1.0, 0.18]);

        // seek + volume bars (the glass thumbs sit in the gaps)
        let f = (self.pos() / tr.dur).clamp(0.0, 1.0);
        bar(&mut v, l.seek_x, l.seek_y, l.seek_w, f, l.seek_th / 2.0 + 3.0, rgba(tr.a, 0.95), [1.0, 1.0, 1.0, 0.28]);
        bar(&mut v, l.vol_x, l.vol_y, l.vol_w, self.vol, l.vol_th / 2.0 + 3.0, [1.0, 1.0, 1.0, 0.85], [1.0, 1.0, 1.0, 0.28]);

        // icons
        let white = [1.0, 1.0, 1.0, 1.0];
        for g in &self.glass {
            let (cx, cy) = (g.x + g.w / 2.0, g.y + g.h / 2.0);
            match g.kind {
                Kind::Play => {
                    if self.playing {
                        let bw = g.w * 0.10;
                        let bh = g.w * 0.34;
                        rr(&mut v, cx - bw * 1.4, cy - bh / 2.0, bw, bh, bw * 0.3, white);
                        rr(&mut v, cx + bw * 0.4, cy - bh / 2.0, bw, bh, bw * 0.3, white);
                    } else {
                        let tw = g.w * 0.30;
                        let th = g.w * 0.36;
                        tri(&mut v, cx - tw * 0.35, cy - th / 2.0, tw, th, true, white);
                    }
                }
                Kind::Prev => {
                    let s = g.w * 0.30;
                    rr(&mut v, cx - s * 0.52, cy - s * 0.48, s * 0.16, s * 0.96, s * 0.04, white);
                    tri(&mut v, cx - s * 0.30, cy - s * 0.48, s * 0.64, s * 0.96, false, white);
                }
                Kind::Next => {
                    let s = g.w * 0.30;
                    tri(&mut v, cx - s * 0.34, cy - s * 0.48, s * 0.64, s * 0.96, true, white);
                    rr(&mut v, cx + s * 0.36, cy - s * 0.48, s * 0.16, s * 0.96, s * 0.04, white);
                }
                Kind::Menu => {
                    for dx in [-8.0f32, 0.0, 8.0] {
                        circle(&mut v, cx + dx, cy, 4.5, white);
                    }
                }
                Kind::Shuffle => {
                    if self.shuffle {
                        circle(&mut v, cx, g.y + g.h - 7.0, 4.5, rgba(tr.a, 1.0));
                    }
                }
                Kind::Repeat => {
                    if self.repeat {
                        circle(&mut v, cx, g.y + g.h - 7.0, 4.5, rgba(tr.a, 1.0));
                    }
                }
                Kind::Row(i) => {
                    let t = &TRACKS[(self.track + 1 + i) % TRACKS.len()];
                    let (mx, my) = (g.x + 10.0, g.y + (g.h - 36.0) / 2.0);
                    rr(&mut v, mx, my, 36.0, 36.0, 10.0, rgba(t.a, 1.0));
                    circle(&mut v, mx + 24.0, my + 12.0, 16.0, rgba(t.b, 0.9));
                    circle(&mut v, mx + 11.0, my + 26.0, 11.0, rgba(t.c, 0.9));
                }
                Kind::Back => {}
            }
        }
        v
    }

    pub fn texts(&self) -> Vec<Text> {
        let mut v = Vec::new();
        let l = &self.lay;
        let tr = &TRACKS[self.track];
        let ctr = |s: &str, dot: f32, c: f32| c - text_w(s, dot) / 2.0;
        let mid = |dot: f32, cy: f32| cy - 7.0 * dot / 2.0;
        let cx = self.w / 2.0;

        let head = "NOW PLAYING";
        v.push(Text { x: ctr(head, 2.0, cx), y: mid(2.0, l.top + 22.0), s: head.into(), dot: 2.0, dim: true });
        v.push(Text { x: ctr(tr.title, 3.0, cx), y: l.title_y, s: tr.title.into(), dot: 3.0, dim: false });
        v.push(Text { x: ctr(tr.artist, 2.0, cx), y: l.artist_y, s: tr.artist.into(), dot: 2.0, dim: true });

        v.push(Text { x: l.seek_x, y: l.times_y, s: fmt_time(self.pos()), dot: 2.0, dim: true });
        let total = fmt_time(tr.dur);
        v.push(Text { x: l.seek_x + l.seek_w - text_w(&total, 2.0), y: l.times_y, s: total, dot: 2.0, dim: true });

        v.push(Text { x: 28.0, y: l.vol_y - 5.0, s: "VOL".into(), dot: 1.5, dim: true });
        v.push(Text { x: 24.0, y: l.label_y, s: "UP NEXT".into(), dot: 2.0, dim: true });

        for g in &self.glass {
            let (gx, gy) = (g.x + g.w / 2.0, g.y + g.h / 2.0);
            match g.kind {
                Kind::Back => v.push(Text { x: ctr("<", 3.0, gx), y: mid(3.0, gy), s: "<".into(), dot: 3.0, dim: false }),
                Kind::Shuffle => v.push(Text { x: ctr("S", 2.5, gx), y: mid(2.5, gy - 2.0), s: "S".into(), dot: 2.5, dim: !self.shuffle }),
                Kind::Repeat => v.push(Text { x: ctr("R", 2.5, gx), y: mid(2.5, gy - 2.0), s: "R".into(), dot: 2.5, dim: !self.repeat }),
                Kind::Row(i) => {
                    let t = &TRACKS[(self.track + 1 + i) % TRACKS.len()];
                    let tx = g.x + 10.0 + 36.0 + 12.0;
                    v.push(Text { x: tx, y: g.y + g.h / 2.0 - 16.0, s: t.title.into(), dot: 2.0, dim: false });
                    v.push(Text { x: tx, y: g.y + g.h / 2.0 + 3.0, s: t.artist.into(), dot: 1.5, dim: true });
                    let d = fmt_time(t.dur);
                    v.push(Text { x: g.x + g.w - 14.0 - text_w(&d, 1.5), y: g.y + g.h / 2.0 - 5.0, s: d, dot: 1.5, dim: true });
                }
                _ => {}
            }
        }
        v
    }
}
