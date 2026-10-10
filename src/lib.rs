//! Android (NativeActivity) demo of the `liquid-rust` crate: https://github.com/al1h3n/liquid-rust
//!
//! Per frame:
//!   1. draw an animated background + a dark panel into the `content` texture (our own wgpu pass)
//!   2. `liquid_rust::Renderer::render` draws the glass on top into the swapchain `target`
//!   3. draw the text labels over the glass (the library asks the host to draw labels itself)
//!
//! NOTE for the agent: lines marked `WGPU30` are the places most likely to need small API
//! adjustments for wgpu 30. The reference for wgpu 30 + winit boilerplate is
//! `examples/demo.rs` in https://github.com/al1h3n/liquid-rust.

mod font;
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
}

impl Gfx {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;

        let instance = wgpu::Instance::default(); // WGPU30: may need an InstanceDescriptor
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");

        let caps = surface.get_capabilities(&adapter);
        let base = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let srgb = base.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            color_space: wgpu::ColorSpace::Srgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: base,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: if srgb != base { vec![srgb] } else { vec![] },
        };
        surface.configure(&device, &config);

        let renderer = liquid_rust::Renderer::new(&device, srgb);

        // --- our own pipelines
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
            bind_group_layouts: &[Some(&bgl)], // WGPU30: may be `&[Some(&bgl)]`
            immediate_size: 0,   // WGPU30: renamed to `immediate_size: 0`
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
            multiview_mask: None, // WGPU30: older versions call this `multiview: None`
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

        Self {
            window, surface, device, queue, config, srgb, renderer, bg_pipe, quad_pipe, bind_group, ubuf, qbuf,
            _content: content, content_view, ui, scale, start: now, last: now, touch: None,
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

    fn render(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;
        self.ui.update(dt);

        // WGPU30: get_current_texture returns an enum in recent wgpu versions.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
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

        // 1) content: background + dark panel
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("content"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.content_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None, // WGPU30
            });
            pass.set_pipeline(&self.bg_pipe);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.quad_pipe);
            pass.set_vertex_buffer(0, self.qbuf.slice(..));
            pass.draw(0..6, 0..panel_count);
        }

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
                content_changed: true, // background is animated
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
                multiview_mask: None, // WGPU30
            });
            pass.set_pipeline(&self.quad_pipe);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.qbuf.slice(..));
            pass.draw(0..6, panel_count..total);
        }

        self.queue.submit([enc.finish()]);
        self.window.pre_present_notify();
        drop(frame);
        // surface.present() is automatic or handled by surface in wgpu 30
    }
}

#[derive(Default)]
struct App {
    gfx: Option<Gfx>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            let window = Arc::new(el.create_window(Window::default_attributes()).expect("window"));
            self.gfx = Some(Gfx::new(window));
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        // Android destroys the native window here; drop the surface and recreate on resume.
        self.gfx = None;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => g.resize(s.width, s.height),
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

    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    std::panic::set_hook(Box::new(|info| log::error!("PANIC: {info}")));

    let event_loop = EventLoop::builder().with_android_app(app).build().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut App::default()).expect("run");
}
