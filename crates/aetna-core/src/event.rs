//! Event types and the [`App`] trait.
//!
//! State-driven rebuilds, routed events, keyboard input, and automatic
//! hover/press/focus visuals. See `docs/LIBRARY_VISION.md` for the application
//! model this fits into.
//!
//! This module owns the *types* — what the host's `App::on_event` sees
//! and what gets registered as hotkeys. The state machine that produces
//! these events lives in [`crate::state::UiState`]; the routing helpers
//! live in [`mod@crate::hit_test`] and [`mod@crate::focus`].
//!
//! # The model
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Counter { value: i32 }
//!
//! impl App for Counter {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             h1(format!("{}", self.value)),
//!             row([
//!                 button("-").key("dec"),
//!                 button("+").key("inc"),
//!             ]),
//!         ])
//!     }
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.is_click_or_activate("inc") {
//!             self.value += 1;
//!         } else if e.is_click_or_activate("dec") {
//!             self.value -= 1;
//!         }
//!     }
//! }
//! ```
//!
//! - **Identity** is `El::key`. Tag a node with `.key("...")` and it's
//!   hit-testable (and gets automatic hover/press visuals).
//! - **The build closure is pure.** It reads `&self`, returns a fresh
//!   tree. The library tracks pointer state, hovered key, pressed key
//!   internally and applies visual deltas after build but before layout
//!   completes.
//! - **Events flow back via `on_event`.** The library hit-tests pointer
//!   events against the most-recently-laid-out tree and emits
//!   [`UiEvent`]s when something is clicked. The host's `App::on_event`
//!   updates state; the renderer reports whether animation state needs
//!   another redraw.

use crate::tree::{El, Rect};

/// Hit-test target metadata. `key` is the author-facing route, while
/// `node_id` is the stable laid-out tree path used by artifacts.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct UiTarget {
    pub key: String,
    pub node_id: String,
    pub rect: Rect,
}

/// Which mouse button (or pointer button) generated a pointer event.
/// The host backend translates its native button id to one of these
/// before calling `pointer_down` / `pointer_up`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    /// Left mouse, primary touch, or pen tip. Drives `Click`.
    Primary,
    /// Right mouse or two-finger touch. Drives `SecondaryClick` —
    /// typically opens a context menu.
    Secondary,
    /// Middle mouse / scroll-wheel click. No library default; surfaced
    /// as `MiddleClick` for apps that want it (autoscroll, paste-on-X).
    Middle,
}

/// Keyboard key values normalized by the core library. This keeps the
/// core independent from host/windowing crates while covering the
/// navigation and activation keys the library owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiKey {
    Enter,
    Escape,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Backspace — deletes the grapheme before the caret.
    Backspace,
    /// Forward delete — deletes the grapheme after the caret.
    Delete,
    /// Home — caret to start of line.
    Home,
    /// End — caret to end of line.
    End,
    /// PageUp — coarse-step navigation (sliders adjust by a larger
    /// amount; lists scroll a viewport).
    PageUp,
    /// PageDown — coarse-step navigation (sliders adjust by a larger
    /// amount; lists scroll a viewport).
    PageDown,
    Character(String),
    Other(String),
}

/// OS modifier-key mask. The four fields mirror the platform-standard
/// modifier set; this struct is intentionally **not** `#[non_exhaustive]`
/// so callers can use struct-literal syntax with `..Default::default()`
/// to spell precise modifier combinations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyPress {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

