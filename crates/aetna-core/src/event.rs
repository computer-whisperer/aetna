//! Event types and the [`App`] trait.
//!
//! The v0.2 application layer: state-driven rebuild + click events +
//! automatic hover/press visuals. See `LIBRARY_VISION.md` for the
//! shape this fits into.
//!
//! This module owns the *types* — what the host's `App::on_event` sees
//! and what gets registered as hotkeys. The state machine that produces
//! these events lives in [`crate::state::UiState`]; the routing helpers
//! live in [`crate::hit_test`] and [`crate::focus`].
//!
//! # The model
//!
//! ```ignore
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
//!         match (e.kind, e.key.as_deref()) {
//!             (UiEventKind::Click, Some("inc")) => self.value += 1,
//!             (UiEventKind::Click, Some("dec")) => self.value -= 1,
//!             _ => {}
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
//!   updates state; the library schedules a redraw.

use crate::tree::{El, Rect};

/// Hit-test target metadata. `key` is the author-facing route, while
/// `node_id` is the stable laid-out tree path used by artifacts.
#[derive(Clone, Debug, PartialEq)]
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
    Character(String),
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
/// per discrete user action (click, key press, scroll wheel tick, …).
#[derive(Clone, Debug)]
pub struct UiEvent {
    /// The `key` of the node the event was routed to, if any. `None`
    /// for events with no specific target (e.g., a window-level
    /// keyboard event).
    pub key: Option<String>,
    /// Full hit-test target for events routed to a concrete element.
    pub target: Option<UiTarget>,
    /// Pointer position in logical pixels when the event was emitted.
    pub pointer: Option<(f32, f32)>,
    /// Keyboard payload for key events.
    pub key_press: Option<KeyPress>,
    /// Composed text payload for [`UiEventKind::TextInput`] events.
    pub text: Option<String>,
    pub kind: UiEventKind,
}

/// What kind of event happened. Open enum — start with click, grow
/// non-breakingly as the library does.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

/// The application contract. Implement this on your state struct and
/// pass it to the host runner (e.g., `aetna_demo::run`).
pub trait App {
    /// Project current state into a scene tree. Called whenever the
    /// host requests a redraw. Pure — no I/O, no mutation.
    fn build(&self) -> El;

    /// Update state in response to a routed event. Default: no-op.
    fn on_event(&mut self, _event: UiEvent) {}

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

    /// Custom shaders this app needs registered. Each tuple is
    /// `(name, wgsl_source, samples_backdrop)`. The host runner
    /// registers them once at startup via
    /// `Runner::register_shader_with(name, wgsl, samples_backdrop)`.
    ///
    /// Backends that don't yet support backdrop sampling (e.g.
    /// vulkano in v0.7) skip entries with `samples_backdrop=true`;
    /// any node bound to such a shader will draw nothing on those
    /// backends rather than mis-render.
    ///
    /// Default: no shaders.
    fn shaders(&self) -> Vec<AppShader> {
        Vec::new()
    }
}

/// One custom shader registration, returned from [`App::shaders`].
#[derive(Clone, Copy, Debug)]
pub struct AppShader {
    pub name: &'static str,
    pub wgsl: &'static str,
    pub samples_backdrop: bool,
}
