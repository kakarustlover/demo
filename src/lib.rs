//! Android (NativeActivity) music-player UI test built on the `liquid-rust` crate.
//!
//! Modes (tier.rs): ULTRA (0), OPTIMIZED (2), AURORA (5 = glass off, flat aurora cards).
//! A governor walks down the ladder when the device is too slow. Crash / black-screen
//! detection across launches, catch_unwind safety net, extensive logs (INIT/STAT/TIER/BENCH).
//! Settings come from <files dir>/config.txt: tier=2 auto=1 anim=0 bench=0

mod font;
mod logger;
mod tier;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use winit::application::ApplicationHandler;
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use tier::{LAST, TIERS};
use ui::{text_w, ContentRect, Ui};

const FPS_DOT: f32 = 1.5;

// ---------------------------------------------------------------- settings / state files

#[derive(Clone, Copy, Debug)]
struct Settings {
    tier: usize,
    auto: bool,
    anim: bool,
    bench: bool,
}

fn load_settings(dir: &Option<PathBuf>) -> Settings {
    let mut s = Settings { tier: 2, auto: true, anim: false, bench: false };
    if let Some(d) = dir {
        if let Ok(txt) = std::fs::read_to_string(d.join("config.txt")) {
            for line in txt.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let Ok(v) = v.trim().parse::<f32>() else { continue };
                    match k.trim() {
                        "tier" => s.tier = (v.max(0.0) as usize).min(LAST),
                        "auto" => s.auto = v >= 0.5,
                        "anim" => s.anim = v >= 0.5,
                        "bench" => s.bench = v >= 0.5,
                        _ => {}
                    }
                }
            }
        }
    }
    s
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    /// Launches that did not end cleanly (crash / black screen). Each one lowers the start tier.
    crashes: usize,
    /// Tier the governor settled on.
    auto_tier: usize,
}

fn load_state(dir: &Option<PathBuf>) -> State {
    let mut s = State::default();
    if let Some(d) = dir {
        if let Ok(txt) = std::fs::read_to_string(d.join("state.txt")) {
            for line in txt.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let Ok(v) = v.trim().parse::<usize>() else { continue };
                    match k.trim() {
                        "crashes" => s.crashes = v,
                        "auto_tier" => s.auto_tier = v.min(LAST),
                        _ => {}
                    }
                }
            }
        }
    }
    s
}

fn save_state(dir: &Option<PathBuf>, s: &State) {
    if let Some(d) = dir {
        let _ = std::fs::write(d.join("state.txt"), format!("crashes={}\nauto_tier={}\n", s.crashes, s.auto_tier));
    }
}

fn write_flag(dir: &Option<PathBuf>, content: &str) {
    if let Some(d) = dir {
        let _ = std::fs::write(d.join("running.flag"), content);
    }
}

fn remove_flag(dir: &Option<PathBuf>) {
    if let Some(d) = dir {
        let _ = std::fs::remove_file(d.join("running.flag"));
    }
}

// ---------------------------------------------------------------- gpu types

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Quad {
    rect: [f32; 4],
    color: [f32; 4],
    radius: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    res: [f32; 2],
    time: f32,
    kind: u32,
}

const MAX_QUADS: usize = 16384;

fn lin(c: f32) -> f32 {
    c.powf(2.2)
}

fn push_text(out: &mut Vec<Quad>, x: f32, y: f32, dot: f32, s: &str, rgb: [f32; 3]) {
    let color = [lin(rgb[0]), lin(rgb[1]), lin(rgb[2]), 1.0];
    let mut cx = x;
    for ch in s.chars() {
        let g = font::glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    out.push(Quad {
                        rect: [cx + col as f32 * dot, y + row as f32 * dot, dot, dot],
                        color,
                        radius: [0.0; 4],
                    });
                }
            }
        }
        cx += 6.0 * dot;
    }
}

/// Colours are given in sRGB; the render target is sRGB, so convert rgb to linear here.
fn push_rects(out: &mut Vec<Quad>, rects: &[ContentRect], s: f32) {
    for r in rects {
        out.push(Quad {
            rect: [r.x * s, r.y * s, r.w * s, r.h * s],
            color: [lin(r.color[0]), lin(r.color[1]), lin(r.color[2]), r.color[3]],
            radius: [r.r * s, 0.0, 0.0, 0.0],
        });
    }
}

