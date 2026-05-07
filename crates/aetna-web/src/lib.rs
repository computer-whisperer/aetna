//! Browser wasm entry point for the shared [`Showcase`] app.
//!
//! This crate is a host/demo package, not the general app-author API.
//! Write normal UI code against `aetna_core::prelude::*` and use
//! `aetna-winit-wgpu` for a native desktop window. Use this crate when
//! you want the browser path for the backend-neutral [`Showcase`]
//! fixture from `aetna-fixtures`.
//!
//! - **Wasm:** `wasm-pack build --target web` ships this crate as a
//!   `cdylib`. The `#[wasm_bindgen(start)]` entry below opens a wgpu
//!   surface against a `<canvas id="aetna_canvas">` in the host page
//!   and drives [`Showcase`] through a winit event loop tailored for
//!   the browser (`spawn_app` rather than `run_app`, async adapter
//!   request).
//! - **Native parity:** run the same app via `cargo run -p aetna-examples --bin
//!   showcase`. There's no separate native bin in this crate — the
//!   reusable native host lives in `aetna-winit-wgpu`, with
//!   `aetna-examples` providing the demo binary.
//!
//! The package includes `assets/index.html` as a minimal browser
//! harness for the generated wasm bundle.
//!
//! Runtime parity check: both targets render the same fixture, accept
//! click + hover + scroll + keyboard input, and exercise the live
//! `aetna-wgpu` paint path. Animation is the same code; only the time
//! source differs (browser raf vs. winit redraw).

use aetna_core::Rect;
pub use aetna_fixtures::Showcase;

