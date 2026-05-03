//! aetna-web — shared App impl + dual entry points for native + browser.
//!
//! This is the "one UI crate, two backends" split — same shape as
//! whisper-agent-webui at `../../whisper-agent`. The portable [`Counter`]
//! `App` impl is identical in both targets; what differs is the surface
//! provider:
//!
//! - **Native:** the sibling binary `aetna-counter-native` calls
//!   [`launch_native`], which delegates to `aetna_demo::run` (winit
//!   window + wgpu surface, blocking event loop).
//! - **Wasm:** wasm-pack builds this crate as a `cdylib`. The
//!   `#[wasm_bindgen(start)]` entry below opens a wgpu surface against
//!   a `<canvas id="aetna_canvas">` in the host page and drives the
//!   same App through a winit event loop tailored for the browser
//!   (`spawn_app` rather than `run_app`, async adapter request).
//!
//! See `assets/index.html` for the minimal browser harness; see
//! `tools/build_web.sh` for the wasm-pack invocation.
//!
//! Runtime parity check: both targets render the same fixture, accept
//! click + hover input, and exercise the live `aetna-wgpu` paint path
//! (including the v5.1 atlas-backed text). Animation is the same code;
//! only the time source differs (browser raf vs. winit redraw).

use aetna_core::{App, El, Rect, UiEvent, UiEventKind, button, column, h1, row, text, tokens};

/// Default logical viewport. Sized to feel reasonable both as a winit
/// window and as a browser canvas. Browsers can override this by
/// resizing the canvas; the runner reacts to `winit::Resized`.
pub const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 480.0,
    h: 320.0,
};

/// Portable Counter — the v0.2 proof point, in shared form so native
/// and browser show byte-identical UIs (modulo OS font fallbacks; we
/// ship Roboto in-tree so this is mostly a non-issue).
#[derive(Default)]
pub struct Counter {
    pub value: i32,
}

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("−").key("dec").secondary(),
                button("Reset").key("reset").ghost(),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_MD),
            text(if self.value == 0 {
                "Click + or − to change the count.".to_string()
            } else {
                format!("You have clicked +/− a net {} times.", self.value)
            })
            .center_text()
            .muted(),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
        .align(aetna_core::Align::Center)
    }

    fn on_event(&mut self, e: UiEvent) {
        match (e.kind, e.key.as_deref()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("inc")) => self.value += 1,
            (UiEventKind::Click | UiEventKind::Activate, Some("dec")) => self.value -= 1,
            (UiEventKind::Click | UiEventKind::Activate, Some("reset")) => self.value = 0,
            _ => {}
        }
    }
}

// ---- Native entry ----

/// Native entry — opens a winit window and runs [`Counter`] through the
/// `aetna-demo` shared runner. Called from `bin/aetna-counter-native.rs`.
#[cfg(not(target_arch = "wasm32"))]
pub fn launch_native() -> Result<(), Box<dyn std::error::Error>> {
    aetna_demo::run("Aetna — counter (native)", VIEWPORT, Counter::default())
}

// ---- Wasm entry ----
//
// Lives in its own module so it can pull in wasm-only deps without
// polluting the rlib's `pub` surface on native builds. The
// `#[wasm_bindgen(start)]` attribute makes wasm-pack emit a JS shim
// that runs `start_web` automatically when the bundle loads.

#[cfg(target_arch = "wasm32")]
mod web_entry {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use aetna_core::{App, KeyModifiers, Rect, UiKey};
    use aetna_wgpu::Runner;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
    use winit::window::{Window, WindowId};

    use super::{Counter, VIEWPORT};

    const CANVAS_ID: &str = "aetna_canvas";

    #[wasm_bindgen(start)]
    pub fn start_web() {
        // Surface panics in the browser console with a stack trace —
        // without this hook a wasm panic dies silently as `unreachable`.
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);

