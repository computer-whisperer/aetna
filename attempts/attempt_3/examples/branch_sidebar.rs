//! branch_sidebar — port of whisper-git's branch sidebar as a static
//! visual fixture.
//!
//! Goal: see whether attempt_3's grammar can express the same visual
//! structure as `whisper-git/src/views/branch_sidebar.rs` (2214 LoC of
//! hand-rolled vulkano vertex emission + event handling) at a fraction
//! of the line count, with comparable visual quality.
//!
//! What's in scope: the static layout — filter bar, section headers
//! with collapse chevrons and counts, branch rows with current-branch
//! accent stripe, hover/focus row highlight, ahead/behind counts, tags,
//! stashes with timestamps.
//!
//! Out of scope: actual filter handling, keyboard navigation, scrolling,
//! context menus, focus order. attempt_3 has no event runtime yet.
//!
//! Run: `./tools/render branch_sidebar`

use attempt_3::*;

fn sidebar() -> El {
    column([
        filter_bar("Filter branches…"),
        section("LOCAL", 5, true, [
            branch_row("main", true,  Some((0, 0))),
            branch_row("feature/ui-lib-demo", false, Some((3, 0))),
            branch_row("vulkan-render-cleanup", false, Some((1, 2))),
            branch_row("agentic-inspector", false, None),
            branch_row("topic/staging-well-rework", false, Some((0, 14))),
        ]).hovered_at(2), // hover highlight on the third row
        section_remote("origin", true, [
            branch_row_remote("main", Some((0, 0))),
            branch_row_remote("feature/ui-lib-demo", Some((3, 0))),
            branch_row_remote("hotfix/2026-04", Some((0, 1))),
            branch_row_remote("dependabot/cargo/serde-2", None),
        ]),
        section("TAGS", 3, true, [
            tag_row("v0.3.0"),
            tag_row("v0.2.4"),
            tag_row("v0.2.3"),
        ]),
        section("STASHES", 2, true, [
            stash_row(0, "WIP on main: rework filter bar", "2h ago"),
            stash_row(1, "Untracked changes after rebase", "yesterday"),
        ]),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_SM)
    .width(Size::Fixed(280.0))
    .height(Size::Fill(1.0))
    .fill(Color::token("bg-sidebar", 11, 12, 16, 255))
    .stroke(tokens::BORDER)
}

// ---- Filter bar ---------------------------------------------------------
//
// shadcn-flavored input look: muted surface, rounded, single line,
// muted placeholder text. No event runtime → static.

fn filter_bar(placeholder: &str) -> El {
    row([
        text("⌕").muted().small(),
        text(placeholder).muted(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .height(Size::Fixed(28.0))
    .width(Size::Fill(1.0))
    .fill(tokens::BG_MUTED)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
}

// ---- Section header + body ---------------------------------------------
//
// `section(label, count, expanded, rows)` produces:
//   ▼ LABEL (n)        <- header (collapsed: ▶, no body)
//   row 1
//   row 2
//   …

fn section<I, E>(label: &'static str, count: usize, expanded: bool, rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = vec![section_header(label, count, expanded)];
    if expanded {
        children.extend(rows.into_iter().map(Into::into));
    }
    column(children)
        .gap(2.0)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn section_header(label: &'static str, count: usize, expanded: bool) -> El {
    let chevron = if expanded { "▼" } else { "▶" };
    row([
        text(chevron).muted().xsmall(),
        text(label).muted().xsmall().semibold(),
        spacer(),
        text(format!("{count}")).muted().xsmall(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_SM, tokens::SPACE_XS))
    .height(Size::Fixed(22.0))
    .width(Size::Fill(1.0))
}

// ---- Remote section: nested per-remote chevron ------------------------
//
// In whisper-git, the REMOTE section can have multiple remotes, each
// with its own collapsible header. For the fixture we show one remote
// with the single-level structure; multi-remote would just be more of
// the same section() calls.

fn section_remote<I, E>(remote: &'static str, expanded: bool, rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = vec![
        section_header("REMOTE", 1, true),
        remote_header(remote, 4, expanded),
    ];
    if expanded {
        children.extend(rows.into_iter().map(Into::into));
    }
    column(children)
        .gap(2.0)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn remote_header(remote: &'static str, count: usize, expanded: bool) -> El {
    let chevron = if expanded { "▾" } else { "▸" };
    row([
        text(chevron).muted().xsmall(),
        text(remote).muted().small(),
        spacer(),
        text(format!("{count}")).muted().xsmall(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_LG, 0.0))
    .height(Size::Fixed(22.0))
    .width(Size::Fill(1.0))
}

// ---- Branch rows -------------------------------------------------------

fn branch_row(name: &'static str, is_current: bool, ahead_behind: Option<(usize, usize)>) -> El {
    let mut row_children: Vec<El> = Vec::new();

    // Accent stripe (3px) on the left for current branch; otherwise an
    // invisible 3px placeholder so non-current rows align with current ones.
    row_children.push(
        spacer()
            .width(Size::Fixed(3.0))
            .height(Size::Fixed(20.0))
            .fill(if is_current { tokens::PRIMARY } else { tokens::PRIMARY.with_alpha(0) }),
    );

    // Branch name; current is foreground, others muted.
    row_children.push(if is_current {
        text(name).bold()
    } else {
        text(name).muted()
    });

    row_children.push(spacer());

    if let Some((ahead, behind)) = ahead_behind {
        if ahead + behind > 0 {
            row_children.push(ahead_behind_badge(ahead, behind));
        }
    }

    let row_el = row(row_children)
        .gap(tokens::SPACE_SM)
        .padding(Sides::xy(tokens::SPACE_SM, 0.0))
        .height(Size::Fixed(24.0))
        .width(Size::Fill(1.0));

    // Subtle hover/current background; default rows have no fill.
    if is_current {
        row_el.fill(tokens::BG_MUTED).radius(tokens::RADIUS_SM)
    } else {
        row_el
    }
}

fn branch_row_remote(name: &'static str, ahead_behind: Option<(usize, usize)>) -> El {
    let mut row_children: Vec<El> = vec![
        text(name).muted().small(),
        spacer(),
    ];
    if let Some((ahead, behind)) = ahead_behind && ahead + behind > 0 {
        row_children.push(ahead_behind_badge(ahead, behind));
    }
    row(row_children)
        .gap(tokens::SPACE_SM)
        .padding(Sides::xy(tokens::SPACE_XL, 0.0))
        .height(Size::Fixed(22.0))
        .width(Size::Fill(1.0))
}

fn ahead_behind_badge(ahead: usize, behind: usize) -> El {
    let mut parts: Vec<El> = Vec::new();
    if ahead > 0 {
        parts.push(text(format!("↑{ahead}")).color(tokens::SUCCESS).xsmall());
    }
    if behind > 0 {
        parts.push(text(format!("↓{behind}")).color(tokens::WARNING).xsmall());
    }
    row(parts).gap(tokens::SPACE_XS).width(Size::Hug)
}

// ---- Tag rows ----------------------------------------------------------

fn tag_row(name: &'static str) -> El {
    row([
        text("🏷").xsmall(),
        text(name).muted().small(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .height(Size::Fixed(22.0))
    .width(Size::Fill(1.0))
}

// ---- Stash rows --------------------------------------------------------

fn stash_row(idx: usize, message: &'static str, when: &'static str) -> El {
    column([
        row([
            text(format!("stash@{{{idx}}}")).color(tokens::INFO).xsmall(),
            spacer(),
            text(when).muted().xsmall(),
        ])
        .gap(tokens::SPACE_SM),
        text(message).muted().small(),
    ])
    .gap(tokens::SPACE_XS)
    .padding(Sides::xy(tokens::SPACE_SM, tokens::SPACE_XS))
    .height(Size::Hug)
    .width(Size::Fill(1.0))
}

// ---- Static-state hint: hover row N -----------------------------------
//
// The branch list has no event runtime, so to demonstrate the hover
// highlight we mark a specific row as hovered. This is the same
// pattern as button_states.rs — fixtures construct multiple visual
// states by setting `state` on the elements that should show them.

trait HoveredAt {
    fn hovered_at(self, idx: usize) -> El;
}

impl HoveredAt for El {
    fn hovered_at(mut self, idx: usize) -> El {
        // children[0] is the section header; +1 to skip it.
        let body_idx = idx + 1;
        if body_idx < self.children.len() {
            let original = self.children[body_idx].clone();
            self.children[body_idx] = original
                .fill(tokens::BG_MUTED.with_alpha(120))
                .radius(tokens::RADIUS_SM)
                .with_state(InteractionState::Hover);
        }
        self
    }
}

fn main() -> std::io::Result<()> {
    let mut root = sidebar();

    let viewport = Rect::new(0.0, 0.0, 280.0, 720.0);
    let bundle = render_bundle(&mut root, viewport, Some("attempts/attempt_3/src"));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "branch_sidebar")?;
    for p in &written {
        println!("wrote {}", p.display());
    }
    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }
    Ok(())
}