/// A keyboard chord for app-level hotkey registration. Match a key with
/// an exact modifier mask: `KeyChord::ctrl('f')` does not also match
/// `Ctrl+Shift+F`, and `KeyChord::vim('j')` does not match if any
/// modifier is held.
///
/// Register chords from [`App::hotkeys`]; the library matches them
/// against incoming key presses ahead of focus activation routing and
/// emits a [`UiEvent`] with `kind = UiEventKind::Hotkey` and `key`
/// equal to the registered name.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyChord {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// A bare key with no modifiers (vim-style). `KeyChord::vim('j')`
    /// matches the `j` key with no Ctrl/Shift/Alt/Logo held.
    pub fn vim(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers::default(),
        }
    }

    /// `Ctrl+<char>`.
    pub fn ctrl(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        }
    }

    /// `Ctrl+Shift+<char>`.
    pub fn ctrl_shift(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        }
    }

    /// A named key with no modifiers (e.g. `KeyChord::named(UiKey::Escape)`).
    pub fn named(key: UiKey) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::default(),
        }
    }

    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Strict match: keys equal AND modifier mask is identical. Holding
    /// extra modifiers does not match a chord that didn't request them.
    pub fn matches(&self, key: &UiKey, modifiers: KeyModifiers) -> bool {
        key_eq(&self.key, key) && self.modifiers == modifiers
    }
}