        let event_loop = EventLoop::new().expect("EventLoop::new");
        let host = Host::<Counter>::new(VIEWPORT, Counter::default());
        // spawn_app hands control to the browser. Native uses
        // run_app(...) which blocks; on wasm32 the event loop is
        // driven by the browser's animation-frame callbacks.
        event_loop.spawn_app(host);
    }

    /// Locate the `<canvas id="aetna_canvas">` element in the host
    /// page. The HTML harness in `assets/index.html` embeds one;
    /// other host pages can mount their own as long as the id matches.
    fn locate_canvas() -> web_sys::HtmlCanvasElement {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        document
            .get_element_by_id(CANVAS_ID)
            .unwrap_or_else(|| panic!("missing #{CANVAS_ID} canvas element"))
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#aetna_canvas is not a canvas")
    }

    /// Resize the canvas's drawing buffer to match its CSS-laid-out
    /// box at the device pixel ratio, then forward the same physical
    /// size to winit so `inner_size()` and the canvas attributes stay
    /// in lockstep. Without this the canvas defaults to 300×150 device
    /// pixels regardless of CSS — the swapchain ends up tiny + stretched
    /// and Firefox's WebGPU backend fails the first present with
    /// "Not enough memory left" because the surface texture and the
    /// canvas drawing buffer disagree.
    fn sync_canvas_to_css(canvas: &web_sys::HtmlCanvasElement, window: &Window) {
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(1.0);
        let css_w = canvas.client_width().max(1) as f64;
        let css_h = canvas.client_height().max(1) as f64;
        let phys_w = (css_w * dpr).round() as u32;
        let phys_h = (css_h * dpr).round() as u32;
        canvas.set_width(phys_w);
        canvas.set_height(phys_h);
        // Tell winit too, so window.inner_size() agrees. The web
        // backend treats request_inner_size as authoritative and
        // updates the canvas drawing buffer / fires Resized; we
        // already set the buffer above so the values match.
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(phys_w, phys_h));
    }

    /// Mirrors `aetna_demo::run`'s `Host` — same shape, different
    /// surface init (async via wasm-bindgen-futures rather than
    /// pollster). Kept inline here so `aetna-demo` stays free of
    /// wasm-only deps.
    struct Host<A: App> {
        viewport: Rect,
        app: A,
        gfx: Rc<RefCell<Option<Gfx>>>,
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

    impl<A: App> Host<A> {
        fn new(viewport: Rect, app: A) -> Self {
            Self {
                viewport,
                app,
                gfx: Rc::new(RefCell::new(None)),
                last_pointer: None,
                modifiers: KeyModifiers::default(),
            }
        }
    }

    impl<A: App + 'static> ApplicationHandler for Host<A> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.gfx.borrow().is_some() {
                return;
            }
            let canvas = locate_canvas();

            // Build the window bound to the existing canvas. We do
            // *not* call `with_inner_size` — on the web backend that
            // forces canvas.width/height to the requested physical
            // pixels, which then disagrees with the surface size if
            // we read it from CSS. Letting winit pick from the canvas
            // attributes (default 300×150 if unset, otherwise whatever
            // the host page declared) keeps inner_size() and the
            // canvas backing buffer in lockstep — and Resized events
            // fire when CSS later resizes the canvas.
            let attrs = Window::default_attributes().with_canvas(Some(canvas.clone()));
            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

            // Force the canvas backing buffer to match the canvas's
            // CSS-laid-out size at the device pixel ratio. Without
            // this the canvas defaults to 300×150 device pixels, the
            // swapchain ends up tiny and stretched, and Firefox's
            // WebGPU backend fails the first present with "not enough
            // memory left" because the surface texture and the canvas
            // drawing buffer disagree.
            sync_canvas_to_css(&canvas, &window);

            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
            let surface = instance
                .create_surface(window.clone())
                .expect("create surface");

            // Adapter + device requests are async on wasm; spawn the
            // setup as a future and stash the result in self.gfx so
            // subsequent resumed/window_event calls find it ready.
            let viewport = self.viewport;
            let gfx_slot = self.gfx.clone();
            let window_for_async = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let adapter = instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::default(),
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: false,
                    })
                    .await
                    .expect("no compatible adapter");

                // Use the adapter's actual limits as the upper bound;
                // downlevel_webgl2_defaults is too tight for the
                // WebGPU backend Firefox picks by default. Capping at
                // adapter.limits() keeps device creation succeeding
                // on integrated GPUs.
                let limits = wgpu::Limits::default().using_resolution(adapter.limits());

                let (device, queue) = adapter
                    .request_device(
                        &wgpu::DeviceDescriptor {
                            label: Some("aetna_web::device"),
                            required_features: wgpu::Features::empty(),
                            required_limits: limits,
                            memory_hints: wgpu::MemoryHints::Performance,
                        },
                        None,
                    )
                    .await
                    .expect("request_device");

                let surface_caps = surface.get_capabilities(&adapter);
                let format = surface_caps
                    .formats
                    .iter()
                    .copied()
                    .find(|f| f.is_srgb())
                    .unwrap_or(surface_caps.formats[0]);
                // Single source of truth for the swapchain size:
                // winit's inner_size() in physical pixels. Same value
                // that aetna-demo's native runner uses; matches what
                // sync_canvas_to_css() set the canvas backing buffer to.
                let inner = window_for_async.inner_size();
                let config = wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format,
                    width: inner.width.max(1),
                    height: inner.height.max(1),
                    present_mode: surface_caps.present_modes[0],
                    alpha_mode: surface_caps.alpha_modes[0],
                    view_formats: vec![],
                    desired_maximum_frame_latency: 2,
                };
                surface.configure(&device, &config);

                let renderer = Runner::new(&device, &queue, format);

                *gfx_slot.borrow_mut() = Some(Gfx {
                    window: window_for_async.clone(),
                    surface,
                    device,
                    queue,
                    config,
                    renderer,
                });
                let _ = viewport;
                window_for_async.request_redraw();
            });
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            let mut gfx_borrow = self.gfx.borrow_mut();
            let Some(gfx) = gfx_borrow.as_mut() else {
                // Async setup hasn't finished; drop the event. The
                // post-setup `request_redraw` will trigger a fresh
                // RedrawRequested once we're ready.
                return;
            };
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

                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    let Some((lx, ly)) = self.last_pointer else {
                        return;
                    };
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

                WindowEvent::MouseWheel { delta, .. } => {
                    let Some((lx, ly)) = self.last_pointer else {
                        return;
                    };
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
                }

                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic: false,
                    ..
                } => {
                    if event.state == ElementState::Pressed
                        && let Some(key) = map_key(&event.logical_key)
                    {
                        if let Some(event) =
                            gfx.renderer.key_down(key, self.modifiers, event.repeat)
                        {
                            self.app.on_event(event);
                        }
                        gfx.window.request_redraw();
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
                            log::error!("surface error: {e}");
                            return;
                        }
                    };
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    let mut tree = self.app.build();
                    gfx.renderer.set_hotkeys(self.app.hotkeys());
                    let scale_factor = gfx.window.scale_factor() as f32;
                    let viewport_rect = Rect::new(
                        0.0,
                        0.0,
                        gfx.config.width as f32 / scale_factor,
                        gfx.config.height as f32 / scale_factor,
                    );
                    let prepare = gfx.renderer.prepare(
                        &gfx.device,
                        &gfx.queue,
                        &mut tree,
                        viewport_rect,
                        scale_factor,
                    );

                    let mut encoder =
                        gfx.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("aetna_web::encoder"),
                            });
                    {
                        let bg = bg_color();
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("aetna_web::pass"),
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

                    if prepare.needs_redraw {
                        gfx.window.request_redraw();
                    }
                    let _ = self.viewport;
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
            Key::Character(s) => Some(UiKey::Character(s.to_string())),
            Key::Named(named) => Some(UiKey::Other(format!("{named:?}"))),
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

    fn srgb_to_linear(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
}
