//! Android (NativeActivity) demo of the `liquid-rust` crate.
//!
//! Performance design:
//!  * scene (background + glass) is rendered at reduced resolution and upscaled
//!  * background is "baked": redrawn only when it changes (or when anim=1)
//!  * frames are rendered only while something moves (idle = no GPU work)
//!  * FPS cap via WaitUntil
//!  * built-in benchmark (bench=1): times each pass by disabling it, logs "BENCH ..." lines
//! Settings come from <files dir>/config.txt (written by LogActivity).

mod font;
mod logger;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use winit::application::ApplicationHandler;
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use ui::{text_w, Ui, BG_LIGHT, TEXT_DOT};

#[derive(Clone, Copy, Debug)]
struct Settings {
    fps: f32,
    scale: f32,
    anim: bool,
    disp: f32,
    bench: bool,
}

impl Settings {
    fn load(dir: Option<PathBuf>) -> Self {
        let mut s = Settings { fps: 30.0, scale: 0.75, anim: false, disp: 0.0, bench: false };
        if let Some(d) = dir {
            if let Ok(txt) = std::fs::read_to_string(d.join("config.txt")) {
                for line in txt.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        let Ok(v) = v.trim().parse::<f32>() else { continue };
                        match k.trim() {
                            "fps" => s.fps = v.clamp(10.0, 120.0),
                            "scale" => s.scale = v.clamp(0.25, 1.0),
                            "anim" => s.anim = v >= 0.5,
                            "disp" => s.disp = v.clamp(0.0, 1.0),
                            "bench" => s.bench = v >= 0.5,
                            _ => {}
                        }
                    }
                }
            }
        }
        s
    }
}

// ---------- benchmark ----------

#[derive(Clone, Copy)]
struct Mode {
    name: &'static str,
    bg: bool,
    copy: bool,
    glass: bool,
    fin: bool,
}

const FULL: Mode = Mode { name: "FULL", bg: true, copy: true, glass: true, fin: true };
const BENCH_MODES: [Mode; 6] = [
    FULL,
    Mode { name: "NO GLASS", glass: false, ..FULL },
    Mode { name: "NO BG SHADER", bg: false, ..FULL },
    Mode { name: "NO COPY", copy: false, ..FULL },
    Mode { name: "NO FINAL", fin: false, ..FULL },
    Mode { name: "EMPTY", bg: false, copy: false, glass: false, fin: false },
];

struct Bench {
    mode: usize,
    t0: Instant,
    frames: u32,
    warm: bool,
    done: bool,
    last_fps: f32,
}

// ---------- gpu types ----------

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
    log::info!("targets: screen {}x{} -> scene {}x{} (scale {})", w, h, sw, sh, scale);
    Targets { content, content_view, scene, scene_view, blit_bg, size: (sw, sh) }
}

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
    scale: f32,
    start: Instant,
    last: Instant,
    touch: Option<u64>,
    frames: u64,
    fps_t: Instant,
    fps_n: u32,
    fps_val: f32,
    fps_logs: u32,
    // idle / baked-background state
    dirty: u32,
    content_dirty: bool,
    last_bg: usize,
    bench: Bench,
}

impl Gfx {
    fn new(window: Arc<Window>, settings: Settings) -> Self {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");

        let caps = surface.get_capabilities(&adapter);
        log::info!("adapter: {:?}", adapter.get_info());
        log::info!(
            "surface caps: formats={:?} present_modes={:?} alpha={:?}",
            caps.formats, caps.present_modes, caps.alpha_modes
        );
        let base = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let srgb = base.add_srgb_suffix();
        log::info!("window {}x{} scale={} base={:?} srgb={:?} settings={:?}", w, h, scale, base, srgb, settings);
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

        let t = make_targets(&device, &tex_bgl, &sampler, w, h, settings.scale, srgb);
        let ui = Ui::new(w as f32 / scale, h as f32 / scale, settings.disp);
        let now = Instant::now();
        log::info!("Gfx created");

        Self {
            window, surface, device, queue, config, srgb, renderer, bg_pipe, quad_pipe, blit_pipe, bind_group,
            tex_bgl, sampler, ubuf, qbuf, t, ui, settings, scale, start: now, last: now, touch: None, frames: 0,
            fps_t: now, fps_n: 0, fps_val: 0.0, fps_logs: 0,
            dirty: 8, content_dirty: true, last_bg: 0,
            bench: Bench { mode: 0, t0: now, frames: 0, warm: false, done: !settings.bench, last_fps: 0.0 },
        }
    }

