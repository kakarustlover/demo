//! Android (NativeActivity) demo of the `liquid-rust` crate.

mod font;
mod logger;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use winit::application::ApplicationHandler;
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use ui::{Ui, BG_LIGHT, TEXT_DOT};

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

fn make_content(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("content"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
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
    bind_group: wgpu::BindGroup,
    ubuf: wgpu::Buffer,
    qbuf: wgpu::Buffer,
    _content: wgpu::Texture,
    content_view: wgpu::TextureView,
    ui: Ui,
    scale: f32,
    start: Instant,
    last: Instant,
    touch: Option<u64>,
    frames: u64,
}

impl Gfx {
    fn new(window: Arc<Window>) -> Self {
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
        log::info!("window {}x{} scale={} base={:?} srgb={:?}", w, h, scale, base, srgb);
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
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

        let (content, content_view) = make_content(&device, w, h, srgb);
        let ui = Ui::new(w as f32 / scale, h as f32 / scale);
        let now = Instant::now();
        log::info!("Gfx created");

        Self {
            window, surface, device, queue, config, srgb, renderer, bg_pipe, quad_pipe, bind_group, ubuf, qbuf,
            _content: content, content_view, ui, scale, start: now, last: now, touch: None, frames: 0,
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w == self.config.width && h == self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        let (c, v) = make_content(&self.device, w, h, self.srgb);
        self._content = c;
        self.content_view = v;
        self.ui = Ui::new(w as f32 / self.scale, h as f32 / self.scale);
    }

    fn touch_event(&mut self, t: winit::event::Touch) {
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

    /// Background + dark panel into `view`.
    fn bg_pass(&self, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, label: &str, panel_count: u32) {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.bg_pipe);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.quad_pipe);
        pass.set_vertex_buffer(0, self.qbuf.slice(..));
        pass.draw(0..6, 0..panel_count);
    }

    fn render(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;
        self.ui.update(dt);

        self.frames += 1;
        let n = self.frames;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                if n <= 5 || n % 120 == 0 {
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
        if n <= 3 || n % 300 == 0 {
            log::info!("frame {n}: {}x{} bg={} dt={:.4}", self.config.width, self.config.height, self.ui.bg, dt);
        }

        let (w, h) = (self.config.width, self.config.height);
        let s = self.scale;

        self.queue.write_buffer(
            &self.ubuf,
            0,
            bytemuck::bytes_of(&Uniforms {
                res: [w as f32, h as f32],
                time: (now - self.start).as_secs_f32(),
                kind: self.ui.bg as u32,
            }),
        );

        // Quads: [panel] then [text]
        let mut quads: Vec<Quad> = Vec::new();
        let (px, py, pw, ph, pr) = self.ui.panel();
        quads.push(Quad {
            rect: [px * s, py * s, pw * s, ph * s],
            color: [0.0, 0.0, 0.0, 0.45],
            radius: [pr * s, 0.0, 0.0, 0.0],
        });
        let panel_count = quads.len() as u32;
        let light = self.ui.bg == BG_LIGHT;
        for t in self.ui.texts() {
            let rgb = if t.panel || !light { [1.0, 1.0, 1.0] } else { [0.08, 0.08, 0.1] };
            push_text(&mut quads, t.x * s, t.y * s, TEXT_DOT * s, &t.s, rgb);
        }
        quads.truncate(MAX_QUADS);
        self.queue.write_buffer(&self.qbuf, 0, bytemuck::cast_slice(&quads));
        let total = quads.len() as u32;

        let target_view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.srgb), ..Default::default() });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        // 1) background into the content texture, and ALSO directly into the target
        //    (in case liquid-rust does not copy content into target by itself)
        self.bg_pass(&mut enc, &self.content_view, "content", panel_count);
        self.bg_pass(&mut enc, &target_view, "target-bg", panel_count);

        // 2) the glass (liquid-rust)
        self.renderer.render(
            &self.device,
            &self.queue,
            &mut enc,
            &liquid_rust::Frame {
                content: &self.content_view,
                target: &target_view,
                size: (w, h),
                scale: s,
                content_changed: true,
            },
            &self.ui.scene,
        );

        // 3) labels on top of the glass
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("labels"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.quad_pipe);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.qbuf.slice(..));
            pass.draw(0..6, panel_count..total);
        }

        self.queue.submit([enc.finish()]);
        if n == 1 {
            log::info!("first frame submitted");
        }
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

#[derive(Default)]
struct App {
    gfx: Option<Gfx>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        log::info!("lifecycle: resumed");
        if self.gfx.is_none() {
            let window = Arc::new(el.create_window(Window::default_attributes()).expect("window"));
            self.gfx = Some(Gfx::new(window));
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

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    logger::init(app.internal_data_path());
    log::info!("android_main start (liquid_demo {})", env!("CARGO_PKG_VERSION"));

    let event_loop = match EventLoop::builder().with_android_app(app).build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("event loop build failed: {e}");
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    if let Err(e) = event_loop.run_app(&mut App::default()) {
        log::error!("run_app error: {e}");
    }
    log::info!("event loop ended, exiting demo process");
    log::logger().flush();
    std::process::exit(0);
}
