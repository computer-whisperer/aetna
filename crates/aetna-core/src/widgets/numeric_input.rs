//! Numeric input — text input with `−` / `+` spinner buttons.
//!
//! shadcn doesn't ship a dedicated component (web apps lean on
//! `<input type="number">` and let the browser draw spinners); for a
//! renderer-agnostic UI kit we render the spinners explicitly so the
//! affordance is consistent across backends.
//!
//! The app owns the value as a `String` (matching [`text_input`]) so
//! mid-edit states like `"1."` aren't clobbered by a parse-and-reformat
//! round-trip on every keystroke. Parse to a number with
//! `s.parse::<f64>()` (or `i64`, …) when you actually need the value.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Form {
//!     count: String,
//!     selection: Selection,
//! }
//!
//! impl App for Form {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         let opts = NumericInputOpts::default()
//!             .min(0.0)
//!             .max(100.0)
//!             .step(1.0);
//!         numeric_input(&self.count, &self.selection, "count", opts)
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent) {
//!         let opts = NumericInputOpts::default()
//!             .min(0.0)
//!             .max(100.0)
//!             .step(1.0);
//!         numeric_input::apply_event(
//!             &mut self.count, &mut self.selection, "count", &opts, &e,
//!         );
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:dec` — `Click` on the `−` button. Steps the value down.
//! - `{key}:inc` — `Click` on the `+` button. Steps the value up.
//! - `{key}:field` — the inner [`text_input`]; routed text edits / IME
//!   commits / pointer caret moves all flow through this key.
//!
//! Spinner clicks parse the current `value`, add or subtract
//! `opts.step`, clamp to `opts.min`/`opts.max` if set, and write the
//! formatted result back. If the value can't be parsed (empty or
//! garbage), the spinner treats it as `min` when set, otherwise as
//! `0.0`.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface: a `row` with two
//! ghost [`button`]s and an inner [`text_input_with`]. An app crate
//! can fork this file to add a different spinner shape (stacked
//! arrows, wheel-on-scroll, named units) without touching library
//! internals.

use std::panic::Location;

use crate::event::{UiEvent, UiEventKind};
use crate::selection::Selection;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::button;
use crate::widgets::text_input::{
    TextInputOpts, apply_event_with as text_input_apply, text_input_with,
};

/// Configuration for [`numeric_input`] / [`apply_event`].
///
/// Defaults: no min, no max, `step = 1.0`, no fixed precision, no
/// placeholder. The same value is expected to be available both at
/// build-time (for the placeholder) and at event-time (so spinner
/// clicks know how much to step and where to clamp), so this is a
/// struct the app holds onto rather than chained modifiers on the
/// returned `El` — the same pattern [`TextInputOpts`] uses.
#[derive(Clone, Copy, Debug)]
pub struct NumericInputOpts<'a> {
    /// Lower bound. Spinner clicks clamp to at least this value.
    /// `None` means unbounded below.
    pub min: Option<f64>,
    /// Upper bound. Spinner clicks clamp to at most this value.
    /// `None` means unbounded above.
    pub max: Option<f64>,
    /// Increment for one spinner click. Default `1.0`.
    pub step: f64,
    /// Fixed decimal places for the formatted result.
    /// `None` means: integral values render as `42`, non-integral via
    /// `f64::Display`. `Some(n)` always formats with `n` decimals
    /// (e.g. `Some(2)` produces `"3.50"`).
    pub decimals: Option<u8>,
    /// Muted hint shown only while `value` is empty.
    pub placeholder: Option<&'a str>,
}

impl Default for NumericInputOpts<'_> {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            step: 1.0,
            decimals: None,
            placeholder: None,
        }
    }
}

impl<'a> NumericInputOpts<'a> {
    pub fn min(mut self, v: f64) -> Self {
        self.min = Some(v);
        self
    }
    pub fn max(mut self, v: f64) -> Self {
        self.max = Some(v);
        self
    }
    pub fn step(mut self, v: f64) -> Self {
        self.step = v;
        self
    }
    pub fn decimals(mut self, v: u8) -> Self {
        self.decimals = Some(v);
        self
    }
    pub fn placeholder(mut self, p: &'a str) -> Self {
        self.placeholder = Some(p);
        self
    }
}

/// A numeric input field: `[−] [text_input] [+]`.
///
/// The two spinner buttons are routed `{key}:dec` and `{key}:inc`;
/// the inner text input is keyed `{key}:field`. The wrapping `row` is
/// keyed `{key}` itself so layout/test code can find the whole
/// composite by the same name the app uses.
#[track_caller]
pub fn numeric_input(
    value: &str,
    selection: &Selection,
    key: &str,
    opts: NumericInputOpts<'_>,
) -> El {
    let caller = Location::caller();

    let dec = button("−")
        .at_loc(caller)
        .key(format!("{key}:dec"))
        .ghost()
        .width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    let inc = button("+")
        .at_loc(caller)
        .key(format!("{key}:inc"))
        .ghost()
        .width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));

    let mut text_opts = TextInputOpts::default();
    if let Some(p) = opts.placeholder {
        text_opts = text_opts.placeholder(p);
    }
    let field_key = format!("{key}:field");
    let field = text_input_with(value, selection, &field_key, text_opts).width(Size::Fill(1.0));

    row([dec, field, inc])
        .at_loc(caller)
        .key(key.to_string())
        .gap(0.0)
        .align(Align::Center)
        .height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

/// Fold a routed [`UiEvent`] into the numeric input's value, handling
/// both spinner clicks and text edits. Returns `true` if the event
/// belonged to this widget (regardless of whether the value changed).
///
/// Spinner clicks parse the current `value`, step by `opts.step`,
/// clamp to `opts.min`/`opts.max`, and rewrite `value` formatted per
/// `opts.decimals`. Text edits are forwarded verbatim to
/// [`text_input::apply_event`] — no parse / reformat cycle, so a
/// half-typed `"1."` keeps its cursor position.
pub fn apply_event(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    opts: &NumericInputOpts<'_>,
    event: &UiEvent,
) -> bool {
    if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        let inc_key = format!("{key}:inc");
        let dec_key = format!("{key}:dec");
        if event.route() == Some(inc_key.as_str()) {
            step_value(value, opts, 1);
            return true;
        }
        if event.route() == Some(dec_key.as_str()) {
            step_value(value, opts, -1);
            return true;
        }
    }
    let field_key = format!("{key}:field");
    let text_opts = match opts.placeholder {
        Some(p) => TextInputOpts::default().placeholder(p),
        None => TextInputOpts::default(),
    };
    text_input_apply(value, selection, &field_key, event, &text_opts)
}

fn step_value(value: &mut String, opts: &NumericInputOpts<'_>, dir: i32) {
    // Treat unparseable input as `min` if set, else 0 — same shape as
    // browsers' default for `<input type="number">` arrow clicks
    // against an empty field.
    let parsed = value
        .parse::<f64>()
        .ok()
        .unwrap_or_else(|| opts.min.unwrap_or(0.0));
    let stepped = parsed + (dir as f64) * opts.step;
    let clamped = clamp_opt(stepped, opts.min, opts.max);
    *value = format_numeric(clamped, opts.decimals);
}

fn clamp_opt(n: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let n = if let Some(hi) = max { n.min(hi) } else { n };
    if let Some(lo) = min { n.max(lo) } else { n }
}

fn format_numeric(n: f64, decimals: Option<u8>) -> String {
    match decimals {
        Some(d) => format!("{:.*}", d as usize, n),
        None if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e18 => {
            // Integral: render without trailing ".0" so the canonical
            // round-trip of `numeric_input("0", ...) → click + → "1"`
            // doesn't drift to "1.0".
            format!("{}", n as i64)
        }
        None => format!("{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn inc_steps_value_up_by_step() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(2.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "5");
    }

    #[test]
    fn dec_steps_value_down_by_step() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(0.5).decimals(1);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "2.5");
    }

    #[test]
    fn inc_clamps_to_max() {
        let mut value = String::from("99");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(0.0).max(100.0);
        // 99 + 1*5 = 104, clamped to 100.
        let opts = opts.step(5.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "100");
    }

    #[test]
    fn dec_clamps_to_min() {
        let mut value = String::from("1");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(0.0).max(100.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "0");
        // Already at min — another dec stays at 0.
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "0");
    }

    #[test]
    fn empty_value_treated_as_min_when_set() {
        let mut value = String::new();
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(10.0).max(100.0);
        // Empty → starts at min (10), then +1 → 11.
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "11");
    }

    #[test]
    fn empty_value_treated_as_zero_when_no_min() {
        let mut value = String::new();
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "1");
    }

    #[test]
    fn unparseable_value_treated_as_zero_when_no_min() {
        let mut value = String::from("abc");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "1");
    }

    #[test]
    fn ignores_unrelated_keys() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        // Different key family — should not match this widget.
        assert!(!apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("other:inc")
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn decimals_format_pads_zeros() {
        let mut value = String::from("0");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(0.10).decimals(2);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "0.10");
    }

    #[test]
    fn no_decimals_strips_trailing_zero() {
        let mut value = String::from("0");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        // 1.0 → "1", not "1.0" (we only fall through to `f64::Display`
        // when the result has a fractional component).
        assert_eq!(value, "1");
    }

    #[test]
    fn build_widget_has_three_children_and_correct_keys() {
        let value = String::from("0");
        let sel = Selection::default();
        let opts = NumericInputOpts::default();
        let el = numeric_input(&value, &sel, "n", opts);
        assert_eq!(el.key.as_deref(), Some("n"));
        assert_eq!(el.children.len(), 3, "decrement, field, increment");
        assert_eq!(el.children[0].key.as_deref(), Some("n:dec"));
        assert_eq!(el.children[1].key.as_deref(), Some("n:field"));
        assert_eq!(el.children[2].key.as_deref(), Some("n:inc"));
    }
}