/// Default logical viewport. Sized to feel reasonable both as a winit
/// window and as a browser canvas. Browsers can override this by
/// resizing the canvas; the runner reacts to `winit::Resized`.
pub const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 900.0,
    h: 640.0,
};

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

    use aetna_core::{App, Cursor, KeyModifiers, PointerButton, Rect, UiKey};
    use aetna_wgpu::{MsaaTarget, PrepareTimings, Runner};

    const SAMPLE_COUNT: u32 = 4;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use web_time::Instant;
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
    use winit::window::{CursorIcon, Window, WindowId};

    use super::{Showcase, VIEWPORT};

    /// Number of redraws to accumulate before logging an averaged
    /// frame-timing line. 60 → roughly once per second at 60fps when
    /// animations are in flight; for idle UI (no redraws) the log
    /// just stops, which is the right behavior.
    const FRAME_LOG_INTERVAL: u32 = 60;

    /// Rolling per-frame timing bucket. Three top-level CPU stages
    /// (`build`, `prepare`, `submit`) plus a per-stage breakdown of
    /// what's inside `prepare` (layout / draw_ops / paint / gpu_upload
    /// / snapshot — see [`PrepareTimings`]). `inter` is the wall-clock
    /// interval between consecutive RedrawRequested calls; comparing
    /// `build + prepare + submit` against `inter` shows how much frame
    /// budget the CPU is burning vs. how much the browser's rAF throttle
    /// gives us.
    #[derive(Default)]
    struct FrameStats {
        build_us: u64,
        prepare_us: u64,
        submit_us: u64,
        inter_us: u64,
        // Sub-buckets inside prepare. Sum is ~prepare_us minus a few
        // microseconds of Instant::now() overhead.
        layout_us: u64,
        draw_ops_us: u64,
        paint_us: u64,
        gpu_upload_us: u64,
        snapshot_us: u64,
        samples: u32,
        last_frame_start: Option<Instant>,
    }

    impl FrameStats {
        fn record(
            &mut self,
            frame_start: Instant,
            t1: Instant,
            t2: Instant,
            t3: Instant,
            prep: PrepareTimings,
        ) {
            self.build_us += (t1 - frame_start).as_micros() as u64;
            self.prepare_us += (t2 - t1).as_micros() as u64;
            self.submit_us += (t3 - t2).as_micros() as u64;
            self.layout_us += prep.layout.as_micros() as u64;
            self.draw_ops_us += prep.draw_ops.as_micros() as u64;
            self.paint_us += prep.paint.as_micros() as u64;
            self.gpu_upload_us += prep.gpu_upload.as_micros() as u64;
            self.snapshot_us += prep.snapshot.as_micros() as u64;
            if let Some(prev) = self.last_frame_start {
                self.inter_us += (frame_start - prev).as_micros() as u64;
            }
            self.last_frame_start = Some(frame_start);
            self.samples += 1;
            if self.samples >= FRAME_LOG_INTERVAL {
                self.flush();
            }
        }

        fn flush(&mut self) {
            // `inter` averages over `samples - 1` because the first
            // frame in each window has no prior frame to diff against.
            let n = self.samples as u64;
            let inter_n = (self.samples.saturating_sub(1)) as u64;
            let build = self.build_us / n;
            let prepare = self.prepare_us / n;
            let submit = self.submit_us / n;
            let layout = self.layout_us / n;
            let draw_ops = self.draw_ops_us / n;
            let paint = self.paint_us / n;
            let gpu_upload = self.gpu_upload_us / n;
            let snapshot = self.snapshot_us / n;
            let cpu = build + prepare + submit;
            let inter = self.inter_us.checked_div(inter_n).unwrap_or(0);
            let util = (cpu * 100).checked_div(inter).unwrap_or(0);
            log::info!(
                "frame[{n}] inter={:.2}ms cpu={:.2}ms util={util}% | build={:.2} prepare={:.2} (layout={:.2} draw_ops={:.2} paint={:.2} gpu={:.2} snapshot={:.2}) submit={:.2}",
                inter as f64 / 1000.0,
                cpu as f64 / 1000.0,
                build as f64 / 1000.0,
                prepare as f64 / 1000.0,
                layout as f64 / 1000.0,
                draw_ops as f64 / 1000.0,
                paint as f64 / 1000.0,
                gpu_upload as f64 / 1000.0,
                snapshot as f64 / 1000.0,
                submit as f64 / 1000.0,
            );
            self.build_us = 0;
            self.prepare_us = 0;
            self.submit_us = 0;
            self.inter_us = 0;
            self.layout_us = 0;
            self.draw_ops_us = 0;
            self.paint_us = 0;
            self.gpu_upload_us = 0;
            self.snapshot_us = 0;
            self.samples = 0;
            // Keep last_frame_start so `inter` in the next window
            // includes the gap from the last logged frame to the
            // first frame of the new window.
        }
    }

    const CANVAS_ID: &str = "aetna_canvas";

    #[wasm_bindgen(start)]
    pub fn start_web() {
        // Surface panics in the browser console with a stack trace —
        // without this hook a wasm panic dies silently as `unreachable`.
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);

        let event_loop = EventLoop::new().expect("EventLoop::new");
        let host = Host::<Showcase>::new(VIEWPORT, Showcase::new());
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

    /// Mirrors the native winit + wgpu host shape, but with browser
    /// surface init (async via wasm-bindgen-futures rather than
    /// pollster). Kept inline here so `aetna-winit-wgpu` stays free of
    /// wasm-only deps.
    struct Host<A: App> {
        viewport: Rect,
        app: A,
        gfx: Rc<RefCell<Option<Gfx>>>,
        last_pointer: Option<(f32, f32)>,
        modifiers: KeyModifiers,
        stats: FrameStats,
        /// Last cursor pushed to `Window::set_cursor`. winit-web maps
        /// the icon to `canvas.style.cursor` so this drives the
        /// browser's CSS cursor; we cache to avoid resetting the same
        /// string each frame.
        last_cursor: Cursor,
    }

    struct Gfx {
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        renderer: Runner,
        msaa: MsaaTarget,
    }

    fn surface_extent(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        }
    }

    impl<A: App> Host<A> {
        fn new(viewport: Rect, app: A) -> Self {
            Self {
                viewport,
                app,
                gfx: Rc::new(RefCell::new(None)),
                last_pointer: None,
                modifiers: KeyModifiers::default(),
                stats: FrameStats::default(),
                last_cursor: Cursor::Default,
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

            // Allow both browser backends. wgpu prefers WebGPU when
            // available (Chrome/Edge stable) and falls back to WebGL2
            // otherwise. WebGPU is required for backdrop-sampling
            // shaders (`liquid_glass`) because WebGL2 surfaces don't
            // advertise `COPY_SRC` on the swapchain texture, so the
            // snapshot copy can't run — we register backdrop shaders
            // only when the chosen adapter's surface supports COPY_SRC,
            // which in practice means "WebGPU was selected."
            //
            // Firefox: as of 2026-05, Firefox's WebGPU implementation
            // still wedges its compositor on pointer events with our
            // atlas-uploading path (whole canvas goes black until the
            // cursor leaves). The workaround on the user side is to
            // disable WebGPU in `about:config` (`dom.webgpu.enabled =
            // false`); wgpu then transparently picks WebGL2 here and
            // backdrop shaders are skipped via the COPY_SRC check
            // below. Revisit when Firefox WebGPU stabilises.
            let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
            instance_desc.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
            let instance = wgpu::Instance::new(instance_desc);
            let surface = instance
                .create_surface(window.clone())
                .expect("create surface");

            // Adapter + device requests are async on wasm; spawn the
            // setup as a future and stash the result in self.gfx so
            // subsequent resumed/window_event calls find it ready.
            //
            // `App::shaders()` is captured here (before the move into
            // the async block) so the runner can register custom
            // shaders the App declares — including backdrop-sampling
            // ones like `liquid_glass`. Without this the showcase's
            // glass card draws are silently dropped because the
            // pipeline doesn't exist.
            let viewport = self.viewport;
            let shaders = self.app.shaders();
            let theme = self.app.theme();
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

                // WebGL2 has a tighter feature/limit envelope than
                // native; downlevel_webgl2_defaults is the matching
                // baseline. Cap at the adapter's actual limits so
                // device creation succeeds on every integrated GPU.
                let limits =
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());

                let (device, queue) = adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: Some("aetna_web::device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: limits,
                        experimental_features: wgpu::ExperimentalFeatures::default(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        trace: wgpu::Trace::Off,
                    })
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
                // that the native winit + wgpu host uses; matches what
                // sync_canvas_to_css() set the canvas backing buffer to.
                let inner = window_for_async.inner_size();
                // COPY_SRC is required so backdrop-sampling shaders can
                // copy the post-Pass-A surface into the runner's
                // snapshot texture mid-frame. WebGL2 surfaces typically
                // advertise it; if the adapter ever doesn't, we fall
                // back to RENDER_ATTACHMENT-only and any backdrop
                // shaders the App declared simply won't paint a glass
                // surface (the rest of the UI is unaffected).
                let want_copy_src = surface_caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
                let usage = if want_copy_src {
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
                } else {
                    log::warn!(
                        "aetna-web: surface does not advertise COPY_SRC; backdrop-sampling \
                         shaders will paint nothing on this backend"
                    );
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                };
                let config = wgpu::SurfaceConfiguration {
                    usage,
                    format,
                    width: inner.width.max(1),
                    height: inner.height.max(1),
                    present_mode: surface_caps.present_modes[0],
                    alpha_mode: surface_caps.alpha_modes[0],
                    view_formats: vec![],
                    desired_maximum_frame_latency: 2,
                };
                surface.configure(&device, &config);

                let mut renderer = Runner::with_sample_count(&device, &queue, format, SAMPLE_COUNT);
                renderer.set_theme(theme);
                renderer.set_surface_size(config.width, config.height);
                // Register every shader the App declared. If the
                // surface doesn't support COPY_SRC (so multi-pass
                // backdrop sampling is impossible), skip the backdrop
                // shaders rather than registering them and rendering
                // garbage.
                for s in shaders {
                    if s.samples_backdrop && !want_copy_src {
                        continue;
                    }
                    renderer.register_shader_with(&device, s.name, s.wgsl, s.samples_backdrop);
                }

                let msaa = MsaaTarget::new(&device, format, surface_extent(&config), SAMPLE_COUNT);
                *gfx_slot.borrow_mut() = Some(Gfx {
                    window: window_for_async.clone(),
                    surface,
                    device,
                    queue,
                    config,
                    renderer,
                    msaa,
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
                    gfx.renderer
                        .set_surface_size(gfx.config.width, gfx.config.height);
                    let extent = surface_extent(&gfx.config);
                    if !gfx.msaa.matches(extent) {
                        gfx.msaa =
                            MsaaTarget::new(&gfx.device, gfx.config.format, extent, SAMPLE_COUNT);
                    }
                    gfx.window.request_redraw();
                }

                WindowEvent::CursorMoved { position, .. } => {
                    let lx = position.x as f32 / scale;
                    let ly = position.y as f32 / scale;
                    self.last_pointer = Some((lx, ly));
                    for event in gfx.renderer.pointer_moved(lx, ly) {
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
                            for event in gfx.renderer.pointer_down(lx, ly, button) {
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
                    if let Some(key) = map_key(&key_event.logical_key) {
                        for event in gfx.renderer.key_down(key, self.modifiers, key_event.repeat) {
                            self.app.on_event(event);
                        }
                    }
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
                    let frame_start = Instant::now();
                    let frame = match gfx.surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                        wgpu::CurrentSurfaceTexture::Lost
                        | wgpu::CurrentSurfaceTexture::Outdated => {
                            gfx.surface.configure(&gfx.device, &gfx.config);
                            return;
                        }
                        other => {
                            log::error!("surface unavailable: {other:?}");
                            return;
                        }
                    };
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    self.app.before_build();
                    let mut tree = self.app.build();
                    gfx.renderer.set_theme(self.app.theme());
                    gfx.renderer.set_hotkeys(self.app.hotkeys());
                    gfx.renderer.set_selection(self.app.selection());
                    let t_after_build = Instant::now();

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
                    let t_after_prepare = Instant::now();

                    // Forward the resolved cursor to the canvas. winit's
                    // web backend turns set_cursor(CursorIcon::...) into
                    // canvas.style.cursor = "..." — same plumbing as
                    // native, just with a CSS string at the end.
                    let cursor = gfx.renderer.ui_state().cursor(&tree);
                    if cursor != self.last_cursor {
                        gfx.window.set_cursor(winit_cursor(cursor));
                        self.last_cursor = cursor;
                    }

                    let mut encoder =
                        gfx.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("aetna_web::encoder"),
                            });
                    // `render()` owns pass lifetimes itself so it can
                    // split around `BackdropSnapshot` boundaries when
                    // the App uses backdrop-sampling shaders. With no
                    // boundary it collapses to a single Clear pass —
                    // same behaviour as the old `begin_render_pass +
                    // draw + end_render_pass` path.
                    gfx.renderer.render(
                        &gfx.device,
                        &mut encoder,
                        &frame.texture,
                        &view,
                        Some(&gfx.msaa.view),
                        wgpu::LoadOp::Clear(bg_color()),
                    );
                    gfx.queue.submit(Some(encoder.finish()));
                    frame.present();
                    let t_after_submit = Instant::now();

                    self.stats.record(
                        frame_start,
                        t_after_build,
                        t_after_prepare,
                        t_after_submit,
                        prepare.timings,
                    );

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
            Key::Named(NamedKey::Backspace) => Some(UiKey::Backspace),
            Key::Named(NamedKey::Delete) => Some(UiKey::Delete),
            Key::Named(NamedKey::Home) => Some(UiKey::Home),
            Key::Named(NamedKey::End) => Some(UiKey::End),
            Key::Named(NamedKey::PageUp) => Some(UiKey::PageUp),
            Key::Named(NamedKey::PageDown) => Some(UiKey::PageDown),
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

    /// Translate an Aetna [`Cursor`] to winit's [`CursorIcon`]. winit's
    /// web backend then maps that to a CSS `cursor:` string and writes
    /// it to the canvas's inline style — so this is the only piece of
    /// platform-specific cursor wiring the browser host needs.
    /// `Cursor` is `non_exhaustive`; new variants land in `aetna-core`
    /// and a parallel arm here, with the wildcard as a forward-compat
    /// fallback.
    fn winit_cursor(cursor: Cursor) -> CursorIcon {
        match cursor {
            Cursor::Default => CursorIcon::Default,
            Cursor::Pointer => CursorIcon::Pointer,
            Cursor::Text => CursorIcon::Text,
            Cursor::NotAllowed => CursorIcon::NotAllowed,
            Cursor::Grab => CursorIcon::Grab,
            Cursor::Grabbing => CursorIcon::Grabbing,
            Cursor::Move => CursorIcon::Move,
            Cursor::EwResize => CursorIcon::EwResize,
            Cursor::NsResize => CursorIcon::NsResize,
            Cursor::NwseResize => CursorIcon::NwseResize,
            Cursor::NeswResize => CursorIcon::NeswResize,
            Cursor::ColResize => CursorIcon::ColResize,
            Cursor::RowResize => CursorIcon::RowResize,
            Cursor::Crosshair => CursorIcon::Crosshair,
            _ => CursorIcon::Default,
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
