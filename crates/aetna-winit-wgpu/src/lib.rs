//! Optional desktop host for running [`App`]s against a real `wgpu`
//! surface in a `winit` window.
//!
//! Most native apps should use this crate instead of calling
//! `aetna-wgpu` directly:
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
//!     aetna_winit_wgpu::run("My Aetna App", viewport, MyApp::default())
//! }
//! ```
//!
//! The host owns the event loop, window, device/queue, surface
//! configuration, render pass boundaries, input mapping, IME forwarding,
//! and animation redraw cadence. Your code owns the [`App`]: application
//! state, [`App::build`], [`App::on_event`], optional hotkeys, custom
//! shaders, and theme.
//!
//! [`run`] takes an [`App`] and runs an event loop that:
//!
//! - Calls [`App::build`] on every redraw, applying current hover/press
//!   visuals automatically before paint.
//! - Routes `winit` pointer events through the renderer's hit-tester
//!   and dispatches events back via [`App::on_event`].
//! - Routes Tab/Shift-Tab through focus traversal and Enter/Space/Escape
//!   through keyboard events.
//! - Requests a redraw whenever interaction state changes (mouse move,
//!   button down/up) so hover/press visuals are immediate.
//!
//! Use [`run_with_config`] when a simple app needs a fixed redraw
//! cadence for external live state such as meters. Put per-frame state
//! refresh in [`App::before_build`]. For fully custom render-loop
//! integration, bypass this crate and call `aetna_wgpu::Runner`
//! directly.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use aetna_core::{App, Cursor, KeyModifiers, PointerButton, Rect, UiKey};
use aetna_wgpu::{MsaaTarget, Runner};

const DEFAULT_SAMPLE_COUNT: u32 = 4;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// Configuration for the optional native winit + wgpu host.
#[derive(Clone, Copy, Debug)]
pub struct HostConfig {
    /// MSAA sample count used for Aetna's SDF surfaces. The default is
    /// 4, matching the demo and validation app paths.
    pub sample_count: u32,
    /// Optional fixed redraw cadence for apps with external live data
    /// sources such as audio meters. Animation-driven redraws still
    /// come from `Runner::prepare().needs_redraw`; this is only for
    /// host-owned clocks.
    pub redraw_interval: Option<Duration>,
    /// Prefer the lowest-latency wgpu present mode the surface
    /// advertises (`Mailbox`, falling back to `Fifo`). Default is
    /// `Fifo`, which is vsync-locked and conservative on power.
    ///
    /// Why this exists: with `Fifo`, every submit queues a frame for
    /// the next vsync; if the app submits faster than the display
    /// refresh, the compositor pulls the *oldest* queued frame at
    /// each vsync. On Wayland/Mesa during an interactive resize this
    /// shows up as the window content trailing the cursor in slow
    /// motion — by the time the latest size we rendered reaches the
    /// screen, several more compositor `configure` events have
    /// arrived. `Mailbox` replaces the pending frame on each submit,
    /// so the next vsync always shows the most recent render.
    ///
    /// Cost: with `Mailbox`, render cadence is no longer naturally
    /// vsync-bounded — an animation that calls `request_redraw` from
    /// `prepare.needs_redraw` will render at GPU speed. Pair this
    /// with `redraw_interval` (or accept the cycles) if that's not
    /// what you want.
    pub low_latency_present: bool,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            sample_count: DEFAULT_SAMPLE_COUNT,
            redraw_interval: None,
            low_latency_present: false,
        }
    }
}

impl HostConfig {
    pub fn with_redraw_interval(mut self, interval: Duration) -> Self {
        self.redraw_interval = Some(interval);
        self
    }

    pub fn with_sample_count(mut self, sample_count: u32) -> Self {
        self.sample_count = sample_count.max(1);
        self
    }

    pub fn with_low_latency_present(mut self, low_latency_present: bool) -> Self {
        self.low_latency_present = low_latency_present;
        self
    }
}

/// Compatibility extension point for apps that use this host crate.
///
/// New apps should prefer [`App::before_build`]. This trait remains for
/// code that wants to name a winit-host-specific app type while still
/// using the same core lifecycle.
pub trait WinitWgpuApp: App {
    fn before_build(&mut self) {
        App::before_build(self);
    }
}

struct BasicApp<A>(A);

