//! attempt_4_demo — standalone harness running attempt_4 [`App`]s
//! against a real wgpu surface in a winit window.
//!
//! v0.2 shape: the host owns the event loop, the device/queue, the
//! swapchain, and the render pass. The library (`attempt_4`) owns
//! layout, paint, hit-testing, and visual state. The user owns the
//! [`App`] — its state, its build closure, its event handler.
//!
//! [`run`] takes an [`App`] and runs an event loop that:
//!
//! - Calls `app.build()` on every redraw, applying current hover/press
//!   visuals automatically before paint.
//! - Routes `winit` pointer events through the renderer's hit-tester
//!   and dispatches `Click` events back via `App::on_event`.
//! - Requests a redraw whenever interaction state changes (mouse move,
//!   button down/up) so hover/press visuals are immediate.
//!
//! Keyboard events, scrolling, and focus traversal are not yet wired —
//! they're the next slice. The architecture is intentionally
//! event-pump-shaped so adding them is non-breaking.

use std::sync::Arc;

use attempt_4::{App, Rect, UiRenderer};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Run a windowed app. Blocks until the user closes the window.
///
/// The `App` is owned by the runner; its `&mut self` is updated in
/// response to routed events and read on every `build` call.
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut host = Host {
        title,
        viewport,
        app,
        gfx: None,
        last_pointer: None,
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

struct Host<A: App> {
    title: &'static str,
    viewport: Rect,
    app: A,
    gfx: Option<Gfx>,
    /// Last pointer position in logical pixels (winit reports physical;
    /// we divide by the window's scale factor before storing).
    last_pointer: Option<(f32, f32)>,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: UiRenderer,
}

impl<A: App> ApplicationHandler for Host<A> {
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

        let renderer = UiRenderer::new(&device, &queue, format);

        self.gfx = Some(Gfx { window, surface, device, queue, config, renderer });
        self.gfx.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let scale = gfx.window.scale_factor() as f32;

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let lx = position.x as f32 / scale;
                let ly = position.y as f32 / scale;
                self.last_pointer = Some((lx, ly));
                gfx.renderer.pointer_moved(lx, ly);
                gfx.window.request_redraw();
            }

            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                gfx.renderer.pointer_left();
                gfx.window.request_redraw();
            }

            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let Some((lx, ly)) = self.last_pointer else { return };
                match state {
                    ElementState::Pressed => {
                        gfx.renderer.pointer_down(lx, ly);
                        gfx.window.request_redraw();
                    }
                    ElementState::Released => {
                        if let Some(event) = gfx.renderer.pointer_up(lx, ly) {
                            self.app.on_event(event);
                        }
                        gfx.window.request_redraw();
                    }
                }
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

                let mut tree = self.app.build();
                // Window is configured at physical size; layout works
                // in logical pixels so divide by the OS-reported scale.
                let scale_factor = gfx.window.scale_factor() as f32;
                let viewport = Rect::new(
                    0.0,
                    0.0,
                    gfx.config.width as f32 / scale_factor,
                    gfx.config.height as f32 / scale_factor,
                );
                gfx.renderer.prepare(&gfx.device, &gfx.queue, &mut tree, viewport, scale_factor);

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