fn key_eq(a: &UiKey, b: &UiKey) -> bool {
    match (a, b) {
        (UiKey::Character(x), UiKey::Character(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    }
}

/// User-facing event. The host's [`App::on_event`] receives one of these
/// per discrete user action.
///
/// Most apps should not destructure every field. Prefer the convenience
/// methods on this type for common routes:
///
/// ```
/// # use aetna_core::prelude::*;
/// # struct Counter { value: i32 }
/// # impl App for Counter {
/// # fn build(&self, _cx: &BuildCx) -> El { button("+").key("inc") }
/// fn on_event(&mut self, event: UiEvent) {
///     if event.is_click_or_activate("inc") {
///         self.value += 1;
///     }
/// }
/// # }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct UiEvent {
    /// Route string for this event.
    ///
    /// For pointer and focus events, this is the [`El::key`][crate::El::key]
    /// of the target node. For [`UiEventKind::Hotkey`], this is the
    /// action name returned from [`App::hotkeys`]. For window-level
    /// keyboard events such as Escape with no focused target, this is
    /// `None`.
    ///
    /// Prefer [`Self::route`] or [`Self::is_click_or_activate`] in app
    /// code. The field remains public for direct pattern matching.
    pub key: Option<String>,
    /// Full hit-test target for events routed to a concrete element.
    pub target: Option<UiTarget>,
    /// Pointer position in logical pixels when the event was emitted.
    pub pointer: Option<(f32, f32)>,
    /// Keyboard payload for key events.
    pub key_press: Option<KeyPress>,
    /// Composed text payload for [`UiEventKind::TextInput`] events.
    pub text: Option<String>,
    /// Library-emitted selection state for
    /// [`UiEventKind::SelectionChanged`] events. Carries the new
    /// [`crate::selection::Selection`] after the runtime resolved a
    /// pointer interaction. The app folds this into its
    /// `Selection` field the same way it folds `apply_event` results
    /// into a [`crate::widgets::text_input::TextSelection`].
    pub selection: Option<crate::selection::Selection>,
    /// Modifier mask captured at the moment this event was emitted. For
    /// keyboard events this duplicates `key_press.modifiers`; for
    /// pointer events it's the host-tracked modifier state at the time
    /// of the click / drag (used by widgets like text_input that need
    /// to detect Shift+click for "extend selection").
    pub modifiers: KeyModifiers,
    /// Click number within a multi-click sequence. Set to 1 for single
    /// click, 2 for double-click, 3 for triple-click, etc. The runtime
    /// increments this when consecutive `PointerDown`s land on the same
    /// target within ~500ms and ~4px of the previous click. `0` means
    /// "not applicable" — set on every event other than `PointerDown` /
    /// `PointerUp` / `Click` (and their secondary / middle siblings,
    /// which always carry 1).
    ///
    /// `text_input` / `text_area` and the static-text selection
    /// manager read this to map double-click → select word, triple-
    /// click → select line.
    pub click_count: u8,
    /// File system path for [`UiEventKind::FileHovered`] /
    /// [`UiEventKind::FileDropped`] events. Multi-file drag-drops fire
    /// one event per file (matching the underlying winit semantics);
    /// each event carries one path. `PathBuf` rather than `String`
    /// because Windows wide-char paths and unusual Unix paths aren't
    /// guaranteed to be UTF-8.
    pub path: Option<std::path::PathBuf>,
    pub kind: UiEventKind,
}

impl UiEvent {
    /// Synthesize a click event for the given route key.
    ///
    /// Intended for tests, headless automation, and snapshot
    /// fixtures that drive UI logic without a real pointer history.
    /// All optional fields default to `None`; modifiers are empty.
    pub fn synthetic_click(key: impl Into<String>) -> Self {
        Self {
            kind: UiEventKind::Click,
            key: Some(key.into()),
            target: None,
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            path: None,
        }
    }

    /// Route string for this event, if any.
    ///
    /// For pointer/focus events this is the target element key. For
    /// hotkeys this is the registered action name.
    pub fn route(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Target element key, if this event was routed to an element.
    ///
    /// Unlike [`Self::route`], this returns `None` for app-level
    /// hotkey actions because those do not have a concrete element
    /// target.
    pub fn target_key(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.key.as_str())
    }

    /// True when this event's route equals `key`.
    pub fn is_route(&self, key: &str) -> bool {
        self.route() == Some(key)
    }

    /// True for a primary click or keyboard activation on `key`.
    ///
    /// This is the most common button/menu route in app code.
    pub fn is_click_or_activate(&self, key: &str) -> bool {
        matches!(self.kind, UiEventKind::Click | UiEventKind::Activate) && self.is_route(key)
    }

    /// True for a registered hotkey action name.
    pub fn is_hotkey(&self, action: &str) -> bool {
        self.kind == UiEventKind::Hotkey && self.is_route(action)
    }

    /// Pointer position in logical pixels, if this event carries one.
    pub fn pointer_pos(&self) -> Option<(f32, f32)> {
        self.pointer
    }

    /// Pointer x coordinate in logical pixels, if this event carries one.
    pub fn pointer_x(&self) -> Option<f32> {
        self.pointer.map(|(x, _)| x)
    }

    /// Pointer y coordinate in logical pixels, if this event carries one.
    pub fn pointer_y(&self) -> Option<f32> {
        self.pointer.map(|(_, y)| y)
    }

    /// Rectangle of the routed target from the last layout pass.
    pub fn target_rect(&self) -> Option<Rect> {
        self.target.as_ref().map(|t| t.rect)
    }

    /// OS-composed text payload for [`UiEventKind::TextInput`].
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

/// What kind of event happened.
///
/// This enum is non-exhaustive so Aetna can add new input events
/// without breaking downstream apps. Match the variants you handle and
/// include a wildcard arm for everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEventKind {
    /// Primary-button pointer down + up landed on the same node.
    Click,
    /// Secondary-button (right-click) pointer down + up landed on the
    /// same node. Used for context menus.
    SecondaryClick,
    /// Middle-button pointer down + up landed on the same node.
    MiddleClick,
    /// Focused element was activated by keyboard (Enter/Space).
    Activate,
    /// Escape was pressed. Routed to the focused element when present,
    /// otherwise emitted as a window-level event.
    Escape,
    /// A registered hotkey chord matched. `event.key` is the registered
    /// name (the second element of the `(KeyChord, String)` pair).
    Hotkey,
    /// Other keyboard input.
    KeyDown,
    /// Composed text input — printable characters from the OS, after
    /// dead-key composition / IME / shift mapping. Routed to the
    /// focused element. Distinct from `KeyDown(Character(_))`: the
    /// latter is the raw key event used for shortcuts and navigation;
    /// `TextInput` is the grapheme stream a text field should consume.
    TextInput,
    /// Pointer moved while the primary button was held down. Routed
    /// to the originally pressed target so a widget can extend a
    /// selection / scrub a slider / move a draggable. `event.pointer`
    /// carries the current logical-pixel position; `event.target` is
    /// the node where the drag began.
    Drag,
    /// Primary pointer button released. Fires regardless of whether
    /// the up landed on the same node as the down — paired with
    /// `Click` (which only fires on a same-node match), this lets
    /// drag-aware widgets always observe drag-end.
    /// `event.target` is the originally pressed node;
    /// `event.pointer` is the up position.
    PointerUp,
    /// Primary pointer button pressed on a hit-test target. Routed
    /// before the eventual `Click` (which fires on up-on-same-target).
    /// Used by widgets like text_input that need to react at
    /// down-time — e.g., to set the selection anchor before any drag
    /// extends it. `event.target` is the down-target,
    /// `event.pointer` is the down position, and `event.modifiers`
    /// carries the modifier mask (Shift+click for extend-selection).
    PointerDown,
    /// The library's selection manager resolved a pointer interaction
    /// on selectable text and wants the app to update its
    /// [`crate::selection::Selection`] state. `event.selection`
    /// carries the new value (an empty `Selection` clears).
    /// Emitted by `pointer_down`, `pointer_moved` (during a drag),
    /// and the runtime's escape / dismiss paths.
    SelectionChanged,
    /// Pointer crossed onto a keyed hit-test target. Routed to the
    /// newly hovered leaf — `event.target` is the new hover target,
    /// `event.pointer` is the current pointer position. Fires
    /// once per identity change, including the initial hover when the
    /// pointer first enters a keyed region from nothing.
    ///
    /// Use for transition-driven side effects (sound on hover-enter,
    /// analytics, hover-intent prefetch) — read state via
    /// [`crate::BuildCx::hovered_key`] /
    /// [`crate::BuildCx::is_hovering_within`] when you just need to
    /// branch the build output. Both surfaces stay coherent because
    /// the runtime debounces redraws and events to the same
    /// hover-identity transitions.
    ///
    /// Always paired with a preceding `PointerLeave` for the previous
    /// target (when there was one). Apps that want subtree-aware
    /// behavior (parent stays "hot" while a child is hovered) should
    /// query `is_hovering_within` rather than tracking enter/leave on
    /// every keyed descendant.
    PointerEnter,
    /// Pointer crossed off a keyed hit-test target — either onto a
    /// different keyed target (paired with a following `PointerEnter`)
    /// or off any keyed surface entirely. Routed to the leaf that
    /// just lost hover — `event.target` is the previous hover target,
    /// `event.pointer` is the current pointer position (or the last
    /// known position when the pointer left the window).
    PointerLeave,
    /// A file is being dragged over the window (the user hasn't
    /// released yet). `event.path` carries the file's path; multi-file
    /// drags fire one event per file, matching the underlying winit
    /// semantics. `event.target` is the keyed leaf at the current
    /// pointer position when one was hit, otherwise `None`
    /// (drop-zone overlays that span the window can match on
    /// `event.target.is_none()` or filter by their own key).
    ///
    /// Apps use this to highlight a drop zone before the drop lands.
    /// Always paired with either a later `FileHoverCancelled` (the
    /// user moved off without releasing) or `FileDropped` (the user
    /// released).
    FileHovered,
    /// The user moved a hovered file off the window without releasing,
    /// or pressed Escape. Window-level event (`event.target` is
    /// `None`) — apps clear any drop-zone affordance state regardless
    /// of which keyed leaf was previously highlighted.
    FileHoverCancelled,
    /// A file was dropped on the window. `event.path` carries the
    /// path; multi-file drops fire one event per file. `event.target`
    /// is the keyed leaf at the drop position, or `None` if the drop
    /// landed outside any keyed surface — apps that want a global drop
    /// target match on `target.is_none()` or treat unrouted events as
    /// hits to a single window-level upload sink.
    FileDropped,
}

/// Per-frame, read-only context for [`App::build`].
///
/// The runner snapshots the app's [`crate::Theme`] before calling
/// `build` and exposes it through `cx.theme()` / `cx.palette()` so app
/// code can branch on the active palette (a custom widget that picks
/// between two non-token colors based on dark vs. light, for instance).
/// `BuildCx` is the explicit handle for this — token references inside
/// widgets resolve through the palette automatically and don't need it.
///
/// Future fields like viewport metrics or frame phase will live here so
/// the API stays additive: adding a new accessor on `BuildCx` doesn't
/// break apps that ignore the context.
#[derive(Copy, Clone, Debug)]
pub struct BuildCx<'a> {
    theme: &'a crate::Theme,
    ui_state: Option<&'a crate::state::UiState>,
    diagnostics: Option<&'a HostDiagnostics>,
}