struct Targets {
    content: wgpu::Texture,
    content_view: wgpu::TextureView,
    scene: wgpu::Texture,
    scene_view: wgpu::TextureView,
    blit_bg: wgpu::BindGroup,
    size: (u32, u32),
}

fn make_targets(
    device: &wgpu::Device,
    tex_bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    w: u32,
    h: u32,
    scale: f32,
    format: wgpu::TextureFormat,
) -> Targets {
    let sw = ((w as f32 * scale).round() as u32).max(1);
    let sh = ((h as f32 * scale).round() as u32).max(1);
    let mk = |label: &str, usage: wgpu::TextureUsages| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    };
    let (content, content_view) = mk(
        "content",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let (scene, scene_view) = mk(
        "scene",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blit"),
        layout: tex_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&scene_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    log::info!("targets: screen {}x{} -> scene {}x{} (scale {:.2})", w, h, sw, sh, scale);
    Targets { content, content_view, scene, scene_view, blit_bg, size: (sw, sh) }
}

// ---------------------------------------------------------------- benchmark / stats

struct Bench {
    mode: usize,
    t0: Instant,
    frames: u32,
    warm: bool,
    done: bool,
    saved_tier: usize,
}

struct Stat {
    t0: Instant,
    frames: u32,
    active: u32,
    gap_sum: f32,
    cpu_ms: f32,
    glass_ms: f32,
}

// ---------------------------------------------------------------- Gfx

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    srgb: wgpu::TextureFormat,
    renderer: liquid_rust::Renderer,
    bg_pipe: wgpu::RenderPipeline,
    quad_pipe: wgpu::RenderPipeline,
    blit_pipe: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    ubuf: wgpu::Buffer,
    qbuf: wgpu::Buffer,
    t: Targets,
    ui: Ui,
    settings: Settings,
    dir: Option<PathBuf>,
    state: State,
    tier: usize,
    scale: f32,
    start: Instant,
    last: Instant,
    touch: Option<u64>,
    frames: u64,
    presented: u64,
    health_ok: bool,
    fps_val: f32,
    // idle / baked-background state
    dirty: u32,
    content_dirty: bool,
    last_bg: usize,
    /// While the (fake) playback runs we wake up 4x per second to move the progress bar.
    next_progress: Instant,
    // governor
    active_run: u32,
    ema: f32,
    since_change: u32,
    bench: Bench,
    st: Stat,
}

impl Gfx {
    fn new(window: Arc<Window>, settings: Settings, state: State, dir: Option<PathBuf>, start_tier: usize) -> Self {
        let t_init = Instant::now();
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("surface");
        log::info!("INIT surface created +{:.0} ms", t_init.elapsed().as_secs_f32() * 1000.0);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("adapter");
        log::info!("INIT adapter +{:.0} ms", t_init.elapsed().as_secs_f32() * 1000.0);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        log::info!("INIT device +{:.0} ms", t_init.elapsed().as_secs_f32() * 1000.0);

        let caps = surface.get_capabilities(&adapter);
        log::info!("adapter: {:?}", adapter.get_info());
        log::info!("adapter features: {:?}", adapter.features());
        log::info!(
            "surface caps: formats={:?} present_modes={:?} alpha={:?}",
            caps.formats, caps.present_modes, caps.alpha_modes
        );
        let base = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let srgb = base.add_srgb_suffix();
        log::info!("window {}x{} density={} base={:?} srgb={:?}", w, h, scale, base, srgb);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: base,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: if srgb != base { vec![srgb] } else { vec![] },
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let renderer = liquid_rust::Renderer::new(&device, srgb);
        log::info!("INIT liquid-rust renderer +{:.0} ms", t_init.elapsed().as_secs_f32() * 1000.0);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit-layout"),
            bind_group_layouts: &[Some(&bgl), Some(&tex_bgl)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let quad_buf = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attrs,
        };
        let bg_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: srgb, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let blit_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: srgb, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let quad_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[Some(quad_buf)],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_quad"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: srgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        log::info!("INIT pipelines +{:.0} ms", t_init.elapsed().as_secs_f32() * 1000.0);

        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let qbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quads"),
            size: (MAX_QUADS * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });

        let tier = if settings.bench { 0 } else { start_tier };
        let t = make_targets(&device, &tex_bgl, &sampler, w, h, TIERS[tier].scale, srgb);
        let ui = Ui::new(w as f32 / scale, h as f32 / scale, tier);
        let now = Instant::now();
        log::info!(
            "INIT done in {:.0} ms: start tier {} ({}), auto={}, bench={}",
            t_init.elapsed().as_secs_f32() * 1000.0, tier, TIERS[tier].name, settings.auto, settings.bench
        );

        Self {
            window, surface, device, queue, config, srgb, renderer, bg_pipe, quad_pipe, blit_pipe, bind_group,
            tex_bgl, sampler, ubuf, qbuf, t, ui, settings, dir, state, tier, scale, start: now, last: now,
            touch: None, frames: 0, presented: 0, health_ok: false, fps_val: 0.0,
            dirty: 8, content_dirty: true, last_bg: 0, next_progress: now,
            active_run: 0, ema: 0.0, since_change: 0,
            bench: Bench { mode: 0, t0: now, frames: 0, warm: false, done: !settings.bench, saved_tier: start_tier },
            st: Stat { t0: now, frames: 0, active: 0, gap_sum: 0.0, cpu_ms: 0.0, glass_ms: 0.0 },
        }
    }

    fn bench_active(&self) -> bool {
        self.settings.bench && !self.bench.done
    }

    /// Should we draw another frame right now?
    fn needs_frame(&self) -> bool {
        self.bench_active() || self.settings.anim || self.dirty > 0
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_secs_f32(1.0 / TIERS[self.tier].fps)
    }

    /// Switch quality tier (rebuilds the render targets if the scale changes).
    fn set_tier(&mut self, new: usize, reason: &str) {
        let new = new.min(LAST);
        if new == self.tier {
            return;
        }
        log::warn!("TIER {} -> {} ({})", TIERS[self.tier].name, TIERS[new].name, reason);
        let old_scale = TIERS[self.tier].scale;
        self.tier = new;
        self.ui.set_tier(new);
        if (TIERS[new].scale - old_scale).abs() > 1e-3 {
            self.t = make_targets(
                &self.device, &self.tex_bgl, &self.sampler,
                self.config.width, self.config.height, TIERS[new].scale, self.srgb,
            );
        }
        self.content_dirty = true;
        self.dirty = 8;
        self.since_change = 0;
        self.active_run = 0;
        self.ema = 0.0;
        log::logger().flush();
    }

    /// Governor / error fallback: lower the tier and remember it for the next launch.
    fn degrade(&mut self, new: usize, reason: &str) {
        self.set_tier(new, reason);
        self.state.auto_tier = self.state.auto_tier.max(self.tier);
        save_state(&self.dir, &self.state);
    }

    fn fallback(&mut self, reason: &str) {
        log::error!("FALLBACK to AURORA: {}", reason);
        self.degrade(LAST, reason);
    }

    fn govern(&mut self, gap: f32) {
        if self.bench_active() || !self.settings.auto || self.tier >= LAST {
            return;
        }
        if gap > 0.25 {
            // idle gap: a new touch/animation streak starts
            self.active_run = 0;
            self.ema = 0.0;
            return;
        }
        self.active_run += 1;
        self.since_change += 1;
        self.ema = if self.ema == 0.0 { gap } else { self.ema * 0.9 + gap * 0.1 };
        let limit = 1.0 / TIERS[self.tier].min_fps;
        if self.since_change > 8 && self.active_run >= 20 && self.ema > limit {
            let step = if self.ema > limit * 2.5 { 2 } else { 1 };
            let reason = format!(
                "auto: avg frame {:.1} ms > {:.1} ms (min {} fps)",
                self.ema * 1000.0, limit * 1000.0, TIERS[self.tier].min_fps
            );
            let to = self.tier + step;
            self.degrade(to, &reason);
        }
    }

    fn bench_tick(&mut self, now: Instant) {
        let el = (now - self.bench.t0).as_secs_f32();
        if !self.bench.warm {
            if el >= 1.0 {
                self.bench.warm = true;
                self.bench.t0 = now;
                self.bench.frames = 0;
            }
            return;
        }
        self.bench.frames += 1;
        if el >= 4.0 {
            let fps = self.bench.frames as f32 / el;
            let t = TIERS[self.bench.mode];
            log::info!(
                "BENCH {:<9} scale={:.2} {:6.1} fps {:7.1} ms/frame",
                t.name, t.scale, fps, 1000.0 / fps
            );
            self.bench.mode += 1;
            self.bench.warm = false;
            self.bench.t0 = now;
            self.bench.frames = 0;
            if self.bench.mode >= TIERS.len() {
                self.bench.done = true;
                log::info!("BENCH DONE");
                let saved = self.bench.saved_tier;
                self.set_tier(saved, "bench finished");
            } else {
                let m = self.bench.mode;
                self.set_tier(m, "bench step");
            }
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w == self.config.width && h == self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.t = make_targets(&self.device, &self.tex_bgl, &self.sampler, w, h, TIERS[self.tier].scale, self.srgb);
        self.ui = Ui::new(w as f32 / self.scale, h as f32 / self.scale, self.tier);
        self.content_dirty = true;
        self.dirty = 8;
    }

    fn touch_event(&mut self, t: winit::event::Touch) {
        self.dirty = 8;
        self.ui.set_now((Instant::now() - self.start).as_secs_f32());
        let (x, y) = (t.location.x as f32 / self.scale, t.location.y as f32 / self.scale);
        match t.phase {
            TouchPhase::Started if self.touch.is_none() => {
                log::debug!("touch down {x:.0},{y:.0}");
                self.touch = Some(t.id);
                self.ui.pointer_down(x, y);
            }
            TouchPhase::Moved if self.touch == Some(t.id) => self.ui.pointer_move(x, y),
            TouchPhase::Ended | TouchPhase::Cancelled if self.touch == Some(t.id) => {
                log::debug!("touch up {x:.0},{y:.0} ({:?})", t.phase);
                self.touch = None;
                self.ui.pointer_up(x, y);
            }
            _ => {}
        }
    }

    /// Background + static content rects into the content texture.
    fn content_pass(&self, enc: &mut wgpu::CommandEncoder, content_count: u32) {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("content"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.t.content_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.bg_pipe);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.quad_pipe);
        pass.set_vertex_buffer(0, self.qbuf.slice(..));
        pass.draw(0..6, 0..content_count);
    }

    fn render(&mut self) {
        let t_start = Instant::now();
        let now = t_start;
        let gap = (now - self.last).as_secs_f32();
        let dt = gap.min(0.05);
        self.last = now;
        if self.ui.update(dt) {
            self.dirty = self.dirty.max(2);
        }
        // fake playback clock (UI only, no audio)
        self.ui.set_now((now - self.start).as_secs_f32());
        self.ui.tick();

        self.frames += 1;
        let n = self.frames;
        let tier = TIERS[self.tier];

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                if n <= 5 || n % 300 == 0 {
                    log::warn!("frame {n}: suboptimal surface");
                }
                f
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                if n <= 5 || n % 120 == 0 {
                    log::warn!("frame {n}: surface timeout/occluded, skipping");
                }
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("frame {n}: surface outdated/lost, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("frame {n}: surface validation error");
                return;
            }
            #[allow(unreachable_patterns)]
            _ => return,
        };
        if n <= 3 {
            log::info!(
                "frame {n}: {}x{} tier={} bg={} dt={:.4}",
                self.config.width, self.config.height, tier.name, self.ui.bg, dt
            );
        }

        let bg_changed = self.ui.bg != self.last_bg;
        self.last_bg = self.ui.bg;
        // The background is baked: redrawn only when it changed (track change) or if anim is on.
        let redraw_content = self.content_dirty || bg_changed || self.settings.anim;

        let (w, h) = (self.config.width, self.config.height);
        let (sw, sh) = self.t.size;
        let s = self.scale;
        let low_scale = s * (sw as f32 / w as f32);
        let t_anim = if self.settings.anim { (now - self.start).as_secs_f32() } else { 0.0 };

        self.queue.write_buffer(
            &self.ubuf,
            0,
            bytemuck::bytes_of(&Uniforms { res: [w as f32, h as f32], time: t_anim, kind: self.ui.bg as u32 }),
        );

        // Quads: [content rects][flat glass (AURORA only)][overlay: art, bars, icons][text]
        let mut quads: Vec<Quad> = Vec::new();
        push_rects(&mut quads, &self.ui.content_rects(), s);
        let content_count = quads.len() as u32;
        if !tier.glass {
            push_rects(&mut quads, &self.ui.flat_glass(), s);
        }
        push_rects(&mut quads, &self.ui.overlay_rects(), s);
        for t in self.ui.texts() {
            let rgb = if t.dim { [0.72, 0.72, 0.78] } else { [1.0, 1.0, 1.0] };
            push_text(&mut quads, t.x * s, t.y * s, t.dot * s, &t.s, rgb);
        }
        let fps_label = format!("{} {:.0} FPS", tier.name, self.fps_val);
        let fps_x = (w as f32 / s - text_w(&fps_label, FPS_DOT) - 8.0) * s;
        push_text(&mut quads, fps_x, 6.0 * s, FPS_DOT * s, &fps_label, [1.0, 1.0, 0.4]);
        quads.truncate(MAX_QUADS);
        self.queue.write_buffer(&self.qbuf, 0, bytemuck::cast_slice(&quads));
        let total = quads.len() as u32;

        let target_view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.srgb), ..Default::default() });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        // 1) content (baked background + scrim) - only when it changed
        if redraw_content {
            self.content_pass(&mut enc, content_count);
        }

        // 2) the glass
        let mut glass_ms = 0.0;
        if tier.glass {
            let tg = Instant::now();
            self.renderer.render(
                &self.device,
                &self.queue,
                &mut enc,
                &liquid_rust::Frame {
                    content: &self.t.content_view,
                    target: &self.t.scene_view,
                    size: (sw, sh),
                    scale: low_scale,
                    content_changed: redraw_content,
                },
                &self.ui.scene,
            );
            glass_ms = tg.elapsed().as_secs_f32() * 1000.0;
        } else {
            // AURORA: no glass shader. Scene = plain copy of the content.
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.t.content,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.t.scene,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
            );
        }

        // 3) upscale the scene to the screen + flat cards (AURORA) + crisp overlay + text
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("final"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit_pipe);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_bind_group(1, &self.t.blit_bg, &[]);
            pass.draw(0..3, 0..1);
            if total > content_count {
                pass.set_pipeline(&self.quad_pipe);
                pass.set_vertex_buffer(0, self.qbuf.slice(..));
                pass.draw(0..6, content_count..total);
            }
        }

        self.queue.submit([enc.finish()]);
        if n == 1 {
            log::info!("first frame submitted");
        }
        self.window.pre_present_notify();
        self.queue.present(frame);
        self.presented += 1;

        // ---- bookkeeping: health flag, stats, governor, benchmark
        self.content_dirty = false;
        if self.dirty > 0 {
            self.dirty -= 1;
        }
        if self.presented == 10 && !self.health_ok {
            self.health_ok = true;
            write_flag(&self.dir, "ok");
            log::info!("HEALTH: 10 frames presented, run marked healthy");
        }

        self.st.frames += 1;
        if gap < 0.25 {
            self.st.active += 1;
            self.st.gap_sum += gap;
        }
        self.st.cpu_ms += t_start.elapsed().as_secs_f32() * 1000.0;
        self.st.glass_ms += glass_ms;
        let el = (now - self.st.t0).as_secs_f32();
        if el >= 1.0 {
            self.fps_val = self.st.frames as f32 / el;
            let avg_gap = if self.st.active > 0 { self.st.gap_sum / self.st.active as f32 * 1000.0 } else { 0.0 };
            log::info!(
                "STAT tier={} fps={:.1} frame={:.1}ms cpu={:.2}ms glass_cpu={:.2}ms scale={:.2} frames={} bg={}",
                tier.name, self.fps_val, avg_gap,
                self.st.cpu_ms / self.st.frames as f32,
                self.st.glass_ms / self.st.frames as f32,
                tier.scale, self.st.frames, self.ui.bg
            );
            self.st = Stat { t0: now, frames: 0, active: 0, gap_sum: 0.0, cpu_ms: 0.0, glass_ms: 0.0 };
        }

        if self.bench_active() {
            self.bench_tick(now);
        } else {
            self.govern(gap);
        }
    }
}

