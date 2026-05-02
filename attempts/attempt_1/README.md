# ui_lib_demo

A tiny MVP sketch of an **LLM-native retained UI library**: semantic components,
typed theme tokens, explicit layout primitives, render-command output, SVG/headless
artifacts, source-mapped node IDs, and simple motion/contact-sheet generation.

This is intentionally not a full renderer. It is the first thin slice of the
API shape Mavis proposed: make the *pretty, inspectable component grammar* the
main product, with rendering as a backend.

## Run

```bash
cd /home/christian/mavis_data/ui_lib_demo
cargo run --example git_dashboard
```

Outputs land in `out/`:

- `git_dashboard.svg` — static headless render
- `git_dashboard.inspect.txt` — semantic/layout/source/action tree
- `git_dashboard.lint.txt` — token/raw-value/duplicate-id report
- `git_dashboard.motion.svg` — animation contact sheet for a modal preset
- `git_dashboard.responsive.svg` — same fixture rendered at multiple widths

## Design rationale

See [`DESIGN_RATIONALE.md`](DESIGN_RATIONALE.md) for the investigation notes: why egui/React differ for LLMs, the intended component grammar, inspection artifacts, motion/contact-sheet idea, and open questions.

See [`OPUS_REVIEW.md`](OPUS_REVIEW.md) for an independent Claude Opus critique, [`NEXT_STEPS.md`](NEXT_STEPS.md) for the distilled implementation priorities, [`ITERATION_LOG.md`](ITERATION_LOG.md) for what changed between sketches, and [`CURRENT_ASSESSMENT.md`](CURRENT_ASSESSMENT.md) for the post-v2 judgment.

## Design goals shown

- Retained semantic `Node` tree with stable IDs and roles.
- Sizing intents (`Fixed`, `Fill`, `Hug`) instead of one-off `desired_w/h` hints.
- Tiny source-map macro (`src_here!`) so example-created nodes point back to the caller.
- Typed message actions on nodes (`El<Msg>`, `.on_action(Msg::...)`) as a first Elm/Iced-style step.
- Typed `Theme` tokens for colors, spacing, radius, shadows, text, and motion.
- High-level pretty components: `Card`, `Button`, `Badge`, `Sidebar`, `Toolbar`,
  `VirtualList`, `Toast`.
- Explicit layout: `Column`, `Row`, `Overlay`, `VirtualList`.
- Backend emits `RenderCommand`s, then an SVG renderer turns those into artifacts.
- Inspector can map visual nodes back to call sites where the example used `src_here!`.
- Motion is semantic (`MotionPreset::ModalEnter`) and inspectable via contact sheet.
- Lint and responsive-tape artifacts expose token use, duplicate IDs, raw values, and narrow-width failures.