impl<A: App> App for BasicApp<A> {
    fn before_build(&mut self) {
        self.0.before_build();
    }

    fn build(&self, cx: &aetna_core::BuildCx) -> aetna_core::El {
        self.0.build(cx)
    }

    fn on_event(&mut self, event: aetna_core::UiEvent) {
        self.0.on_event(event);
    }

    fn hotkeys(&self) -> Vec<(aetna_core::KeyChord, String)> {
        self.0.hotkeys()
    }

    fn drain_toasts(&mut self) -> Vec<aetna_core::toast::ToastSpec> {
        self.0.drain_toasts()
    }

    fn shaders(&self) -> Vec<aetna_core::AppShader> {
        self.0.shaders()
    }

    fn theme(&self) -> aetna_core::Theme {
        self.0.theme()
    }

    fn selection(&self) -> aetna_core::Selection {
        self.0.selection()
    }
}

impl<A: App> WinitWgpuApp for BasicApp<A> {}

/// Run a windowed app. Blocks until the user closes the window.
///
/// The `App` is owned by the runner; its `&mut self` is updated in
/// response to routed events and read on every `build` call.
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, BasicApp(app), HostConfig::default())
}

/// Run a windowed app with host-specific configuration.
///
/// Use this when a plain [`App`] wants a host cadence
/// (`redraw_interval`) or non-default MSAA. For fully custom
/// render-loop integration, bypass this crate and call
/// `aetna_wgpu::Runner` directly.
pub fn run_with_config<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, BasicApp(app), config)
}

/// Run a windowed app with host-specific configuration.
///
/// Prefer [`run_with_config`] for new apps; [`App::before_build`] is
/// available there as well.
pub fn run_host_app_with_config<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, app, config)
}

/// Run a windowed app with default host configuration.
///
/// Prefer [`run`] for new apps; [`App::before_build`] is available
/// there as well.
pub fn run_host_app<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, app, HostConfig::default())
}

fn run_host<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut host = Host {
        title,
        viewport,
        config,
        app,
        gfx: None,
        last_pointer: None,
        modifiers: KeyModifiers::default(),
        next_periodic_redraw: None,
        last_cursor: Cursor::Default,
        pending_resize: None,
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

struct Host<A: WinitWgpuApp> {
    title: &'static str,
    viewport: Rect,
    config: HostConfig,
    app: A,
    gfx: Option<Gfx>,
    /// Last pointer position in logical pixels (winit reports physical;
    /// we divide by the window's scale factor before storing).
    last_pointer: Option<(f32, f32)>,
    modifiers: KeyModifiers,
    next_periodic_redraw: Option<Instant>,
    /// Last cursor pushed to `Window::set_cursor`. Avoids redundant
    /// per-frame calls when the resolved cursor hasn't changed —
    /// `set_cursor` is cheap but goes through a syscall on most
    /// platforms.
    last_cursor: Cursor,
    /// Latest size from `WindowEvent::Resized` not yet applied to the
    /// surface. Compositors (Wayland especially) deliver a burst of
    /// resize events during an interactive drag; coalescing them so
    /// `surface.configure()` + MSAA realloc run once per frame
    /// instead of once per event keeps the window content from
    /// trailing the cursor.
    pending_resize: Option<PhysicalSize<u32>>,
}

struct Gfx {
    // Fields drop in declaration order. GPU resources must go before
    // the device/window they were created from so shutdown tears them
    // down before their owners disappear.
    renderer: Runner,
    surface: wgpu::Surface<'static>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    /// Multisampled color attachment for the surface frame, kept in
    /// sync with `config.width`/`config.height` and reallocated on
    /// resize. The surface frame texture is the resolve target.
    msaa: Option<MsaaTarget>,
}

fn surface_extent(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    }
}

