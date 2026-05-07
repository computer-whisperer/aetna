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
//!     fn build(&self) -> El {
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
/// # fn build(&self) -> El { button("+").key("inc") }
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
    fn build(&self) -> El;

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

    /// Custom shaders this app needs registered. Each tuple is
    /// `(name, wgsl_source, samples_backdrop)`. The host runner
    /// registers them once at startup via
    /// `Runner::register_shader_with(name, wgsl, samples_backdrop)`.
    ///
    /// Backends that don't support backdrop sampling skip entries with
    /// `samples_backdrop=true`; any node bound to such a shader will
    /// draw nothing on those backends rather than mis-render.
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
    pub samples_backdrop: bool,
}
