# Aetna — Widget Kit

> The contract for building widgets on top of Aetna. **Stock widgets get no privileged APIs that user widgets don't** — this document is the public surface that proves it.

## The symmetry invariant

If a stock widget (button, card, badge, modal, scroll, …) can do something, a widget defined in an app crate must be able to do the same thing using the same API. No backdoor methods on `El`, no internal-only fields, no library-side `match` on `Kind` that lights up behaviour for one variant but not another.

Stock widgets in `crates/aetna-core/src/widgets/` are reference compositions, not privileged code paths. An app can fork any of them and produce an equivalent widget without depending on internals. **`widgets/button.rs` is the dogfood proof** — it uses only the surface documented below.

## What's in the kit

A widget is a function (or struct + builder) that returns an [`El`]. To make widgets that look and behave like stock widgets, you have these things to work with — nothing else, nothing less:

### 1. The `El` builder

The whole grammar from `crates/aetna-core/src/tree/`. Sizing (`width`, `height`, `padding`, `gap`, `axis`, `align`, `justify`), visuals (`fill`, `stroke`, `stroke_width`, `radius`, `shadow`), text (`text`, `text_color`, `text_align`, `font_size`, `font_weight`, `mono`, `italic`, `underline`, `strikethrough`, `link`, `wrap_text`), and the paint-time transforms (`opacity`, `translate`, `scale`, `animate`).

### 2. Identity & a11y tags

- `key(s)` — stable identity for hit-test routing and event delivery.
- `at_loc(loc)` — source-mapped location, set automatically when your builder is `#[track_caller]`.
- `Kind::Custom("widget-name")` — the recommended kind for any user widget. Surfaces the name in tree dumps and lint output without claiming any built-in behaviour.

The decorative `Kind` variants (`Button`, `Card`, `Badge`, `Heading`, `Modal`, `Scrim`) are inspector tags only. The library does not dispatch behaviour on them. Use them or use `Custom` — the rendered output is the same.

### 3. Style profiles

`StyleProfile` (`Solid`, `Tinted`, `Surface`, `TextOnly`) controls how the cross-cutting modifiers (`.primary`, `.success`, `.warning`, `.destructive`, `.info`, `.muted`, `.ghost`, `.outline`, `.secondary`) react to your widget. Set it once in your builder; the modifier vocabulary just works.

This is the rule that lets new widgets ship without editing `style.rs`. If your widget should react to `.primary()` like a button (solid fill), use `StyleProfile::Solid`. Like a badge (tinted alpha), use `Tinted`. Like a card (surface tint), use `Surface`. Pure text colour shifts only, use `TextOnly`.

### 4. Focus + interaction

- `.focusable()` — opt into Tab focus order and the focus ring. The library writes `focus_color` + `focus_width` uniforms onto your node's quad whenever the focus envelope is non-zero (animated by the runtime). The `RoundedRect` stock shader draws the ring in the `paint_overflow` band; if you bind a custom shader, you receive the same uniforms and decide what to paint with them.
- `.paint_overflow(Sides)` — extend your painted area beyond your layout bounds. Layout-neutral (siblings don't shift, hit-testing still uses layout bounds). Use this to give the focus ring (or a drop shadow, or a glow halo, or a custom focus visual) somewhere to render outside the box.
- `.block_pointer()` — stop pointer events from passing through to siblings underneath. Used by modal panels and similar.

The library handles `Hover` / `Press` / `Focus` envelopes automatically once `focusable` is set: hover lightens, press darkens, focus rings fade in/out. None of these are kind-keyed — they apply to any focusable node.

### 5. Custom shaders & custom layout

The two **escape hatches** documented in `SHADER_VISION.md`:

- `.shader(ShaderBinding)` — bind your own shader for the surface paint, replacing `stock::rounded_rect`. The library injects `inner_rect` and `focus_color` / `focus_width` (when focusable + focused) uniforms into your binding — your shader can use them or ignore them.
- `.layout(F)` — supply your own positioning function for direct children. The library still recurses into each child and drives hit-test / focus / animation off the rects you return.

### 6. Per-instance state — `widget_state::<T>`

Stateful widgets stash per-node, per-type state on `UiState`. The library owns the storage but never reads the values; it wipes entries when a node leaves the tree.

```rust
use aetna_core::WidgetState;

#[derive(Default, Debug)]
struct TextInputState {
    caret: usize,
    selection: Option<(usize, usize)>,
    blink_phase: f32,
}

impl WidgetState for TextInputState {
    fn debug_summary(&self) -> String {
        format!("caret={} sel={:?}", self.caret, self.selection)
    }
}

// In your build closure or event handler:
let state = ui_state.widget_state_mut::<TextInputState>(&node_id);
state.caret += 1;
```

`debug_summary()` shows up in `dump_tree` artifacts so the agent loop can see what a widget thinks per frame.

`widget_state_mut::<T: Default>` lazy-inserts on first access. `widget_state::<T>` returns `Option<&T>`. `clear_widget_state::<T>` removes the entry.

State is keyed by `(computed_id, TypeId)`, so multiple widgets can stash multiple state types on the same node without colliding.

### 7. Hotkeys & key delivery

Hotkeys are an app-level concern (`App::hotkeys()` returns `Vec<(KeyChord, String)>`); the library matches them in `key_down` ahead of focus activation. Widget builders that want a hotkey advertise the chord via the host's hotkey registry — there's no widget-private hotkey table.

Focused-node key capture is automatic: the focused node receives `KeyDown(target)` events before any unfocused dispatch. A text input that wants to consume Tab needs to declare so via `App::handles_tab_for(target)` (until v0.7.6 lands focused-key priority).

## What you don't get

These would be symmetry violations and aren't part of the kit:

- **No stock-only fields on `El`.** Every public field/builder method is yours too.
- **No library-side `match` on `Kind::*`.** The decorative variants are inspector tags. The structural ones (`Group`, `Spacer`, `Divider`, `Scroll`, `VirtualList`, `Inlines`, `HardBreak`, `Custom`, `Text`) earn their place — they affect layout/event semantics — and apply to your widget the same way they apply to stock.
- **No reaching past the runner.** The `Runner` in each backend crate consumes `DrawOp` and `UiState`; widgets produce `El` trees. There's no widget API that pokes the runner directly.

## The dogfood test

A widget passes the kit if it can be written using *only* the items above. The compiler can't enforce this — the API is open. The convention is enforced by `widgets/button.rs`, `widgets/badge.rs`, `widgets/card.rs`: each is a tight composition of public surface, no internals.

If you find yourself wanting a feature that requires reaching past this kit, that's a signal to **add the feature to the kit** rather than carving an exception. Open an issue or rev `widget_kit.md`. The point of the symmetry invariant is that the library is a substrate, not a library of fixed components.

## Status

- v0.7.5 — kit defined and dogfooded by stock widgets. `widget_state` typed bucket landed.
- v0.7.6 — input plumbing (mouse-up, drag, secondary click, character/IME text, focused-key priority).
- v0.8 — text_input / text_area widgets, dogfooded against this kit. Expect the kit to grow one item: cosmic-text Buffer access for widget-side glyph hit-testing.
- v0.9 — anchored popovers + `context_menu` / `dropdown` helpers. Expect another kit growth: anchor anchoring API.