/// Why the current frame is being built. Hosts set this before each
/// `request_redraw` so apps that surface a diagnostic overlay can show
/// what kind of input is driving the redraw cadence.
///
/// `Other` is the conservative default: it covers redraws the host
/// can't attribute (idle redraws driven by external `request_redraw`
/// callers, the initial paint, etc.). Specific variants narrow the
/// reason when the host can.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FrameTrigger {
    /// Host can't attribute the redraw to a specific cause.
    #[default]
    Other,
    /// Initial paint after surface configuration.
    Initial,
    /// Surface resize / DPI change.
    Resize,
    /// Pointer move, button, or wheel.
    Pointer,
    /// Keyboard / IME input.
    Keyboard,
    /// Inside-out animation deadline elapsed (one of the visible
    /// widgets asked for a future frame via `redraw_within`, or a
    /// visual animation is still settling). Drives the layout-path
    /// (full rebuild + prepare).
    Animation,
    /// Time-driven shader deadline elapsed (e.g. stock spinner /
    /// skeleton / progress-indeterminate, or a custom shader
    /// registered with `samples_time=true`). Drives the paint-only
    /// path: `frame.time` advances but layout state is unchanged.
    ShaderPaint,
    /// Periodic host-config cadence (`HostConfig::redraw_interval`).
    Periodic,
}

