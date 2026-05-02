# Attempt 2 — Design Premise

This is a from-scratch attempt at the LLM-native UI library after `attempts/attempt_1/` showed the shape of the problem but indexed on the wrong priorities. Read attempt_1's `DESIGN_RATIONALE.md` and `CURRENT_ASSESSMENT.md` for the original investigation; this doc captures the updated thesis that motivates restarting.

## What's different from attempt_1

Attempt_1's premise:

> "egui's *iterative* LLM-polish loop is bad; build a retained semantic tree + artifact pipeline so polish iteration converges faster."

Attempt_2's premise:

> "The LLM should produce polished UI on the **first shot**. The grammar is the wedge; the artifact loop is a backstop."

That reframing changes priorities throughout.

## The three load-bearing premises

### 1. The bar is one-shot polish, not iterative convergence

The actual gap between LLM-authored egui and LLM-authored React isn't that egui can't be polished — it's that the LLM can't get there in one shot. "Center the text in this button" in egui requires several rounds of convincing the model the text isn't centered. In React, the model nails it the first time, because the training distribution is full of polished React and the conventional component (`<Button>`) does the right thing without the LLM having to construct centering.

The library's primary job is therefore to make the *minimum LLM output* the *correct LLM output*. `Button("Commit")` with no variant, no size, no padding, no alignment specified — should render as a polished default button. The polished output should be the zero-effort output. Customization is opt-in, not required.

That moves priority away from the inspection/lint/responsive-tape pipeline (still useful, still a backstop) and toward grammar-level decisions: vocabulary parity with what LLMs already type fluently, polished defaults, intent-level layout primitives.

### 2. LLM onboarding cost is a first-class design constraint

A cold LLM session must be able to read this library's source and start producing correct, polished UI without burning a large fraction of its context window learning the system. If the API requires 100k tokens of learning before the LLM can write a button, we've lost ground compared to React/Tailwind, where that knowledge is already in the model's weights.

Concrete implications:

- **Total public-API surface stays small** — small enough that a session can read all of it if it wants. Aim for the public API to fit in low thousands of LoC, not tens of thousands.
- **One canonical way to do each thing.** Choice paralysis is wasted tokens. If there are two ways to make a primary button, the LLM has to pick, and will sometimes pick the wrong one or invent a third.
- **Components are file-local.** `src/components/button.rs` should contain the whole story of what a button is, its variants, and how it's used. Avoid chains of trait impls across files for one component — LLMs don't follow those reliably.
- **No required ceremony in user code.** No `theme` plumbing on every call. No `src_here!` peppered everywhere. No three-file `Msg`/`view`/`update` synchronization just to add a hover state. The example file should look like the LLM's natural output.
- **Doc comments are load-bearing.** `///` is in the LLM training distribution and gets consumed natively. Spend tokens on doc comments — the LLM reads them and uses them.

### 3. Layout in Rust source — and Rust source is the documentation

Layout stays in plain Rust. No JSX-like macro DSL. No external markup. Fluent builders, plain function calls, and a small number of types.

The reason isn't aesthetic — it's that Rust source is *readable by LLMs without ceremony*. A model can grep for `Button`, open `button.rs`, read the doc comments and the impl, and know how to use it. That's a major advantage over TypeScript libraries with deep type-system tricks where understanding requires synthesizing across multiple `.d.ts` files. We deliberately keep this property:

- The library source should be a teaching surface. Reading it should leave a session ready to write idiomatic code.
- No clever proc-macro magic that hides the actual code structure. If we use macros, they should expand to obvious Rust.
- Types should be concrete and inspectable, not parameterized into oblivion.
- Prefer obvious over clever. A `pub fn button(label: &str) -> Button` is worth more than a one-line tour-de-force.

## Vocabulary parity with shadcn/Tailwind

Aggressively shadow the shadcn/Tailwind vocabulary so that LLM training transfers verbatim:

