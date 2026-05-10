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

    use aetna_core::{
        App, BuildCx, Cursor, FrameTrigger, HostDiagnostics, KeyModifiers, Palette, PointerButton,
        Rect, UiKey,
    };
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

    /// Wire the global `tracing` subscriber to `tracing-wasm`, which
    /// emits `performance.mark` / `performance.measure` calls for every
    /// span. Open DevTools → Performance, hit Record, exercise the UI;
    /// each span shows up as a labeled User Timing measure in the
    /// flamegraph (`prepare::layout`, `paint::text::shape_runs`, etc).
    /// Defaults are fine — span events go to console.log, measures get
    /// written, and the subscriber only sees enabled spans (no extra
    /// filter wiring needed on top of the `profiling` feature).
    #[cfg(feature = "profiling")]
    fn install_profiling_subscriber() {
        tracing_wasm::set_as_global_default();
    }

    #[wasm_bindgen(start)]
    pub fn start_web() {
        // Surface panics in the browser console with a stack trace —
        // without this hook a wasm panic dies silently as `unreachable`.
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
        // When built with `--features profiling`, route every
        // `profile_span!` call to the browser's User Timing API so spans
        // show up as named measures in DevTools → Performance alongside
        // the page's own frame/script work. Off-builds compile this away.
        #[cfg(feature = "profiling")]
        install_profiling_subscriber();

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

    /// Read the canvas's CSS-laid-out box at the device pixel ratio.
    /// Returned size is what the swapchain backing buffer should match;
    /// callers pass it to `apply_canvas_size` to actually reconfigure
    /// the surface.
    fn measure_canvas(canvas: &web_sys::HtmlCanvasElement) -> (u32, u32) {
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(1.0);
        let css_w = canvas.client_width().max(1) as f64;
        let css_h = canvas.client_height().max(1) as f64;
        let phys_w = (css_w * dpr).round() as u32;
        let phys_h = (css_h * dpr).round() as u32;
        (phys_w, phys_h)
    }

    /// Set the canvas's drawing buffer to `(phys_w, phys_h)` and
    /// reconfigure the surface + MSAA target to match. Called once at
    /// initial setup and on every ResizeObserver fire afterward.
    ///
    /// We bypass winit's `request_inner_size` round-trip — the web
    /// backend doesn't reliably translate it into a `Resized` event, so
    /// canvas resizes mid-session were leaving the swapchain stretched
    /// at the original size until the page reloaded. Doing the
    /// reconfigure inline keeps the surface in lockstep with the
    /// canvas.
    fn apply_canvas_size(
        canvas: &web_sys::HtmlCanvasElement,
        gfx: &mut Gfx,
        phys_w: u32,
        phys_h: u32,
    ) {
        canvas.set_width(phys_w);
        canvas.set_height(phys_h);
        if gfx.config.width == phys_w && gfx.config.height == phys_h {
            return;
        }
        gfx.config.width = phys_w;
        gfx.config.height = phys_h;
        gfx.surface.configure(&gfx.device, &gfx.config);
        gfx.renderer.set_surface_size(phys_w, phys_h);
        let extent = surface_extent(&gfx.config);
        if !gfx.msaa.matches(extent) {
            gfx.msaa = MsaaTarget::new(&gfx.device, gfx.render_format, extent, SAMPLE_COUNT);
        }
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
        /// Reason the next redraw is being requested. Each event handler
        /// that calls `request_redraw` sets this beforehand; the
        /// RedrawRequested arm consumes it once and snapshots it into
        /// [`HostDiagnostics::trigger`]. Defaults back to `Other` after
        /// each consume — safe fallback for redraws the host can't
        /// attribute (e.g. the post-async-setup `request_redraw`).
        next_trigger: FrameTrigger,
        /// Wall clock at the start of the previous redraw; diff with
        /// the next frame's start gives `last_frame_dt`.
        last_frame_at: Option<Instant>,
        /// Counts redraws actually rendered.
        frame_index: u64,
        /// Physical canvas size used by the most recent full
        /// [`Runner::prepare`] call. The repaint dispatcher requires
        /// this to match the current `gfx.config` size before taking
        /// the paint-only path: the cached `DrawOp` list was laid out
        /// against this size, so a `ResizeObserver` fire that updated
        /// `gfx.config` since must force a fresh layout rather than
        /// painting stale geometry to the new viewport.
        last_prepared_size: Option<(u32, u32)>,
        /// Adapter backend tag, captured at adapter selection time.
        /// `Rc<RefCell>` because the surface is created in an async
        /// task that finishes after `Host::new`; the cell is read
        /// each frame in the RedrawRequested arm.
        backend: Rc<RefCell<&'static str>>,
        /// Held for its drop side-effects: the JS callback object
        /// that ResizeObserver fires. Dropping this disconnects the
        /// observer.
        _resize_closure: Option<Closure<dyn FnMut()>>,
        /// The observer itself; held alongside the closure so its
        /// JS-side observation outlives this frame.
        _resize_observer: Option<web_sys::ResizeObserver>,
    }

    struct Gfx {
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        renderer: Runner,
        msaa: MsaaTarget,
        /// Format used for render-target views and pipelines. May
        /// differ from `config.format` when we re-view a linear
        /// swapchain texture as sRGB (Chromium WebGPU path) — the
        /// swapchain stores `Rgba8Unorm`, but every view is
        /// `Rgba8UnormSrgb` so the hardware encodes on write.
        render_format: wgpu::TextureFormat,
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
                next_trigger: FrameTrigger::Initial,
                last_frame_at: None,
                frame_index: 0,
                last_prepared_size: None,
                backend: Rc::new(RefCell::new("?")),
                _resize_closure: None,
                _resize_observer: None,
            }
        }
    }

    fn backend_label(backend: wgpu::Backend) -> &'static str {
        match backend {
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Dx12 => "DX12",
            wgpu::Backend::Gl => "WebGL2",
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Noop => "noop",
        }
    }

    /// sRGB-tagged view-format sibling for a linear `*8Unorm` swapchain
    /// format. Used to recover gamma-correct output on Chromium's WebGPU
    /// surface: the swapchain offers only linear formats there, so we
    /// declare the sRGB form as a view format and render through that —
    /// hardware applies the sRGB encode on store and the compositor
    /// reads gamma-correct pixels. Returns `None` for formats that have
    /// no sRGB sibling (e.g. `Rgba16Float`, where the float storage is
    /// already linear-precision-correct), in which case the caller
    /// keeps the chosen format unchanged.
    fn srgb_view_of(format: wgpu::TextureFormat) -> Option<wgpu::TextureFormat> {
        use wgpu::TextureFormat as F;
        match format {
            F::Rgba8Unorm => Some(F::Rgba8UnormSrgb),
            F::Bgra8Unorm => Some(F::Bgra8UnormSrgb),
            _ => None,
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
            // canvas backing buffer in lockstep. The ResizeObserver
            // installed below carries the canvas through later layout
            // changes; we don't depend on winit dispatching `Resized`.
            let attrs = Window::default_attributes().with_canvas(Some(canvas.clone()));
            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

            // Force the canvas backing buffer to match the canvas's
            // CSS-laid-out size at the device pixel ratio. Without
            // this the canvas defaults to 300×150 device pixels, the
            // swapchain ends up tiny and stretched, and Firefox's
            // WebGPU backend fails the first present with "not enough
            // memory left" because the surface texture and the canvas
            // drawing buffer disagree. winit's `Window::inner_size()`
            // reads canvas.width/canvas.height on the web backend, so
            // setting them here is what the async surface setup picks
            // up for the initial swap-chain dimensions.
            let (initial_w, initial_h) = measure_canvas(&canvas);
            canvas.set_width(initial_w);
            canvas.set_height(initial_h);

            // Keep the canvas backing buffer tracking its CSS box
            // size for the lifetime of the page. ResizeObserver fires
            // once on observe() with the initial size, then again
            // every time the canvas's content rect changes. We bypass
            // winit's `request_inner_size` round-trip — its web
            // backend doesn't reliably translate that into a
            // `Resized` event, which left the swapchain stretched
            // mid-session — and reconfigure the surface directly via
            // `apply_canvas_size`. Until the async surface setup
            // completes we just keep canvas.width/height in sync so
            // the eventual `inner_size()` read picks up the latest.
            let canvas_for_observer = canvas.clone();
            let window_for_observer = window.clone();
            let gfx_for_observer = self.gfx.clone();
            let resize_closure: Closure<dyn FnMut()> = Closure::new(move || {
                let (phys_w, phys_h) = measure_canvas(&canvas_for_observer);
                let mut gfx_borrow = gfx_for_observer.borrow_mut();
                if let Some(gfx) = gfx_borrow.as_mut() {
                    apply_canvas_size(&canvas_for_observer, gfx, phys_w, phys_h);
                } else {
                    canvas_for_observer.set_width(phys_w);
                    canvas_for_observer.set_height(phys_h);
                }
                drop(gfx_borrow);
                window_for_observer.request_redraw();
            });
            let observer = web_sys::ResizeObserver::new(resize_closure.as_ref().unchecked_ref())
                .expect("ResizeObserver::new failed");
            observer.observe(&canvas);
            self._resize_closure = Some(resize_closure);
            self._resize_observer = Some(observer);

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
            let backend_slot = self.backend.clone();
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

                // Log the adapter we actually got. `Backends::BROWSER_WEBGPU
                // | Backends::GL` silently falls back to WebGL2 if the
                // browser's WebGPU init fails, and WebGL2 frames cost
                // an order of magnitude more GPU time than WebGPU on
                // the same scene — so this is the first thing to check
                // when investigating "why is it slow on the web".
                let info = adapter.get_info();
                log::info!(
                    "aetna-web: adapter selected — backend={:?} name={:?} driver={:?} device_type={:?}",
                    info.backend,
                    info.name,
                    info.driver,
                    info.device_type,
                );
                *backend_slot.borrow_mut() = backend_label(info.backend);

                // Per-sample MSAA shading is a downlevel cap. WebGL2
                // (GLES 3.0) and most browser WebGPU adapters don't
                // advertise it, and naga rejects shaders that use
                // `@interpolate(perspective, sample)` at module
                // creation when the cap is missing. Read the flag here
                // and pass it to `Runner::with_caps` so stock + custom
                // shaders downlevel cleanly on those backends.
                let downlevel = adapter.get_downlevel_capabilities();
                let per_sample_shading = downlevel
                    .flags
                    .contains(wgpu::DownlevelFlags::MULTISAMPLED_SHADING);
                if !per_sample_shading {
                    log::info!(
                        "aetna-web: adapter lacks DownlevelFlags::MULTISAMPLED_SHADING; \
                         shaders will downlevel `@interpolate(perspective, sample)` to per-pixel-centre interpolation"
                    );
                }

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
                // Decide the render-target view format. If the chosen
                // swapchain format is already sRGB-tagged (native, most
                // browsers' WebGL2 surfaces), this collapses to the
                // same format. Chromium's WebGPU surface offers only
                // linear formats — `Rgba8Unorm`, `Bgra8Unorm`,
                // `Rgba16Float` — so without this fix-up our shaders'
                // linear writes hit the compositor uncorrected and the
                // page renders 2.2-gamma's worth darker than native.
                // The trick: keep the swapchain format as `Rgba8Unorm`
                // (storage), declare `Rgba8UnormSrgb` as a view format,
                // and create every render-target view through that. The
                // hardware applies the sRGB encode on store. WebGPU
                // explicitly permits this view-format reinterpretation
                // because the two formats differ only in the sRGB flag.
                let render_format = srgb_view_of(format).unwrap_or(format);
                let view_formats = if render_format != format {
                    vec![render_format]
                } else {
                    Vec::new()
                };
                log::info!(
                    "aetna-web: surface format {:?} (sRGB? {}) → render view {:?}; offered {:?}",
                    format,
                    format.is_srgb(),
                    render_format,
                    surface_caps.formats,
                );
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
                // Prefer Fifo (vsync) so redraws can't outrun the
                // browser's compositor — same rationale as
                // aetna-winit-wgpu.
                let present_mode = if surface_caps
                    .present_modes
                    .contains(&wgpu::PresentMode::Fifo)
                {
                    wgpu::PresentMode::Fifo
                } else {
                    surface_caps.present_modes[0]
                };
                let config = wgpu::SurfaceConfiguration {
                    usage,
                    format,
                    width: inner.width.max(1),
                    height: inner.height.max(1),
                    present_mode,
                    alpha_mode: surface_caps.alpha_modes[0],
                    view_formats,
                    desired_maximum_frame_latency: 2,
                };
                surface.configure(&device, &config);

                let mut renderer = Runner::with_caps(
                    &device,
                    &queue,
                    render_format,
                    SAMPLE_COUNT,
                    per_sample_shading,
                );
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
                    renderer.register_shader_with(
                        &device,
                        s.name,
                        s.wgsl,
                        s.samples_backdrop,
                        s.samples_time,
                    );
                }

                let msaa = MsaaTarget::new(
                    &device,
                    render_format,
                    surface_extent(&config),
                    SAMPLE_COUNT,
                );
                *gfx_slot.borrow_mut() = Some(Gfx {
                    window: window_for_async.clone(),
                    surface,
                    device,
                    queue,
                    config,
                    renderer,
                    msaa,
                    render_format,
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
                            MsaaTarget::new(&gfx.device, gfx.render_format, extent, SAMPLE_COUNT);
                    }
                    self.next_trigger = FrameTrigger::Resize;
                    gfx.window.request_redraw();
                }

                WindowEvent::CursorMoved { position, .. } => {
                    let lx = position.x as f32 / scale;
                    let ly = position.y as f32 / scale;
                    self.last_pointer = Some((lx, ly));
                    let moved = gfx.renderer.pointer_moved(lx, ly);
                    for event in moved.events {
                        self.app.on_event(event);
                    }
                    if moved.needs_redraw {
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }
                }

                WindowEvent::CursorLeft { .. } => {
                    self.last_pointer = None;
                    for event in gfx.renderer.pointer_left() {
                        self.app.on_event(event);
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                // Browser drag/drop and clipboard-image plumbing rides
                // the HTML File API rather than winit (which doesn't
                // surface DroppedFile on wasm32). Web hosts that need
                // file-drop support listen for `dragenter` / `drop` on
                // the canvas via wasm-bindgen and route the resulting
                // bytes through their own paths. The winit event arms
                // exist for source-parity with the native hosts; on
                // web they currently won't fire.
                WindowEvent::HoveredFile(path) => {
                    let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                    for event in gfx.renderer.file_hovered(path, lx, ly) {
                        self.app.on_event(event);
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                WindowEvent::HoveredFileCancelled => {
                    for event in gfx.renderer.file_hover_cancelled() {
                        self.app.on_event(event);
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                WindowEvent::DroppedFile(path) => {
                    let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                    for event in gfx.renderer.file_dropped(path, lx, ly) {
                        self.app.on_event(event);
                    }
                    self.next_trigger = FrameTrigger::Pointer;
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
                            self.next_trigger = FrameTrigger::Pointer;
                            gfx.window.request_redraw();
                        }
                        ElementState::Released => {
                            for event in gfx.renderer.pointer_up(lx, ly, button) {
                                self.app.on_event(event);
                            }
                            self.next_trigger = FrameTrigger::Pointer;
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
                        self.next_trigger = FrameTrigger::Pointer;
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
                    self.next_trigger = FrameTrigger::Keyboard;
                    gfx.window.request_redraw();
                }
                WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                    if let Some(event) = gfx.renderer.text_input(text) {
                        self.app.on_event(event);
                    }
                    self.next_trigger = FrameTrigger::Keyboard;
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
                    // Render through the sRGB view format (see
                    // `srgb_view_of` and the surface configuration step
                    // for why). When the swapchain is already sRGB this
                    // collapses to the storage format and the view is
                    // identical to `..Default::default()`.
                    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(gfx.render_format),
                        ..Default::default()
                    });

                    let last_frame_dt = self
                        .last_frame_at
                        .map(|t| frame_start.duration_since(t))
                        .unwrap_or(std::time::Duration::ZERO);
                    self.last_frame_at = Some(frame_start);
                    let trigger = std::mem::take(&mut self.next_trigger);
                    let scale_factor = gfx.window.scale_factor() as f32;
                    let viewport_rect = Rect::new(
                        0.0,
                        0.0,
                        gfx.config.width as f32 / scale_factor,
                        gfx.config.height as f32 / scale_factor,
                    );
                    let current_size = (gfx.config.width, gfx.config.height);
                    // Paint-only path: a time-driven shader's deadline
                    // fired and nothing else has changed since the last
                    // full prepare — skip rebuild + layout and reuse the
                    // cached ops via `repaint`. The size guard catches
                    // ResizeObserver fires that updated `gfx.config`
                    // since the last prepare without setting a trigger.
                    let paint_only = trigger == FrameTrigger::ShaderPaint
                        && Some(current_size) == self.last_prepared_size;

                    let (prepare, palette, t_after_build, t_after_prepare) = if paint_only {
                        // No build pass: reuse the renderer's already-set
                        // theme palette and skip diagnostics / frame_index
                        // bump. Apps reading `cx.diagnostics()` see the
                        // overlay update only on layout frames, which is
                        // the documented contract for paint-only.
                        let palette = gfx.renderer.theme().palette().clone();
                        let t_after_build = Instant::now();
                        let prepare = gfx.renderer.repaint(
                            &gfx.device,
                            &gfx.queue,
                            viewport_rect,
                            scale_factor,
                        );
                        let t_after_prepare = Instant::now();
                        (prepare, palette, t_after_build, t_after_prepare)
                    } else {
                        self.frame_index = self.frame_index.wrapping_add(1);
                        let diagnostics = HostDiagnostics {
                            backend: *self.backend.borrow(),
                            surface_size: (gfx.config.width, gfx.config.height),
                            scale_factor,
                            msaa_samples: SAMPLE_COUNT,
                            frame_index: self.frame_index,
                            last_frame_dt,
                            trigger,
                        };
                        self.app.before_build();
                        let theme = self.app.theme();
                        let cx = BuildCx::new(&theme)
                            .with_ui_state(gfx.renderer.ui_state())
                            .with_diagnostics(&diagnostics);
                        let mut tree = self.app.build(&cx);
                        let palette = theme.palette().clone();
                        gfx.renderer.set_theme(theme);
                        gfx.renderer.set_hotkeys(self.app.hotkeys());
                        gfx.renderer.set_selection(self.app.selection());
                        gfx.renderer.push_toasts(self.app.drain_toasts());
                        gfx.renderer
                            .push_focus_requests(self.app.drain_focus_requests());
                        let t_after_build = Instant::now();
                        let prepare = gfx.renderer.prepare(
                            &gfx.device,
                            &gfx.queue,
                            &mut tree,
                            viewport_rect,
                            scale_factor,
                        );
                        let t_after_prepare = Instant::now();

                        // Cursor resolution depends on the laid-out tree
                        // and the hovered key derived from layout ids,
                        // so it only updates on the full-prepare path.
                        // Paint-only frames inherit the previous cursor.
                        let cursor = gfx.renderer.ui_state().cursor(&tree);
                        if cursor != self.last_cursor {
                            gfx.window.set_cursor(winit_cursor(cursor));
                            self.last_cursor = cursor;
                        }
                        self.last_prepared_size = Some(current_size);
                        (prepare, palette, t_after_build, t_after_prepare)
                    };

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
                        wgpu::LoadOp::Clear(bg_color(&palette)),
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

                    // Two-lane scheduling: a layout-driven signal
                    // (animation settling, widget redraw_within,
                    // tooltip / toast pending) takes precedence over a
                    // paint-only signal — both arrive immediately
                    // because the browser raf loop has no deadline
                    // parking, but the trigger encodes which path the
                    // next frame should take. On a paint-only frame
                    // `repaint` reports `next_layout_redraw_in = None`
                    // (it didn't re-evaluate), so the layout deadline
                    // can only fall through if the prior full prepare
                    // already cleared it.
                    if prepare.next_layout_redraw_in.is_some() {
                        self.next_trigger = FrameTrigger::Animation;
                        gfx.window.request_redraw();
                    } else if prepare.next_paint_redraw_in.is_some() {
                        self.next_trigger = FrameTrigger::ShaderPaint;
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

    fn bg_color(palette: &Palette) -> wgpu::Color {
        let c = palette.background;
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