impl FrameTrigger {
    /// Short, fixed-width tag for diagnostic overlays.
    pub fn label(self) -> &'static str {
        match self {
            FrameTrigger::Other => "other",
            FrameTrigger::Initial => "initial",
            FrameTrigger::Resize => "resize",
            FrameTrigger::Pointer => "pointer",
            FrameTrigger::Keyboard => "keyboard",
            FrameTrigger::Animation => "animation",
            FrameTrigger::ShaderPaint => "shader-paint",
            FrameTrigger::Periodic => "periodic",
        }
    }
}

/// Per-frame diagnostic snapshot the host hands the app via
/// [`BuildCx::diagnostics`]. Apps that surface a debug overlay (e.g.
/// the showcase status block) read this each build to display the
/// active backend, frame cadence, and what triggered the redraw.
///
/// Hosts populate every field they can; `backend` is a static string
/// (`"WebGPU"`, `"Vulkan"`, `"Metal"`, `"DX12"`, `"GL"`) so the app
/// doesn't need to depend on `wgpu` to read it. Time fields use
/// `std::time::Duration`, which works on both native and wasm32 — only
/// `Instant::now()` is the wasm-incompatible piece, and that stays on
/// the host side.
#[derive(Clone, Debug)]
pub struct HostDiagnostics {
    /// Render backend in human-readable form.
    pub backend: &'static str,
    /// Current surface size in physical pixels.
    pub surface_size: (u32, u32),
    /// Display scale factor (`physical / logical`).
    pub scale_factor: f32,
    /// Active MSAA sample count (1 = MSAA off).
    pub msaa_samples: u32,
    /// Frame counter; increments every redraw the host actually
    /// renders. Useful for verifying that an animated source is
    /// progressing.
    pub frame_index: u64,
    /// Wall-clock time between this redraw and the previous one.
    /// `Duration::ZERO` for the first frame (no prior frame).
    pub last_frame_dt: std::time::Duration,
    /// Why the host triggered this frame.
    pub trigger: FrameTrigger,
}