    fn bench_active(&self) -> bool {
        self.settings.bench && !self.bench.done
    }

    /// Should we draw another frame right now?
    fn needs_frame(&self) -> bool {
        self.bench_active() || self.settings.anim || self.dirty > 0
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w == self.config.width && h == self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.t = make_targets(&self.device, &self.tex_bgl, &self.sampler, w, h, self.settings.scale, self.srgb);
        self.ui = Ui::new(w as f32 / self.scale, h as f32 / self.scale, self.settings.disp);
        self.content_dirty = true;
        self.dirty = 8;
    }

    fn touch_event(&mut self, t: winit::event::Touch) {
        self.dirty = 8;
        let (x, y) = (t.location.x as f32 / self.scale, t.location.y as f32 / self.scale);
        match t.phase {
            TouchPhase::Started if self.touch.is_none() => {
                log::debug!("touch down {x:.0},{y:.0}");
                self.touch = Some(t.id);
                self.ui.pointer_down(x, y);
            }
            TouchPhase::Moved if self.touch == Some(t.id) => self.ui.pointer_move(x, y),
            TouchPhase::Ended | TouchPhase::Cancelled if self.touch == Some(t.id) => {
                self.touch = None;
                self.ui.pointer_up(x, y);
            }
            _ => {}
        }
    }

    /// Background (optional) + plain content rects into the content texture.
    fn content_pass(&self, enc: &mut wgpu::CommandEncoder, draw_bg: bool, content_count: u32) {
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
        if draw_bg {
            pass.set_pipeline(&self.bg_pipe);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&self.quad_pipe);
        pass.set_vertex_buffer(0, self.qbuf.slice(..));
        pass.draw(0..6, 0..content_count);
    }

    fn bench_tick(&mut self, now: Instant) {
        let b = &mut self.bench;
        let el = (now - b.t0).as_secs_f32();
        if !b.warm {
            if el >= 1.0 {
                b.warm = true;
                b.t0 = now;
                b.frames = 0;
            }
            return;
        }
        b.frames += 1;
        if el >= 4.0 {
            let fps = b.frames as f32 / el;
            b.last_fps = fps;
            log::info!("BENCH {:<13} {:6.1} fps  {:7.1} ms/frame", BENCH_MODES[b.mode].name, fps, 1000.0 / fps);
            b.mode += 1;
            b.warm = false;
            b.t0 = now;
            b.frames = 0;
            if b.mode >= BENCH_MODES.len() {
                b.done = true;
                b.mode = 0;
                log::info!("BENCH DONE");
            }
        }
    }

