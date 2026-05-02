//! attempt_4_demo — standalone harness running attempt_4 fixtures
//! against a real wgpu surface in a winit window.
//!
//! The library crate (`attempt_4`) is the substrate; this crate is the
//! "host application" doing what a real consumer would do — owning the
//! event loop, the device/queue, the swapchain, and the render pass.
//! It's intentionally separate so the `attempt_4` crate has zero winit
//! dependency and is host-agnostic.
//!
//! [`run`] takes a tree-builder closure and runs an event loop that
//! re-paints whenever the window asks. v0.1 doesn't react to events
//! (no hover/press over the network), but the structure is in place.

use std::sync::Arc;

use attempt_4::{El, Rect, UiRenderer};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Run a windowed demo, painting the tree returned by `build`.
///
/// `build` is called once at startup; v0.1 doesn't yet reactively
/// rebuild on events (hover/press will land later). The window opens
/// at the tree's natural size (taken from `viewport.w` / `viewport.h`)
/// and re-renders on resize.
pub fn run<F>(title: &'static str, viewport: Rect, build: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn() -> El + 'static,
{
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = App {
        title,
        viewport,
        build: Box::new(build),
        gfx: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    title: &'static str,
    viewport: Rect,
    build: Box<dyn Fn() -> El>,
    gfx: Option<Gfx>,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: UiRenderer,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(PhysicalSize::new(self.viewport.w as u32, self.viewport.h as u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no compatible adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("attempt_4_demo::device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("request_device");

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = UiRenderer::new(&device, format);

        self.gfx = Some(Gfx { window, surface, device, queue, config, renderer });
        self.gfx.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = match gfx.surface.get_current_texture() {
                    Ok(f) => f,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        gfx.surface.configure(&gfx.device, &gfx.config);
                        return;
                    }
                    Err(e) => {
                        eprintln!("surface error: {e}");
                        return;
                    }
                };
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

                let mut tree = (self.build)();
                let viewport = Rect::new(0.0, 0.0, gfx.config.width as f32, gfx.config.height as f32);
                gfx.renderer.prepare(&gfx.device, &gfx.queue, &mut tree, viewport);

                let mut encoder = gfx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("attempt_4_demo::encoder"),
                });
                {
                    let bg = bg_color();
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("attempt_4_demo::pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(bg),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    gfx.renderer.draw(&mut pass);
                }
                gfx.queue.submit(Some(encoder.finish()));
                frame.present();
            }
            _ => {}
        }
    }
}

fn bg_color() -> wgpu::Color {
    let c = attempt_4::tokens::BG_APP;
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
    }
}

/// Surface format is sRGB, but `wgpu::Color::Clear` is taken as
/// linear-space — convert so the clear color matches our token.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
