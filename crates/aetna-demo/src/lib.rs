//! aetna-demo — standalone harness running [`App`]s against a real
//! wgpu surface in a winit window.
//!
//! Shape: the host owns the event loop, the device/queue, the
//! swapchain, and the render pass. The library (`aetna-core` +
//! `aetna-wgpu`) owns layout, paint, hit-testing, and visual state.
//! The user owns the [`App`] — its state, its build closure, its event
//! handler.
//!
//! [`run`] takes an [`App`] and runs an event loop that:
//!
//! - Calls `app.build()` on every redraw, applying current hover/press
//!   visuals automatically before paint.
//! - Routes `winit` pointer events through the renderer's hit-tester
//!   and dispatches `Click` events back via `App::on_event`.
//! - Routes Tab/Shift-Tab through focus traversal and Enter/Space/Escape
//!   through keyboard events.
//! - Requests a redraw whenever interaction state changes (mouse move,
//!   button down/up) so hover/press visuals are immediate.

pub mod icon_gallery;
pub mod showcase;
pub use icon_gallery::IconGallery;
pub use showcase::Showcase;

use std::sync::Arc;

use aetna_core::{App, KeyModifiers, PointerButton, Rect, UiKey};
use aetna_wgpu::Runner;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
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
        modifiers: KeyModifiers::default(),
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
    modifiers: KeyModifiers,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Runner,
}