impl<A: WinitWgpuApp> ApplicationHandler for Host<A> {
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

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no compatible adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aetna_winit_wgpu::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("request_device");

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        // Pick a present mode. `Fifo` is the conservative default —
        // mandatory in the wgpu spec, vsync-locked, predictable power
        // cost. `low_latency_present` opts into `Mailbox` (with `Fifo`
        // fallback) for apps where interaction latency matters more
        // than steady-state throughput; see `HostConfig` for the
        // rationale and trade-offs.
        //
        // `AETNA_PRESENT_MODE=mailbox|immediate|fifo` overrides at
        // runtime — useful for diagnosing without a recompile.
        let mode_override = std::env::var("AETNA_PRESENT_MODE").ok();
        let prefer_mailbox =
            self.config.low_latency_present || mode_override.as_deref() == Some("mailbox");
        let prefer_immediate = mode_override.as_deref() == Some("immediate");
        let prefer_fifo = mode_override.as_deref() == Some("fifo");
        let present_mode = if prefer_immediate
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else if prefer_mailbox
            && !prefer_fifo
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            surface_caps.present_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            // COPY_SRC is required so backdrop-sampling shaders can
            // copy the post-Pass-A surface into the runner's snapshot
            // texture mid-frame. Cost is minimal — most surfaces
            // already advertise it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // Keep the in-flight queue shallow. With `Fifo` this is a
            // hint that Mesa's WSI does not always honor — measured
            // resize lag on Wayland was unaffected by changing this
            // alone — but it's still the right default: an
            // interactive UI gains nothing from buffering more than
            // one frame ahead. Combined with `low_latency_present`
            // (Mailbox), interactive cadence is bounded by render
            // time, not by drained queue depth.
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let sample_count = self.config.sample_count.max(1);
        let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
        renderer.set_theme(self.app.theme());
        renderer.set_surface_size(config.width, config.height);
        // Register any custom shaders the app declared. Done once at
        // startup; pipelines are cached for the runner's lifetime.
        for s in self.app.shaders() {
            renderer.register_shader_with(
                &device,
                s.name,
                s.wgsl,
                s.samples_backdrop,
                s.samples_time,
            );
        }

        let msaa = (sample_count > 1)
            .then(|| MsaaTarget::new(&device, format, surface_extent(&config), sample_count));

