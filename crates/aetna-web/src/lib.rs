//! Browser host for Aetna wasm apps.
//!
//! Write normal UI code against `aetna_core::prelude::*`, then call
//! [`start_with`] from your wasm crate's `#[wasm_bindgen(start)]`
//! entry point. The host opens a wgpu surface against a canvas in the
//! page and drives the app through winit's browser event loop.
//!
//! The default configuration expects a `<canvas id="aetna_canvas">`.
//! Use [`start_with_config`] when embedding into a page with a different
//! canvas id.
//!
//! `aetna-winit-wgpu` is the equivalent reusable native host.

use aetna_core::Rect;

/// Default canvas element id used by [`WebHostConfig::default`].
pub const DEFAULT_CANVAS_ID: &str = "aetna_canvas";

/// Default logical viewport. Sized to feel reasonable both as a winit
/// window and as a browser canvas. Browsers can override this by
/// resizing the canvas; the runner reacts to `winit::Resized`.
pub const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 900.0,
    h: 640.0,
};

/// Browser host configuration.
#[derive(Clone, Debug)]
pub struct WebHostConfig {
    /// Fallback logical viewport used when the canvas has no CSS size
    /// yet. Once the page lays the canvas out, the host tracks its CSS
    /// box through `ResizeObserver`.
    pub viewport: Rect,
    /// Id of the canvas element the host should attach to.
    pub canvas_id: String,
}

impl WebHostConfig {
    pub fn new(viewport: Rect) -> Self {
        Self {
            viewport,
            canvas_id: DEFAULT_CANVAS_ID.to_string(),
        }
    }

    pub fn with_canvas_id(mut self, canvas_id: impl Into<String>) -> Self {
        self.canvas_id = canvas_id.into();
        self
    }
}

impl Default for WebHostConfig {
    fn default() -> Self {
        Self::new(VIEWPORT)
    }
}

#[cfg(target_arch = "wasm32")]
pub use web_entry::{WebHandle, start_with, start_with_config};

#[cfg(not(target_arch = "wasm32"))]
pub use native_stub::{WebHandle, start_with, start_with_config};

#[cfg(not(target_arch = "wasm32"))]
mod native_stub {
    use aetna_core::{App, Rect};

    use super::WebHostConfig;

    /// Browser redraw handle.
    ///
    /// On non-wasm targets this is a no-op placeholder so host crates
    /// can type-check shared code. It is only functional on
    /// `wasm32-unknown-unknown`.
    #[derive(Clone, Debug, Default)]
    pub struct WebHandle {
        _private: (),
    }

    impl WebHandle {
        pub fn request_redraw(&self) {}
    }

    pub fn start_with<A: App + 'static>(_viewport: Rect, _app: A) -> WebHandle {
        panic!("aetna-web can only start apps on wasm32-unknown-unknown")
    }

    pub fn start_with_config<A: App + 'static>(_config: WebHostConfig, _app: A) -> WebHandle {
        panic!("aetna-web can only start apps on wasm32-unknown-unknown")
    }
}

// ---- Wasm host ----
//
// Lives in its own module so it can pull in wasm-only deps without
// polluting native builds.

#[cfg(target_arch = "wasm32")]
mod web_entry {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Arc;

    use aetna_core::{
        App, BuildCx, Cursor, FrameTrigger, HostDiagnostics, KeyModifiers, Palette, PointerButton,
        Rect, UiEvent, UiEventKind, UiKey, clipboard,
        widgets::text_input::{self, ClipboardKind},
    };
    use aetna_wgpu::{PrepareTimings, Runner};