- Token names: `bg-card`, `bg-muted`, `text-foreground`, `text-muted-foreground`, `border`, `ring`, `radius-md`, `gap-4`, etc. — exposed in Rust as `theme.bg.card`, `theme.text.muted_foreground`, `theme.gap.md`, etc.
- Variant names: `primary`, `secondary`, `ghost`, `destructive`, `outline`, `link`.
- Component names: `Card`, `Button`, `Badge`, `Dialog`, `Tooltip`, `Tabs`, `Separator`.
- Slot names: `header`, `body`, `footer`, `title`, `description`, `actions`.
- Layout names: `flex`, `grid`, `gap`, `padding`, `stack`.

This is borrowing-priors, not re-implementing CSS. We reject the runtime semantics (no cascade, no className strings, no JSX, no hooks) but adopt the surface vocabulary. Where the names diverge between Tailwind and shadcn, prefer shadcn — it's the more polished, opinionated subset.

## What attempt_1 had right and we keep

- Sizing intents (`Fill`, `Hug`, `Fixed`) over raw pixel hints. Pixel arithmetic should never appear in user code.
- Typed token vocabulary over magic numbers. Tighten naming to shadcn/Tailwind parity.
- Render-command IR between the layout pass and the backend. Lets us swap SVG → real renderer later.
- SVG output as the cheap deterministic fixture backend (see Renderer section).

## What attempt_1 got wrong, and we don't repeat

- `src_here!` ceremony at every call site. Source-mapping should be invisible to the user — derived from call paths, attribute macros on the build function, or just dropped if we can't do it cleanly.
- `theme` threaded through every component call. Theme should be implicit (thread-local context, or implicit from a single `view!` entry, or an opt-in builder for users who want to override).
- Lint signal that flags library-internal raw values as user errors. If we keep lint in attempt_2, it must distinguish user-introduced from library-leaked values.
- Heavy upfront commitment to Elm/Iced messages before testing it against actual LLM authoring. Keep open the option of per-component lightweight state for things like hover, dropdown-open, focus.
- Building generic retained-UI plumbing first and grammar/polish second. Reverse the order: grammar and polish defaults first, plumbing only as needed.
- Verbose `vec![...]` children. Use `IntoIterator`/`IntoNode` polymorphism so a single child, an array, an `Option`, or a `Vec` all compose naturally.

## Renderer plan

Stay on SVG output for fixture generation until grammar + polish are convincing. Reasons:

- Cheap. Generate a hundred fixtures in a millisecond.
- Deterministic. Reproducible artifacts for comparing across LLM sessions and across runs.
- Readable. SVG is text; the LLM can read its own output back and reason about it.
- Avoids conflating polish bugs with renderer bugs while we're tuning the grammar.

The whisper-git substrate (vulkano + fontdue + winit, ~3.5k LoC) lives in a sibling worktree and is available to extract when needed. Don't extract until SVG actively misleads about the polish question — at which point swapping the backend behind the render-command IR should be a contained job, not a rewrite.

## Initial scope (what to build first)

- **One curated dark theme.** Tokens named after Tailwind/shadcn (`bg.card`, `text.muted_foreground`, `border`, `radius.md`, etc.).
- **Layout primitives.** `column`, `row`, `stack`, with `gap`, `padding`, `align`. No width/height arithmetic in user code.
- **Components.** `Card`, `Button`, `Badge`, `Text`. Each in its own file, each with polished defaults, each with shadcn-style variant names. Each file should be readable end-to-end as a learning surface.
- **One real example.** Build a single moderately rich UI (e.g., a settings panel or a simplified `branch_sidebar` from whisper-git) that demonstrates the API can do a real screen polished in one shot.
- **SVG backend.** Carry forward the IR/SVG approach from attempt_1, simplified.

Defer until the above is convincing: virtual lists, modals, command palette, motion presets, lint reports, responsive tapes, focus traces, point-pick, edit protocol. These are good ideas; none of them help with one-shot polish.

## Validation question

A fresh LLM session, given a brief prompt and this library's source on disk, should be able to produce a polished SVG fixture in one shot for a moderate UI request — for example, *"a settings page with three sections, a couple of toggles, and a primary save button"* — without producing visibly broken layout or visibly off-center text.

Until that happens reliably across multiple LLM models and multiple prompts, the library hasn't earned the rest of the roadmap.