impl Default for HostDiagnostics {
    fn default() -> Self {
        Self {
            backend: "?",
            surface_size: (0, 0),
            scale_factor: 1.0,
            msaa_samples: 1,
            frame_index: 0,
            last_frame_dt: std::time::Duration::ZERO,
            trigger: FrameTrigger::default(),
        }
    }
}

impl<'a> BuildCx<'a> {
    /// Construct a [`BuildCx`] borrowing the supplied theme. Hosts call
    /// this once per frame after [`App::theme`] and before [`App::build`].
    /// Hosts that own a [`crate::state::UiState`] should chain
    /// [`Self::with_ui_state`] so the app can read interaction state
    /// (hover) during build via [`Self::hovered_key`] /
    /// [`Self::is_hovering_within`].
    pub fn new(theme: &'a crate::Theme) -> Self {
        Self {
            theme,
            ui_state: None,
            diagnostics: None,
        }
    }

    /// Attach the runtime's [`crate::state::UiState`] so build-time
    /// accessors (`hovered_key`, `is_hovering_within`) can answer.
    /// When omitted, those accessors return `None` / `false` — useful
    /// for headless rendering paths that don't track interaction
    /// state.
    pub fn with_ui_state(mut self, ui_state: &'a crate::state::UiState) -> Self {
        self.ui_state = Some(ui_state);
        self
    }

    /// Attach a [`HostDiagnostics`] snapshot for this frame. Hosts call
    /// this when they want apps to surface debug overlays (e.g. the
    /// showcase status block); apps that don't read `diagnostics()`
    /// pay nothing for it. Headless render paths leave it `None`.
    pub fn with_diagnostics(mut self, diagnostics: &'a HostDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Per-frame diagnostic snapshot from the host (backend, frame
    /// cadence, trigger reason, etc.), or `None` when the host did
    /// not attach one. Apps display this in optional debug overlays.
    pub fn diagnostics(&self) -> Option<&HostDiagnostics> {
        self.diagnostics
    }

    /// The active runtime theme for this frame.
    pub fn theme(&self) -> &crate::Theme {
        self.theme
    }

    /// Shorthand for `self.theme().palette()`.
    pub fn palette(&self) -> &crate::Palette {
        self.theme.palette()
    }

    /// Key of the leaf node currently under the pointer, or `None`
    /// when nothing is hovered or this `BuildCx` was built without a
    /// `UiState` (headless rendering paths).
    ///
    /// Use for branching the build output on hover state without
    /// mirroring it via `App::on_event` handlers — e.g., a sidebar
    /// row that previews details in a side pane based on what's
    /// currently hovered.
    ///
    /// For region-aware queries (parent stays "hot" while a child is
    /// hovered), prefer [`Self::is_hovering_within`].
    pub fn hovered_key(&self) -> Option<&str> {
        self.ui_state?.hovered_key()
    }

    /// True iff `key`'s node — or any descendant of it — is the
    /// current hover target. Subtree-aware, matching the semantics of
    /// [`crate::tree::El::hover_alpha`]. Returns `false` when this
    /// `BuildCx` has no attached `UiState` or when `key` isn't in the
    /// current tree.
    ///
    /// Reads the underlying tracker, not the eased subtree envelope —
    /// the boolean flips immediately on hit-test identity change.
    pub fn is_hovering_within(&self, key: &str) -> bool {
        self.ui_state
            .is_some_and(|state| state.is_hovering_within(key))
    }
}

/// The application contract. Implement this on your state struct and
/// pass it to a host runner (e.g., `aetna_winit_wgpu::run`).
pub trait App {
    /// Refresh app-owned external state immediately before a frame is
    /// built.
    ///
    /// Hosts call this once per redraw before [`Self::build`]. Use it
    /// for polling an external source, reconciling optimistic local
    /// state with a backend snapshot, or advancing host-owned live data
    /// that should be visible in the next tree. Keep expensive work
    /// outside the render loop; this hook is still on the frame path.
    ///
    /// Default: no-op.
    fn before_build(&mut self) {}