        self.gfx = Some(Gfx {
            renderer,
            surface,
            queue,
            device,
            window,
            config,
            msaa,
        });
        self.next_periodic_redraw = self
            .config
            .redraw_interval
            .map(|interval| Instant::now() + interval);
        self.gfx.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.gfx.take();
                event_loop.exit();
            }

            event => {
                let Some(gfx) = self.gfx.as_mut() else {
                    return;
                };
                let scale = gfx.window.scale_factor() as f32;

                match event {
                    WindowEvent::Resized(size) => {
                        let w = size.width.max(1);
                        let h = size.height.max(1);
                        // Drop no-op resizes the compositor sometimes
                        // re-sends with the same dimensions — running
                        // surface.configure() for them just stalls the
                        // GPU pipeline without changing anything.
                        let already_pending = self
                            .pending_resize
                            .map(|s| s.width == w && s.height == h)
                            .unwrap_or(false);
                        let same_as_current = self.pending_resize.is_none()
                            && w == gfx.config.width
                            && h == gfx.config.height;
                        if already_pending || same_as_current {
                            return;
                        }
                        self.pending_resize = Some(PhysicalSize::new(w, h));
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
                        // Wayland and most X11 compositors deliver
                        // CursorMoved at high frequency while the
                        // cursor is over the surface — only redraw
                        // when the move actually changed something
                        // (hovered identity, scrollbar drag, drag
                        // event), per `PointerMove`.
                        if moved.needs_redraw {
                            gfx.window.request_redraw();
                        }
                    }

                    WindowEvent::CursorLeft { .. } => {
                        self.last_pointer = None;
                        for event in gfx.renderer.pointer_left() {
                            self.app.on_event(event);
                        }
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
                        if let Some(key) = map_key(&key_event.logical_key) {
                            for event in
                                gfx.renderer.key_down(key, self.modifiers, key_event.repeat)
                            {
                                self.app.on_event(event);
                            }
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
                        // Apply the latest coalesced resize, if any,
                        // before acquiring the next surface texture so
                        // the frame we render matches the size the
                        // compositor is asking for.
                        if let Some(size) = self.pending_resize.take() {
                            gfx.config.width = size.width;
                            gfx.config.height = size.height;
                            gfx.surface.configure(&gfx.device, &gfx.config);
                            gfx.renderer
                                .set_surface_size(gfx.config.width, gfx.config.height);
                            let extent = surface_extent(&gfx.config);
                            if let Some(msaa) = gfx.msaa.as_mut()
                                && !msaa.matches(extent)
                            {
                                *msaa = MsaaTarget::new(
                                    &gfx.device,
                                    gfx.config.format,
                                    extent,
                                    msaa.sample_count,
                                );
                            }
                        }
                        let frame = match gfx.surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(t)
                            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                            wgpu::CurrentSurfaceTexture::Lost
                            | wgpu::CurrentSurfaceTexture::Outdated => {
                                gfx.surface.configure(&gfx.device, &gfx.config);
                                return;
                            }
                            other => {
                                eprintln!("surface unavailable: {other:?}");
                                return;
                            }
                        };
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        WinitWgpuApp::before_build(&mut self.app);
                        let theme = self.app.theme();
                        let cx = aetna_core::BuildCx::new(&theme)
                            .with_ui_state(gfx.renderer.ui_state());
                        let mut tree = self.app.build(&cx);
                        let palette = theme.palette().clone();
                        gfx.renderer.set_theme(theme);
                        // Snapshot hotkeys alongside build() so the chord list
                        // reflects current state (apps can return different
                        // hotkeys per mode, e.g. `j/k` only in list view).
                        gfx.renderer.set_hotkeys(self.app.hotkeys());
                        gfx.renderer.set_selection(self.app.selection());
                        // Drain any toasts the app accumulated since
                        // the last frame and queue them onto the
                        // runtime's toast stack. The synthesize pass
                        // inside `prepare` then renders the layer.
                        gfx.renderer.push_toasts(self.app.drain_toasts());
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

                        // Resolve the pointer cursor against the laid-out
                        // tree (computed_ids are now valid) and push it to
                        // the window when it changes. Doing this after
                        // prepare means hover / press transitions show up
                        // on the same frame as the visual update.
                        let cursor = gfx.renderer.ui_state().cursor(&tree);
                        if cursor != self.last_cursor {
                            gfx.window.set_cursor(winit_cursor(cursor));
                            self.last_cursor = cursor;
                        }

                        let mut encoder =
                            gfx.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("aetna_winit_wgpu::encoder"),
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
                            gfx.msaa.as_ref().map(|msaa| &msaa.view),
                            wgpu::LoadOp::Clear(bg_color(&palette)),
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
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_ref() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        let Some(interval) = self.config.redraw_interval else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        let now = Instant::now();
        let next = self
            .next_periodic_redraw
            .get_or_insert_with(|| now + interval);
        if now >= *next {
            gfx.window.request_redraw();
            *next = now + interval;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(*next));
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
        // Back / Forward / Other → not surfaced; apps that need them can
        // grow the enum.
        _ => None,
    }
}

/// Translate an Aetna [`Cursor`] to winit's [`CursorIcon`]. The Aetna
/// enum is a subset of winit's so this stays a 1:1 map; the wildcard
/// arm is a forward-compat safety net (Aetna's `Cursor` is
/// `non_exhaustive` — add a new variant in core, add the matching arm
/// here, otherwise it falls back to the platform default).
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

fn key_modifiers(mods: winit::keyboard::ModifiersState) -> KeyModifiers {
    KeyModifiers {
        shift: mods.shift_key(),
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        logo: mods.super_key(),
    }
}

fn bg_color(palette: &aetna_core::Palette) -> wgpu::Color {
    let c = palette.background;
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

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::Selection;
    use aetna_core::SelectionPoint;
    use aetna_core::SelectionRange;

    /// `BasicApp` is the wrapper the host uses around the user's app
    /// type. It must forward every per-frame App trait method to the
    /// inner type — a missing forward silently falls through to the
    /// trait default and the host loses sight of app state. A
    /// previous bug had `selection()` left out, which made the
    /// painter never receive a non-empty selection.
    #[test]
    fn basic_app_forwards_selection_to_inner() {
        struct AppWithSelection;
        impl App for AppWithSelection {
            fn build(&self, _cx: &aetna_core::BuildCx) -> aetna_core::El {
                aetna_core::widgets::text::text("hi")
            }
            fn selection(&self) -> Selection {
                Selection {
                    range: Some(SelectionRange {
                        anchor: SelectionPoint::new("p", 0),
                        head: SelectionPoint::new("p", 5),
                    }),
                }
            }
        }
        let basic = BasicApp(AppWithSelection);
        let sel = basic.selection();
        let r = sel.range.as_ref().expect("range forwarded through wrapper");
        assert_eq!(r.anchor.key, "p");
        assert_eq!(r.head.byte, 5);
    }
}