impl<A: App> ApplicationHandler for Host<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(PhysicalSize::new(
                self.viewport.w as u32,
                self.viewport.h as u32,
            ));
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
                label: Some("aetna_demo::device"),
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
            // COPY_SRC is required so backdrop-sampling shaders can
            // copy the post-Pass-A surface into the runner's snapshot
            // texture mid-frame. Cost is minimal — most surfaces
            // already advertise it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut renderer = Runner::new(&device, &queue, format);
        renderer.set_surface_size(config.width, config.height);
        // Register any custom shaders the app declared. Done once at
        // startup; pipelines are cached for the runner's lifetime.
        for s in self.app.shaders() {
            renderer.register_shader_with(&device, s.name, s.wgsl, s.samples_backdrop);
        }

        self.gfx = Some(Gfx {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
        });
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
                gfx.renderer
                    .set_surface_size(gfx.config.width, gfx.config.height);
                gfx.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let lx = position.x as f32 / scale;
                let ly = position.y as f32 / scale;
                self.last_pointer = Some((lx, ly));
                if let Some(event) = gfx.renderer.pointer_moved(lx, ly) {
                    self.app.on_event(event);
                }
                gfx.window.request_redraw();
            }

            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                gfx.renderer.pointer_left();
                gfx.window.request_redraw();
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = pointer_button(button) else {
                    return;
                };
                let Some((lx, ly)) = self.last_pointer else {
                    return;
                };
                match state {
                    ElementState::Pressed => {
                        if let Some(event) = gfx.renderer.pointer_down(lx, ly, button) {
                            self.app.on_event(event);
                        }
                        gfx.window.request_redraw();
                    }
                    ElementState::Released => {
                        for event in gfx.renderer.pointer_up(lx, ly, button) {
                            self.app.on_event(event);
                        }
                        gfx.window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let Some((lx, ly)) = self.last_pointer else {
                    return;
                };
                // Convert wheel ticks to logical pixels. Line-based
                // deltas come from notched mouse wheels; pixel-based
                // from trackpads. ~50 px/line matches typical OS feel.
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 50.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / scale,
                };
                if gfx.renderer.pointer_wheel(lx, ly, dy) {
                    gfx.window.request_redraw();
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = key_modifiers(modifiers.state());
                gfx.renderer.set_modifiers(self.modifiers);
            }

            WindowEvent::KeyboardInput {
                event:
                    key_event @ winit::event::KeyEvent {
                        state: ElementState::Pressed,
                        ..
                    },
                is_synthetic: false,
                ..
            } => {
                if let Some(key) = map_key(&key_event.logical_key)
                    && let Some(event) =
                        gfx.renderer.key_down(key, self.modifiers, key_event.repeat)
                {
                    self.app.on_event(event);
                }
                // Composed text payload (handles Shift+a → "A", dead
                // keys, etc). winit attaches this on the same press
                // event for non-IME input; IME composition arrives
                // separately via `WindowEvent::Ime`.
                if let Some(text) = &key_event.text
                    && let Some(event) = gfx.renderer.text_input(text.to_string())
                {
                    self.app.on_event(event);
                }
                gfx.window.request_redraw();
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if let Some(event) = gfx.renderer.text_input(text) {
                    self.app.on_event(event);
                }
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
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let mut tree = self.app.build();
                // Snapshot hotkeys alongside build() so the chord list
                // reflects current state (apps can return different
                // hotkeys per mode, e.g. `j/k` only in list view).
                gfx.renderer.set_hotkeys(self.app.hotkeys());
                // Window is configured at physical size; layout works
                // in logical pixels so divide by the OS-reported scale.
                let scale_factor = gfx.window.scale_factor() as f32;
                let viewport = Rect::new(
                    0.0,
                    0.0,
                    gfx.config.width as f32 / scale_factor,
                    gfx.config.height as f32 / scale_factor,
                );
                let prepare = gfx.renderer.prepare(
                    &gfx.device,
                    &gfx.queue,
                    &mut tree,
                    viewport,
                    scale_factor,
                );

                let mut encoder =
                    gfx.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("aetna_demo::encoder"),
                        });
                // `render()` owns pass lifetimes itself so it can split
                // around `BackdropSnapshot` boundaries when the app
                // uses backdrop-sampling shaders. With no boundary it
                // collapses to a single pass — same behaviour as the
                // old `draw(pass)` path.
                gfx.renderer.render(
                    &gfx.device,
                    &mut encoder,
                    &frame.texture,
                    &view,
                    wgpu::LoadOp::Clear(bg_color()),
                );
                gfx.queue.submit(Some(encoder.finish()));
                frame.present();

                // Animation in flight → request another frame so springs
                // keep stepping. When everything settles, the loop idles
                // again until the next input event.
                if prepare.needs_redraw {
                    gfx.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn map_key(key: &Key) -> Option<UiKey> {
    match key {
        Key::Named(NamedKey::Enter) => Some(UiKey::Enter),
        Key::Named(NamedKey::Escape) => Some(UiKey::Escape),
        Key::Named(NamedKey::Tab) => Some(UiKey::Tab),
        Key::Named(NamedKey::Space) => Some(UiKey::Space),
        Key::Named(NamedKey::ArrowUp) => Some(UiKey::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(UiKey::ArrowDown),
        Key::Named(NamedKey::ArrowLeft) => Some(UiKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(UiKey::ArrowRight),
        Key::Named(NamedKey::Backspace) => Some(UiKey::Backspace),
        Key::Named(NamedKey::Delete) => Some(UiKey::Delete),
        Key::Named(NamedKey::Home) => Some(UiKey::Home),
        Key::Named(NamedKey::End) => Some(UiKey::End),
        Key::Character(s) => Some(UiKey::Character(s.to_string())),
        Key::Named(named) => Some(UiKey::Other(format!("{named:?}"))),
        _ => None,
    }
}

fn pointer_button(b: MouseButton) -> Option<PointerButton> {
    match b {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        // Back / Forward / Other → not surfaced; apps that need them can
        // grow the enum.
        _ => None,
    }
}

fn key_modifiers(mods: winit::keyboard::ModifiersState) -> KeyModifiers {
    KeyModifiers {
        shift: mods.shift_key(),
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        logo: mods.super_key(),
    }
}

fn bg_color() -> wgpu::Color {
    let c = aetna_core::tokens::BG_APP;
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