    /// Project current state into a scene tree. Called whenever the
    /// host requests a redraw, after [`Self::before_build`]. Prefer to
    /// keep this pure: read current state and return a fresh tree.
    ///
    /// `cx` carries per-frame, read-only context (active theme, future
    /// viewport / phase metadata). Apps that don't need to branch on
    /// the theme during construction can ignore the parameter — token
    /// references in widget code resolve through the palette
    /// automatically.
    fn build(&self, cx: &BuildCx) -> El;

    /// Update state in response to a routed event. Default: no-op.
    fn on_event(&mut self, _event: UiEvent) {}

    /// The application's current text [`crate::selection::Selection`].
    /// Read by the host once per frame so the library can paint
    /// highlight bands and resolve `selected_text` for clipboard.
    /// Apps that own a `Selection` field return a clone here; the
    /// default returns the empty selection.
    fn selection(&self) -> crate::selection::Selection {
        crate::selection::Selection::default()
    }

    /// App-level hotkey registry. The library matches incoming key
    /// presses against this list before its own focus-activation
    /// routing; a match emits a [`UiEvent`] with `kind =
    /// UiEventKind::Hotkey` and `key = Some(name)`.
    ///
    /// Called once per build cycle; the host runner snapshots the list
    /// alongside `build()` so the chords stay in sync with state.
    /// Default: no hotkeys.
    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        Vec::new()
    }

    /// Drain pending toast notifications produced since the last
    /// frame. The runtime calls this once per `prepare_layout`,
    /// stamps each spec with a monotonic id and `expires_at = now +
    /// ttl`, queues it in the runtime toast state, and
    /// synthesizes a `toast_stack` layer at the El root so the
    /// rendered tree mirrors the visible state. Apps typically
    /// accumulate specs in a `Vec<ToastSpec>` field from event
    /// handlers, then `mem::take` it here.
    ///
    /// **Root requirement:** apps that produce toasts (or use
    /// `.tooltip(text)` on any node) must wrap their
    /// [`Self::build`] return value in `overlays(main, [])` so the
    /// runtime can append the floating layer as an overlay sibling
    /// — same convention used for popovers and modals. Debug
    /// builds panic if the synthesizer runs against a non-overlay
    /// root.
    ///
    /// Default: no toasts.
    fn drain_toasts(&mut self) -> Vec<crate::toast::ToastSpec> {
        Vec::new()
    }

    /// Custom shaders this app needs registered. Each entry carries
    /// the shader name, its WGSL source, and per-flag opt-ins
    /// (backdrop sampling, time-driven motion). The host runner
    /// registers them once at startup via
    /// `Runner::register_shader_with(name, wgsl, samples_backdrop, samples_time)`.
    ///
    /// Backends that don't support backdrop sampling skip entries with
    /// `samples_backdrop=true`; any node bound to such a shader will
    /// draw nothing on those backends rather than mis-render.
    /// `samples_time=true` declares that the shader's output depends
    /// on `frame.time`, which keeps the host idle loop ticking while
    /// any node is bound to it.
    ///
    /// Default: no shaders.
    fn shaders(&self) -> Vec<AppShader> {
        Vec::new()
    }

    /// Runtime paint theme for this app. Hosts apply it to the renderer
    /// before preparing each frame so stateful apps can switch global
    /// material routing without backend-specific calls.
    fn theme(&self) -> crate::Theme {
        crate::Theme::default()
    }
}

/// One custom shader registration, returned from [`App::shaders`].
#[derive(Clone, Copy, Debug)]
pub struct AppShader {
    pub name: &'static str,
    pub wgsl: &'static str,
    /// Reads the prior pass's color target (`@group(2) backdrop_tex`).
    /// Backends without backdrop support skip these.
    pub samples_backdrop: bool,
    /// Reads `frame.time` and so requires continuous redraw whenever
    /// any node is bound to it. The runtime ORs this into
    /// `PrepareResult::needs_redraw` per frame.
    pub samples_time: bool,
}