    fn render(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;
        if self.ui.update(dt) {
            self.dirty = self.dirty.max(2);
        }

        self.frames += 1;
        let n = self.frames;

        self.fps_n += 1;
        let el = (now - self.fps_t).as_secs_f32();
        if el >= 0.5 {
            self.fps_val = self.fps_n as f32 / el;
            self.fps_n = 0;
            self.fps_t = now;
            self.fps_logs += 1;
            if self.fps_logs % 4 == 1 && !self.bench_active() {
                log::info!("fps={:.1} (cap {}) scale={} anim={}", self.fps_val, self.settings.fps, self.settings.scale, self.settings.anim);
            }
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
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
            log::info!("frame {n}: {}x{} bg={} dt={:.4}", self.config.width, self.config.height, self.ui.bg, dt);
        }

        let mode = if self.bench_active() { BENCH_MODES[self.bench.mode] } else { FULL };
        let bg_changed = self.ui.bg != self.last_bg;
        self.last_bg = self.ui.bg;
        // The background is baked: redrawn only when needed.
        let redraw_content = self.content_dirty || bg_changed || self.settings.anim || self.bench_active();

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

        // Quads: [content rects][labels]
        let mut quads: Vec<Quad> = Vec::new();
        for r in self.ui.content_rects() {
            quads.push(Quad {
                rect: [r.x * s, r.y * s, r.w * s, r.h * s],
                color: r.color,
                radius: [r.r * s, 0.0, 0.0, 0.0],
            });
        }
        let content_count = quads.len() as u32;
        let light = self.ui.bg == BG_LIGHT;
        for t in self.ui.texts() {
            let rgb = if t.panel || !light { [1.0, 1.0, 1.0] } else { [0.08, 0.08, 0.1] };
            push_text(&mut quads, t.x * s, t.y * s, TEXT_DOT * s, &t.s, rgb);
        }
        let fps_label = if self.bench_active() {
            format!("{} {:.0} FPS", mode.name, self.fps_val)
        } else {
            format!("{:.0} FPS {:.0}%", self.fps_val, self.settings.scale * 100.0)
        };
        let fps_x = (w as f32 / s - text_w(&fps_label) - 12.0) * s;
        let fps_rgb = if light { [0.08, 0.08, 0.1] } else { [1.0, 1.0, 0.4] };
        push_text(&mut quads, fps_x, 8.0 * s, TEXT_DOT * s, &fps_label, fps_rgb);
        quads.truncate(MAX_QUADS);
        self.queue.write_buffer(&self.qbuf, 0, bytemuck::cast_slice(&quads));
        let total = quads.len() as u32;

        let target_view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.srgb), ..Default::default() });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        // 1) content (background baked + plain rects) - only when it changed
        if redraw_content {
            self.content_pass(&mut enc, mode.bg, content_count);
        }

        // 2) content -> scene (the glass is drawn on top of this copy)
        if mode.copy {
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

        // 3) the glass (liquid-rust) at low resolution
        if mode.glass {
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
        }

        // 4) upscale to the screen + sharp labels at full resolution
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
            if mode.fin {
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
        }

        self.queue.submit([enc.finish()]);
        if n == 1 {
            log::info!("first frame submitted");
        }
        self.window.pre_present_notify();
        self.queue.present(frame);

        self.content_dirty = false;
        if self.dirty > 0 {
            self.dirty -= 1;
        }
        if self.bench_active() {
            self.bench_tick(now);
        }
    }
}

struct App {
    gfx: Option<Gfx>,
    settings: Settings,
    next_frame: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        log::info!("lifecycle: resumed");
        if self.gfx.is_none() {
            let window = Arc::new(el.create_window(Window::default_attributes()).expect("window"));
            self.gfx = Some(Gfx::new(window, self.settings));
            self.next_frame = Instant::now();
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        log::info!("lifecycle: suspended");
        self.gfx = None;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                log::info!("resized {}x{}", s.width, s.height);
                g.resize(s.width, s.height);
            }
            WindowEvent::Touch(t) => g.touch_event(t),
            WindowEvent::RedrawRequested => g.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(g) = &self.gfx else {
            el.set_control_flow(ControlFlow::Wait);
            return;
        };
        if g.bench_active() {
            // benchmark: draw as fast as the display allows
            g.window.request_redraw();
            el.set_control_flow(ControlFlow::Poll);
            return;
        }
        if !g.needs_frame() {
            // idle: no GPU work at all until the next touch
            el.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        let iv = Duration::from_secs_f32(1.0 / self.settings.fps);
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
    let settings = Settings::load(dir);
    log::info!("android_main start (liquid_demo {}) settings={:?}", env!("CARGO_PKG_VERSION"), settings);

    let event_loop = match EventLoop::builder().with_android_app(app).build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("event loop build failed: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App { gfx: None, settings, next_frame: Instant::now() };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("run_app error: {e}");
    }
    log::info!("event loop ended, exiting demo process");
    log::logger().flush();
    std::process::exit(0);
}