    // MSAA is off on the browser. The WebGL2 path doesn't advertise
    // `MULTISAMPLED_SHADING`, so MSAA gives nothing to the SDF stock
    // surfaces (they do their own analytic AA in the fragment shader);
    // it would only have improved vector-icon polygon-edge AA. With it
    // on, Firefox + Mesa's implicit MSAA resolve was mis-syncing
    // partial regions of the swapchain — the sidebar would freeze at
    // its previous pixels until something forced a tree reshape. WebGPU
    // (Chromium) was unaffected but we use the same value for both
    // browser backends to keep one code path. Revisit once the WebGL2
    // resolve issue is understood (or once WebGPU is the only target).
    const SAMPLE_COUNT: u32 = 1;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;
    use web_time::Instant;
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
    use winit::window::{CursorIcon, Window, WindowId};

    use super::WebHostConfig;

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

    /// Handle returned by [`start_with`] so embedding code can wake the
    /// host after external browser events enqueue app work.
    #[derive(Clone)]
    pub struct WebHandle {
        inner: Rc<WebHandleInner>,
    }

    struct WebHandleInner {
        window: RefCell<Option<Arc<Window>>>,
        ready: Cell<bool>,
        pending_redraw: Cell<bool>,
    }

    impl WebHandle {
        fn new() -> Self {
            Self {
                inner: Rc::new(WebHandleInner {
                    window: RefCell::new(None),
                    ready: Cell::new(false),
                    pending_redraw: Cell::new(false),
                }),
            }
        }

        /// Request a redraw from external browser integration code.
        ///
        /// If the browser window or GPU setup is not ready yet, the
        /// request is remembered and flushed once setup completes.
        pub fn request_redraw(&self) {
            if self.inner.ready.get()
                && let Some(window) = self.inner.window.borrow().as_ref()
            {
                window.request_redraw();
                return;
            }
            self.inner.pending_redraw.set(true);
        }

        fn set_window(&self, window: Arc<Window>) {
            *self.inner.window.borrow_mut() = Some(window);
        }

        fn mark_ready(&self) -> bool {
            self.inner.ready.set(true);
            self.inner.pending_redraw.replace(false)
        }
    }