// ---------------------------------------------------------------- app

struct App {
    gfx: Option<Gfx>,
    settings: Settings,
    state: State,
    dir: Option<PathBuf>,
    start_tier: usize,
    next_frame: Instant,
    panics: u32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        log::info!("lifecycle: resumed");
        if self.gfx.is_none() {
            write_flag(&self.dir, "started");
            let window = Arc::new(el.create_window(Window::default_attributes()).expect("window"));
            self.gfx = Some(Gfx::new(window, self.settings, self.state, self.dir.clone(), self.start_tier));
            self.next_frame = Instant::now();
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        log::info!("lifecycle: suspended");
        if let Some(g) = &self.gfx {
            if g.health_ok {
                remove_flag(&self.dir);
            } else {
                log::warn!("suspended before 10 healthy frames: crash flag kept -> next launch starts one tier lower");
            }
        }
        self.gfx = None;
        log::logger().flush();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                log::info!("resized {}x{}", s.width, s.height);
                g.resize(s.width, s.height);
            }
            WindowEvent::Focused(f) => log::info!("focused: {f}"),
            WindowEvent::Occluded(o) => log::info!("occluded: {o}"),
            WindowEvent::Touch(t) => g.touch_event(t),
            WindowEvent::RedrawRequested => {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.render()));
                if r.is_err() {
                    self.panics += 1;
                    log::error!("render() panicked (#{})", self.panics);
                    if self.panics > 5 {
                        log::error!("too many panics, exiting");
                        log::logger().flush();
                        std::process::exit(2);
                    }
                    g.fallback("render() panicked");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(g) = self.gfx.as_mut() else {
            el.set_control_flow(ControlFlow::Wait);
            return;
        };
        if g.bench_active() {
            // benchmark: draw as fast as the display allows
            g.window.request_redraw();
            el.set_control_flow(ControlFlow::Poll);
            return;
        }
        let now = Instant::now();
        // fake playback: wake up 4x per second to move the progress bar
        if g.ui.is_playing() && now >= g.next_progress {
            g.dirty = g.dirty.max(1);
            g.next_progress = now + Duration::from_millis(250);
        }
        if !g.needs_frame() {
            if g.ui.is_playing() {
                el.set_control_flow(ControlFlow::WaitUntil(g.next_progress));
            } else {
                // idle: no GPU work at all until the next touch
                el.set_control_flow(ControlFlow::Wait);
            }
            return;
        }
        let iv = g.frame_interval();
        if now >= self.next_frame {
            g.window.request_redraw();
            self.next_frame = if now > self.next_frame + iv { now + iv } else { self.next_frame + iv };
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    let dir = app.internal_data_path();
    logger::init(dir.clone());
    let settings = load_settings(&dir);
    let mut state = load_state(&dir);

    // crash / black-screen detection: the flag is removed only after a healthy, clean run
    let prev_flag = dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("running.flag")).ok());
    if let Some(f) = &prev_flag {
        state.crashes += 1;
        save_state(&dir, &state);
        log::error!("PREVIOUS RUN DID NOT END CLEANLY (flag='{}') -> crashes={}", f.trim(), state.crashes);
    } else {
        log::info!("previous run ended cleanly (crashes={})", state.crashes);
    }
    let start_tier = (settings.tier.max(state.auto_tier) + state.crashes).min(LAST);
    log::info!(
        "android_main start (liquid_demo {}) settings={:?} state={:?} -> start tier {} ({})",
        env!("CARGO_PKG_VERSION"), settings, state, start_tier, TIERS[start_tier].name
    );

    let event_loop = match EventLoop::builder().with_android_app(app).build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("event loop build failed: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App { gfx: None, settings, state, dir, start_tier, next_frame: Instant::now(), panics: 0 };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("run_app error: {e}");
    }
    log::info!("event loop ended, exiting demo process");
    log::logger().flush();
    std::process::exit(0);
}