    /// Start an Aetna app in the browser using the default canvas id.
    ///
    /// Call this from the downstream crate's own
    /// `#[wasm_bindgen(start)]` function.
    pub fn start_with<A: App + 'static>(viewport: Rect, app: A) -> WebHandle {
        start_with_config(WebHostConfig::new(viewport), app)
    }

    /// Start an Aetna app in the browser with explicit host config.
    ///
    /// The function spawns winit's web event loop and returns
    /// immediately. Keep the returned [`WebHandle`] anywhere external
    /// JS callbacks need to wake Aetna after pushing work into
    /// app-owned shared state.
    pub fn start_with_config<A: App + 'static>(config: WebHostConfig, app: A) -> WebHandle {
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
        let handle = WebHandle::new();
        let host = Host::new(config, app, handle.clone());
        // spawn_app hands control to the browser. Native uses
        // run_app(...) which blocks; on wasm32 the event loop is
        // driven by the browser's animation-frame callbacks.
        event_loop.spawn_app(host);
        handle
    }

    /// Open a URL surfaced by `App::drain_link_opens` in a new tab.
    /// `_blank` matches what users expect for a click on an external
    /// link in app UI; `noopener` severs the `window.opener` reference
    /// so the opened page can't reverse-control this one. Failures are
    /// logged rather than panicking — popup blockers and CSP rules can
    /// reject the open and the showcase shouldn't crash because the
    /// browser said no.
    fn open_link(url: &str) {
        let Some(window) = web_sys::window() else {
            log::warn!("aetna-web: no window; dropping link open for {url}");
            return;
        };
        if let Err(err) = window.open_with_url_and_target_and_features(url, "_blank", "noopener") {
            log::warn!("aetna-web: window.open({url}) failed: {err:?}");
        }
    }

    /// Locate the configured canvas element in the host page.
    fn locate_canvas(canvas_id: &str) -> web_sys::HtmlCanvasElement {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        document
            .get_element_by_id(canvas_id)
            .unwrap_or_else(|| panic!("missing #{canvas_id} canvas element"))
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap_or_else(|_| panic!("#{canvas_id} is not a canvas"))
    }

    /// Read the canvas's CSS-laid-out box at the device pixel ratio.
    /// Returned size is what the swapchain backing buffer should match;
    /// callers pass it to `apply_canvas_size` to actually reconfigure
    /// the surface.
    fn measure_canvas(canvas: &web_sys::HtmlCanvasElement, fallback: Rect) -> (u32, u32) {
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(1.0);
        let css_w = if canvas.client_width() > 0 {
            canvas.client_width() as f64
        } else {
            fallback.w.max(1.0) as f64
        };
        let css_h = if canvas.client_height() > 0 {
            canvas.client_height() as f64
        } else {
            fallback.h.max(1.0) as f64
        };
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
        if let Some(msaa) = gfx.msaa.as_mut() {
            let extent = surface_extent(&gfx.config);
            if !msaa.matches(extent) {
                *msaa = aetna_wgpu::MsaaTarget::new(
                    &gfx.device,
                    gfx.render_format,
                    extent,
                    SAMPLE_COUNT,
                );
            }
        }
    }

    /// Mirrors the native winit + wgpu host shape, but with browser
    /// surface init (async via wasm-bindgen-futures rather than
    /// pollster). Kept inline here so `aetna-winit-wgpu` stays free of
    /// wasm-only deps.
    struct Host<A: App> {
        config: WebHostConfig,
        app: A,
        handle: WebHandle,
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
        /// Timing breakdown from the last completed rendered frame.
        last_build: Duration,
        last_prepare: Duration,
        last_layout: Duration,
        last_layout_intrinsic_cache_hits: u64,
        last_layout_intrinsic_cache_misses: u64,
        last_layout_pruned_subtrees: u64,
        last_layout_pruned_nodes: u64,
        last_draw_ops: Duration,
        last_draw_ops_culled_text_ops: u64,
        last_paint: Duration,
        last_paint_culled_ops: u64,
        last_gpu_upload: Duration,
        last_snapshot: Duration,
        last_submit: Duration,
        last_text_layout_cache_hits: u64,
        last_text_layout_cache_misses: u64,
        last_text_layout_cache_evictions: u64,
        last_text_layout_shaped_bytes: u64,
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
        /// Browser `paste` events carry trusted clipboard text without
        /// the Firefox permission menu used by `navigator.clipboard.readText`.
        /// The callback enqueues text here, then requests a redraw; the
        /// RedrawRequested arm converts it into a focused Aetna `TextInput`.
        pending_clipboard_text: Rc<RefCell<VecDeque<String>>>,
        /// Web browsers do not expose the X11/Wayland primary-selection
        /// clipboard. Keep an app-local approximation so Aetna selection
        /// highlight can still feed middle-click paste inside the canvas.
        primary_selection: String,
        /// Held for its drop side-effects: the JS paste callback object.
        _paste_closure: Option<Closure<dyn FnMut(web_sys::ClipboardEvent)>>,
        /// Held for its drop side-effects: the JS keydown callback object.
        _keydown_closure: Option<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
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
        /// `None` when [`SAMPLE_COUNT`] is 1 — the renderer draws
        /// straight into the swapchain texture and there's no resolve
        /// pass. `Some` when MSAA is enabled, holding the
        /// multisampled colour attachment that the swapchain texture
        /// is the resolve target for.
        msaa: Option<aetna_wgpu::MsaaTarget>,
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
        fn new(config: WebHostConfig, app: A, handle: WebHandle) -> Self {
            Self {
                config,
                app,
                handle,
                gfx: Rc::new(RefCell::new(None)),
                last_pointer: None,
                modifiers: KeyModifiers::default(),
                stats: FrameStats::default(),
                last_cursor: Cursor::Default,
                next_trigger: FrameTrigger::Initial,
                last_frame_at: None,
                frame_index: 0,
                last_build: Duration::ZERO,
                last_prepare: Duration::ZERO,
                last_layout: Duration::ZERO,
                last_layout_intrinsic_cache_hits: 0,
                last_layout_intrinsic_cache_misses: 0,
                last_layout_pruned_subtrees: 0,
                last_layout_pruned_nodes: 0,
                last_draw_ops: Duration::ZERO,
                last_draw_ops_culled_text_ops: 0,
                last_paint: Duration::ZERO,
                last_paint_culled_ops: 0,
                last_gpu_upload: Duration::ZERO,
                last_snapshot: Duration::ZERO,
                last_submit: Duration::ZERO,
                last_text_layout_cache_hits: 0,
                last_text_layout_cache_misses: 0,
                last_text_layout_cache_evictions: 0,
                last_text_layout_shaped_bytes: 0,
                last_prepared_size: None,
                backend: Rc::new(RefCell::new("?")),
                pending_clipboard_text: Rc::new(RefCell::new(VecDeque::new())),
                primary_selection: String::new(),
                _paste_closure: None,
                _keydown_closure: None,
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
            let canvas = locate_canvas(&self.config.canvas_id);

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
            let attrs = Window::default_attributes()
                .with_canvas(Some(canvas.clone()))
                // Browser paste, including Linux middle-click primary
                // paste, is delivered as a DOM ClipboardEvent. winit's
                // default web preventDefault path suppresses those
                // browser-side events, so Aetna handles clipboard
                // suppression at the document paste listener instead.
                .with_prevent_default(false);
            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
            self.handle.set_window(window.clone());

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
            let viewport = self.config.viewport;
            let (initial_w, initial_h) = measure_canvas(&canvas, viewport);
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
                let (phys_w, phys_h) = measure_canvas(&canvas_for_observer, viewport);
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

            let pending_clipboard_text = self.pending_clipboard_text.clone();
            let window_for_paste = window.clone();
            let paste_closure: Closure<dyn FnMut(web_sys::ClipboardEvent)> =
                Closure::new(move |event: web_sys::ClipboardEvent| {
                    let Some(data) = event.clipboard_data() else {
                        log::warn!("aetna-web: paste event had no clipboardData");
                        return;
                    };
                    let Ok(text) = data.get_data("text/plain") else {
                        log::warn!("aetna-web: paste event could not read text/plain");
                        return;
                    };
                    if text.is_empty() {
                        return;
                    }
                    event.prevent_default();
                    event.stop_propagation();
                    pending_clipboard_text.borrow_mut().push_back(text);
                    window_for_paste.request_redraw();
                });
            canvas
                .owner_document()
                .expect("canvas has no owner document")
                .add_event_listener_with_callback("paste", paste_closure.as_ref().unchecked_ref())
                .expect("add paste listener");
            self._paste_closure = Some(paste_closure);

            let keydown_closure: Closure<dyn FnMut(web_sys::KeyboardEvent)> =
                Closure::new(move |event: web_sys::KeyboardEvent| {
                    if should_prevent_browser_key_default(&event) {
                        event.prevent_default();
                    }
                });
            canvas
                .add_event_listener_with_callback(
                    "keydown",
                    keydown_closure.as_ref().unchecked_ref(),
                )
                .expect("add keydown listener");
            self._keydown_closure = Some(keydown_closure);

            // Allow both browser backends. wgpu's synchronous
            // Instance::new() can't safely decide this: if
            // `navigator.gpu` exists, it routes the whole instance
            // through WebGPU, even on browsers/GPUs where
            // requestAdapter() later returns null. The async helper
            // probes adapter creation first and removes WebGPU from the
            // descriptor when it is not really usable, letting WebGL2
            // handle Chrome/Linux-style partial support instead of
            // panicking during adapter selection.
            //
            // WebGPU is required for backdrop-sampling shaders
            // (`liquid_glass`) because WebGL2 surfaces don't advertise
            // `COPY_SRC` on the swapchain texture, so the snapshot copy
            // can't run — we register backdrop shaders only when the
            // chosen adapter's surface supports COPY_SRC, which in
            // practice means "WebGPU was selected."
            //
            // Firefox: as of 2026-05, Firefox's WebGPU implementation
            // still wedges its compositor on pointer events with our
            // atlas-uploading path (whole canvas goes black until the
            // cursor leaves). The workaround on the user side is to
            // disable WebGPU in `about:config` (`dom.webgpu.enabled =
            // false`); wgpu then transparently picks WebGL2 here and
            // backdrop shaders are skipped via the COPY_SRC check
            // below. Revisit when Firefox WebGPU stabilises.
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
            let shaders = self.app.shaders();
            let theme = self.app.theme();
            let gfx_slot = self.gfx.clone();
            let backend_slot = self.backend.clone();
            let window_for_async = window.clone();
            let handle_for_async = self.handle.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
                instance_desc.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
                let instance = wgpu::util::new_instance_with_webgpu_detection(instance_desc).await;
                let surface = instance
                    .create_surface(window_for_async.clone())
                    .expect("create surface");

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
                // support it, and naga rejects shaders that use
                // `@interpolate(perspective, sample)` at module
                // creation when the cap is missing. Read the flag here
                // and pass it to `Runner::with_caps` so stock + custom
                // shaders downlevel cleanly on those backends.
                //
                // Chrome's SwiftShader WebGL2 fallback currently reports
                // `MULTISAMPLED_SHADING` through wgpu, but the GLSL ES
                // target still rejects the sample interpolation qualifier.
                // Treat WebGL2 as unsupported regardless of the reported
                // flag; WebGPU/native can keep trusting the adapter cap.
                let downlevel = adapter.get_downlevel_capabilities();
                let per_sample_shading = info.backend != wgpu::Backend::Gl
                    && downlevel
                        .flags
                        .contains(wgpu::DownlevelFlags::MULTISAMPLED_SHADING);
                if !per_sample_shading {
                    log::info!(
                        "aetna-web: per-sample shading unavailable on selected backend; \
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

                // MSAA target only when SAMPLE_COUNT > 1; the
                // single-sample path renders straight into the
                // swapchain texture.
                let msaa = if SAMPLE_COUNT > 1 {
                    Some(aetna_wgpu::MsaaTarget::new(
                        &device,
                        render_format,
                        surface_extent(&config),
                        SAMPLE_COUNT,
                    ))
                } else {
                    None
                };
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
                if handle_for_async.mark_ready() {
                    log::debug!("aetna-web: flushing pending external redraw request");
                }
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
                    if let Some(msaa) = gfx.msaa.as_mut() {
                        let extent = surface_extent(&gfx.config);
                        if !msaa.matches(extent) {
                            *msaa = aetna_wgpu::MsaaTarget::new(
                                &gfx.device,
                                gfx.render_format,
                                extent,
                                SAMPLE_COUNT,
                            );
                        }
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
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
                    }
                    if moved.needs_redraw {
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }
                }

                WindowEvent::CursorLeft { .. } => {
                    self.last_pointer = None;
                    for event in gfx.renderer.pointer_left() {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
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
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                WindowEvent::HoveredFileCancelled => {
                    for event in gfx.renderer.file_hover_cancelled() {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                WindowEvent::DroppedFile(path) => {
                    let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                    for event in gfx.renderer.file_dropped(path, lx, ly) {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
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
                                dispatch_app_event(
                                    &mut self.app,
                                    event,
                                    gfx.renderer.ui_state(),
                                    &mut self.primary_selection,
                                );
                            }
                            self.next_trigger = FrameTrigger::Pointer;
                            gfx.window.request_redraw();
                        }
                        ElementState::Released => {
                            for event in gfx.renderer.pointer_up(lx, ly, button) {
                                let event =
                                    attach_primary_selection_text(event, &self.primary_selection);
                                dispatch_app_event(
                                    &mut self.app,
                                    event,
                                    gfx.renderer.ui_state(),
                                    &mut self.primary_selection,
                                );
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
                            match text_input::clipboard_request(&event) {
                                Some(ClipboardKind::Copy) => {
                                    copy_current_selection(
                                        &self.app,
                                        gfx.renderer.ui_state(),
                                        write_clipboard_text,
                                    );
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx.renderer.ui_state(),
                                        &mut self.primary_selection,
                                    );
                                }
                                Some(ClipboardKind::Cut) => {
                                    copy_current_selection(
                                        &self.app,
                                        gfx.renderer.ui_state(),
                                        write_clipboard_text,
                                    );
                                    dispatch_app_event(
                                        &mut self.app,
                                        clipboard::delete_selection_event(event),
                                        gfx.renderer.ui_state(),
                                        &mut self.primary_selection,
                                    );
                                }
                                Some(ClipboardKind::Paste) => {}
                                None => dispatch_app_event(
                                    &mut self.app,
                                    event,
                                    gfx.renderer.ui_state(),
                                    &mut self.primary_selection,
                                ),
                            }
                        }
                    }
                    if let Some(text) = &key_event.text
                        && let Some(event) = gfx.renderer.text_input(text.to_string())
                    {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Keyboard;
                    gfx.window.request_redraw();
                }
                WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                    if let Some(event) = gfx.renderer.text_input(text) {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            gfx.renderer.ui_state(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Keyboard;
                    gfx.window.request_redraw();
                }

                WindowEvent::RedrawRequested => {
                    let frame_start = Instant::now();
                    let clipboard_drained = drain_pending_clipboard_text(
                        &mut self.app,
                        &mut gfx.renderer,
                        &self.pending_clipboard_text,
                        &mut self.primary_selection,
                    );
                    if clipboard_drained {
                        self.next_trigger = FrameTrigger::Keyboard;
                    }
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
                            last_build: self.last_build,
                            last_prepare: self.last_prepare,
                            last_layout: self.last_layout,
                            last_layout_intrinsic_cache_hits: self.last_layout_intrinsic_cache_hits,
                            last_layout_intrinsic_cache_misses: self
                                .last_layout_intrinsic_cache_misses,
                            last_layout_pruned_subtrees: self.last_layout_pruned_subtrees,
                            last_layout_pruned_nodes: self.last_layout_pruned_nodes,
                            last_draw_ops: self.last_draw_ops,
                            last_draw_ops_culled_text_ops: self.last_draw_ops_culled_text_ops,
                            last_paint: self.last_paint,
                            last_paint_culled_ops: self.last_paint_culled_ops,
                            last_gpu_upload: self.last_gpu_upload,
                            last_snapshot: self.last_snapshot,
                            last_submit: self.last_submit,
                            last_text_layout_cache_hits: self.last_text_layout_cache_hits,
                            last_text_layout_cache_misses: self.last_text_layout_cache_misses,
                            last_text_layout_cache_evictions: self.last_text_layout_cache_evictions,
                            last_text_layout_shaped_bytes: self.last_text_layout_shaped_bytes,
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
                        gfx.renderer
                            .push_scroll_requests(self.app.drain_scroll_requests());
                        for url in self.app.drain_link_opens() {
                            open_link(&url);
                        }
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
                        gfx.msaa.as_ref().map(|m| &m.view),
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
                    self.last_build = t_after_build - frame_start;
                    self.last_prepare = t_after_prepare - t_after_build;
                    self.last_submit = t_after_submit - t_after_prepare;
                    self.last_layout = prepare.timings.layout;
                    self.last_layout_intrinsic_cache_hits =
                        prepare.timings.layout_intrinsic_cache.hits;
                    self.last_layout_intrinsic_cache_misses =
                        prepare.timings.layout_intrinsic_cache.misses;
                    self.last_layout_pruned_subtrees = prepare.timings.layout_prune.subtrees;
                    self.last_layout_pruned_nodes = prepare.timings.layout_prune.nodes;
                    self.last_draw_ops = prepare.timings.draw_ops;
                    self.last_draw_ops_culled_text_ops = prepare.timings.draw_ops_culled_text_ops;
                    self.last_paint = prepare.timings.paint;
                    self.last_paint_culled_ops = prepare.timings.paint_culled_ops;
                    self.last_gpu_upload = prepare.timings.gpu_upload;
                    self.last_snapshot = prepare.timings.snapshot;
                    self.last_text_layout_cache_hits = prepare.timings.text_layout_cache.hits;
                    self.last_text_layout_cache_misses = prepare.timings.text_layout_cache.misses;
                    self.last_text_layout_cache_evictions =
                        prepare.timings.text_layout_cache.evictions;
                    self.last_text_layout_shaped_bytes =
                        prepare.timings.text_layout_cache.shaped_bytes;

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
                    let _ = self.config.viewport;
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

    fn should_prevent_browser_key_default(event: &web_sys::KeyboardEvent) -> bool {
        // Keep browser/system shortcuts alive, especially Ctrl/Cmd+V:
        // preventing that keydown suppresses the trusted DOM `paste`
        // event that carries clipboard text in Firefox.
        if event.ctrl_key() || event.meta_key() || event.alt_key() {
            return false;
        }

        let key = event.key();
        if key.chars().count() == 1 {
            return true;
        }

        matches!(
            key.as_str(),
            "ArrowUp"
                | "ArrowDown"
                | "ArrowLeft"
                | "ArrowRight"
                | "Backspace"
                | "Delete"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
                | "Tab"
                | "Enter"
                | "Escape"
        )
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

    fn copy_current_selection<A: App>(
        app: &A,
        ui_state: &aetna_core::state::UiState,
        write_text: impl FnOnce(String),
    ) {
        let Some(text) = clipboard::selected_text_for_app(app, ui_state) else {
            return;
        };
        write_text(text);
    }

    fn write_clipboard_text(text: String) {
        let Some(window) = web_sys::window() else {
            log::warn!("aetna-web: no window; clipboard write dropped");
            return;
        };
        let promise = window.navigator().clipboard().write_text(&text);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = wasm_bindgen_futures::JsFuture::from(promise).await {
                log::warn!("aetna-web: clipboard writeText failed: {err:?}");
            }
        });
    }

    fn attach_primary_selection_text(mut event: UiEvent, primary_selection: &str) -> UiEvent {
        if event.kind == UiEventKind::MiddleClick && !primary_selection.is_empty() {
            event.text = Some(primary_selection.to_string());
        }
        event
    }

    fn dispatch_app_event<A: App>(
        app: &mut A,
        event: UiEvent,
        ui_state: &aetna_core::state::UiState,
        primary_selection: &mut String,
    ) {
        let before = app.selection();
        app.on_event(event);
        if app.selection() != before {
            *primary_selection = clipboard::selected_text_for_app(app, ui_state)
                .filter(|text| !text.is_empty())
                .unwrap_or_default();
        }
    }

    fn drain_pending_clipboard_text<A: App>(
        app: &mut A,
        renderer: &mut Runner,
        pending_text: &Rc<RefCell<VecDeque<String>>>,
        primary_selection: &mut String,
    ) -> bool {
        let mut drained = false;
        while let Some(text) = pending_text.borrow_mut().pop_front() {
            let Some(event) = renderer.text_input(text.clone()) else {
                continue;
            };
            drained = true;
            let event = clipboard::paste_text_event(event, text);
            dispatch_app_event(app, event, renderer.ui_state(), primary_selection);
        }
        drained
    }
}
