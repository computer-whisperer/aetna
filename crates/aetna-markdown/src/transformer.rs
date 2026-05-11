//! Walk pulldown-cmark events into an Aetna `El` tree.

use aetna_core::prelude::*;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options as CmarkOptions, Parser,
    Tag, TagEnd,
};

/// Optional markdown extensions that can change rendered output.
///
/// [`md`] uses `Default`, which keeps output conservative while still
/// enabling the GFM features Aetna renders directly today: tables,
/// strikethrough, and task lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Replace ASCII punctuation with typographic punctuation during
    /// parsing (`--`, `---`, `...`, straight quotes).
    pub smart_punctuation: bool,
    /// Render GFM alert blockquotes (`[!NOTE]`, `[!WARNING]`, ...)
    /// through Aetna's `alert` widget instead of a plain blockquote.
    pub gfm_alerts: bool,
    /// Parse `$...$` and `$$...$$` as native Aetna math instead of
    /// leaving pulldown-cmark's math extension disabled.
    pub math: bool,
}

impl MarkdownOptions {
    pub fn smart_punctuation(mut self, enabled: bool) -> Self {
        self.smart_punctuation = enabled;
        self
    }

    pub fn gfm_alerts(mut self, enabled: bool) -> Self {
        self.gfm_alerts = enabled;
        self
    }

    pub fn math(mut self, enabled: bool) -> Self {
        self.math = enabled;
        self
    }
}

/// Render a markdown document as an Aetna `El`.
///
/// The result is a `column([...])` of block-level Aetna widgets — the
/// same shape an author would have hand-written. See the crate-level
/// docs for the supported subset and the deferred features.
pub fn md(input: &str) -> El {
    md_with_options(input, MarkdownOptions::default())
}

/// Render a markdown document with explicit extension options.
pub fn md_with_options(input: &str, options: MarkdownOptions) -> El {
    // GFM tables, task lists, and `~~strike~~` all have direct widget-kit
    // / inline-modifier analogs. Footnotes and math stay off until the
    // markdown surface grows first-class support for references and TeX.
    let mut parser_options = CmarkOptions::ENABLE_TABLES
        | CmarkOptions::ENABLE_STRIKETHROUGH
        | CmarkOptions::ENABLE_TASKLISTS;
    if options.smart_punctuation {
        parser_options |= CmarkOptions::ENABLE_SMART_PUNCTUATION;
    }
    if options.gfm_alerts {
        parser_options |= CmarkOptions::ENABLE_GFM;
    }
    if options.math {
        parser_options |= CmarkOptions::ENABLE_MATH;
    }

    let parser = Parser::new_ext(input, parser_options);
    let mut walker = Walker::new(options);
    for event in parser {
        walker.handle(event);
    }
    walker.finish()
}

/// Block-level frame on the parser's open-container stack. `Walker`
/// pops these on the matching `End` event and folds the collected
/// child content into a single Aetna widget.
enum Frame {
    /// Open `<p>` — accumulates inline runs.
    Paragraph(Vec<El>),
    /// Open `<h1..h6>` — accumulates inline runs.
    Heading(HeadingLevel, Vec<El>),
    /// Open `<blockquote>` — accumulates block children.
    BlockQuote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<El>,
    },
    /// Open `<ul>` / `<ol>` — collects items as nested block lists.
    List {
        /// `None` ↔ bullet list, `Some(start)` ↔ ordered list starting
        /// at `start` (CommonMark allows non-1 starts).
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    /// Open `<li>` — accumulates one item's block children plus an
    /// optional GFM task marker.
    Item {
        blocks: Vec<El>,
        task_checked: Option<bool>,
    },
    /// Open `<pre><code>` — accumulating verbatim text. The optional
    /// `lang` is the fenced info string (`` ```rust `` → `Some("rust")`,
    /// indented blocks and bare `` ``` `` fences → `None`); when
    /// highlighting is enabled and the lang resolves to a known syntax,
    /// the close handler tokenises the body, otherwise it emits the
    /// existing plain-mono `code_block(...)`.
    CodeBlock { lang: Option<String>, text: String },
    /// Open `<a>` — accumulates inline children that share its URL.
    /// The URL is applied to each text run on close (not via inline
    /// style flags) so a link spanning multiple text events groups
    /// correctly under one href in the painter.
    Link(String, Vec<El>),
    /// Open `<img>` — accumulates alt text and keeps the destination
    /// for placeholder rendering. Image content loading is deferred
    /// (see crate docs).
    Image {
        alt: String,
        dest_url: String,
        title: String,
    },
    /// Open `<table>` — collects the header and body rows the matching
    /// `End(Table)` folds into a `widgets::table` block.
    Table {
        /// Per-column alignments (`:---`, `:---:`, `---:`), applied to
        /// header and body cell text.
        alignments: Vec<Alignment>,
        /// Header row, populated on `TagEnd::TableHead`. `None` if the
        /// document somehow ends a table without a header (CommonMark
        /// + GFM always emits one but this stays defensive).
        head: Option<Vec<El>>,
        /// Body rows, accumulated on each `TagEnd::TableRow`.
        body: Vec<Vec<El>>,
    },
    /// Open `<thead>` — accumulates the header cells.
    TableHead(Vec<El>),
    /// Open `<tr>` (body row) — accumulates the row's cells.
    TableRow(Vec<El>),
    /// Open `<th>` / `<td>`. `in_header` toggles the header-styled
    /// `table_head(...)` builder on close vs. the body-styled
    /// `table_cell(...)`.
    TableCell {
        runs: Vec<El>,
        in_header: bool,
        alignment: Alignment,
    },
}

struct ListItem {
    content: El,
    task_checked: Option<bool>,
}

/// Inline styling currently in effect for new text runs.
///
/// Markdown inline tags (`*em*`, `**strong**`, `~~strike~~`) can nest;
/// each pair pushes / pops a depth counter. The Code and Link cases
/// are scoped through their own frames in `Walker::stack` rather than
/// as flags here, since they carry data (the run's `code_role` shape
/// and the link URL respectively).
#[derive(Default)]
struct InlineState {
    italic_depth: u32,
    bold_depth: u32,
    strike_depth: u32,
}

impl InlineState {
    fn apply(&self, mut el: El) -> El {
        if self.bold_depth > 0 {
            el = el.bold();
        }
        if self.italic_depth > 0 {
            el = el.italic();
        }
        if self.strike_depth > 0 {
            el = el.strikethrough();
        }
        el
    }
}

struct Walker {
    options: MarkdownOptions,
    /// Open block-level frames + open `<a>` / `<img>` containers,
    /// innermost last. `<a>` and `<img>` are stack-tracked rather than
    /// stored as inline-state flags because they own the text events
    /// between Start/End and need to fold them into their own El on
    /// close.
    stack: Vec<Frame>,
    /// Inline-style flags for upcoming text events.
    inline: InlineState,
    /// Top-level blocks collected outside any open frame.
    root: Vec<El>,
}

impl Walker {
    fn new(options: MarkdownOptions) -> Self {
        Self {
            options,
            stack: Vec::new(),
            inline: InlineState::default(),
            root: Vec::new(),
        }
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(text) => self.text(text.into_string()),
            Event::Code(text) => self.code_span(text.into_string()),
            Event::SoftBreak => self.text(" ".to_string()),
            Event::HardBreak => {
                self.ensure_inline_frame();
                self.push_inline(hard_break());
            }
            Event::Rule => self.push_block(divider()),
            Event::InlineMath(text) => self.inline_math(text.into_string()),
            Event::DisplayMath(text) => self.display_math(text.into_string()),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(checked) => self.task_list_marker(checked),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.stack.push(Frame::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.stack.push(Frame::Heading(level, Vec::new())),
            Tag::BlockQuote(kind) => self.stack.push(Frame::BlockQuote {
                kind: kind.filter(|_| self.options.gfm_alerts),
                blocks: Vec::new(),
            }),
            Tag::List(start) => self.stack.push(Frame::List {
                start,
                items: Vec::new(),
            }),
            Tag::Item => self.stack.push(Frame::Item {
                blocks: Vec::new(),
                task_checked: None,
            }),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // The info string can carry attributes after a
                        // space (`rust ignore`); first token is the
                        // language tag, anything else we don't speak.
                        let token = info.split_whitespace().next().unwrap_or("");
                        if token.is_empty() {
                            None
                        } else {
                            Some(token.to_string())
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                self.stack.push(Frame::CodeBlock {
                    lang,
                    text: String::new(),
                });
            }
            Tag::Emphasis => self.inline.italic_depth += 1,
            Tag::Strong => self.inline.bold_depth += 1,
            Tag::Strikethrough => self.inline.strike_depth += 1,
            Tag::Link { dest_url, .. } => {
                self.stack
                    .push(Frame::Link(dest_url.into_string(), Vec::new()));
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                // Alt text accumulates through inline events while the
                // image frame is open; on End we fold into a placeholder.
                self.stack.push(Frame::Image {
                    alt: String::new(),
                    dest_url: dest_url.into_string(),
                    title: title.into_string(),
                });
            }
            Tag::Table(alignments) => {
                self.stack.push(Frame::Table {
                    alignments,
                    head: None,
                    body: Vec::new(),
                });
            }
            Tag::TableHead => self.stack.push(Frame::TableHead(Vec::new())),
            Tag::TableRow => self.stack.push(Frame::TableRow(Vec::new())),
            Tag::TableCell => {
                // Header vs body is decided by what the topmost open
                // frame is at cell-start time: a `TableHead` parent
                // means this cell renders with the header recipe.
                let in_header = matches!(self.stack.last(), Some(Frame::TableHead(_)));
                let alignment = self.next_table_cell_alignment();
                self.stack.push(Frame::TableCell {
                    runs: Vec::new(),
                    in_header,
                    alignment,
                });
            }
            // Footnote definitions, definition lists, and subscript /
            // superscript are deferred — open a paragraph frame so any
            // inline text between Start and End ends up captured (and
            // dropped when we pop).
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::Subscript
            | Tag::Superscript => {
                self.stack.push(Frame::Paragraph(Vec::new()));
            }
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                if let Some(Frame::Paragraph(inlines)) = self.stack.pop() {
                    // Empty paragraph: pulldown-cmark wraps inline
                    // images in their own paragraph, so once the image
                    // pops out as a block the wrapping paragraph is
                    // empty. Skip emission for that case (and for any
                    // other zero-run paragraph) so the document
                    // doesn't carry phantom empty blocks.
                    if inlines.is_empty() {
                        return;
                    }
                    let block = build_paragraph(inlines);
                    self.push_block(block);
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading(level, inlines)) = self.stack.pop() {
                    let block = build_heading(level, inlines);
                    self.push_block(block);
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::BlockQuote { kind, blocks }) = self.stack.pop() {
                    self.push_block(build_blockquote(kind, blocks));
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { start, items }) = self.stack.pop() {
                    let block = build_list(start, items);
                    self.push_block(block);
                }
            }
            TagEnd::Item => {
                // Tight-list items in pulldown-cmark omit the wrapping
                // `Paragraph` events — text events arrive directly
                // under `Item`. We lazily push a `Paragraph` frame on
                // the first inline event under such an item (see
                // `ensure_inline_frame`). Drain any such open
                // paragraphs into the item's blocks before closing.
                while matches!(self.stack.last(), Some(Frame::Paragraph(_))) {
                    if let Some(Frame::Paragraph(inlines)) = self.stack.pop()
                        && !inlines.is_empty()
                    {
                        let block = build_paragraph(inlines);
                        self.push_block(block);
                    }
                }
                if let Some(Frame::Item {
                    blocks,
                    task_checked,
                }) = self.stack.pop()
                {
                    let item_el = build_list_item(blocks);
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem {
                            content: item_el,
                            task_checked,
                        });
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::CodeBlock { lang, text }) = self.stack.pop() {
                    self.push_block(build_code_block(lang.as_deref(), text));
                }
            }
            TagEnd::Emphasis => {
                self.inline.italic_depth = self.inline.italic_depth.saturating_sub(1)
            }
            TagEnd::Strong => self.inline.bold_depth = self.inline.bold_depth.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.inline.strike_depth = self.inline.strike_depth.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some(Frame::Link(url, inlines)) = self.stack.pop() {
                    for run in inlines {
                        // Each text leaf inside the `<a>` adopts the
                        // same href so the renderer groups them into
                        // one link for hit-testing.
                        let linked = run.link(url.clone());
                        self.push_inline(linked);
                    }
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Image {
                    alt,
                    dest_url,
                    title,
                }) = self.stack.pop()
                {
                    let placeholder = build_image_placeholder(&alt, &dest_url, &title);
                    if self.in_inline_container() {
                        self.push_inline(placeholder);
                    } else {
                        self.push_block(placeholder);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table { head, body, .. }) = self.stack.pop() {
                    self.push_block(build_table(head, body));
                }
            }
            TagEnd::TableHead => {
                if let Some(Frame::TableHead(cells)) = self.stack.pop() {
                    let header_row = table_row(cells);
                    if let Some(Frame::Table { head, .. }) = self.stack.last_mut() {
                        *head = Some(vec![header_row]);
                    }
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::TableRow(cells)) = self.stack.pop() {
                    let body_row = table_row(cells);
                    if let Some(Frame::Table { body, .. }) = self.stack.last_mut() {
                        body.push(vec![body_row]);
                    }
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::TableCell {
                    runs,
                    in_header,
                    alignment,
                }) = self.stack.pop()
                {
                    let cell = build_table_cell(runs, in_header, alignment);
                    match self.stack.last_mut() {
                        Some(Frame::TableHead(cells)) | Some(Frame::TableRow(cells)) => {
                            cells.push(cell);
                        }
                        _ => {}
                    }
                }
            }
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::Subscript
            | TagEnd::Superscript => {
                // Drain the matching ignored frame from `start`.
                self.stack.pop();
            }
        }
    }

    fn text(&mut self, s: String) {
        // CodeBlock receives raw text; everything else flows through
        // an inline buffer with the active style applied.
        if let Some(Frame::CodeBlock { text: buf, .. }) = self.stack.last_mut() {
            buf.push_str(&s);
            return;
        }
        if let Some(Frame::Image { alt, .. }) = self.stack.last_mut() {
            alt.push_str(&s);
            return;
        }
        self.ensure_inline_frame();
        let run = self.inline.apply(text(s));
        self.push_inline(run);
    }

    fn code_span(&mut self, s: String) {
        // Inline code: `text(...).code()` carries the code role, which
        // theme application maps to mono + foreground. Strikethrough
        // / italic / bold can wrap a code span in CommonMark, so the
        // current InlineState still applies on top of `.code()`.
        if matches!(self.stack.last(), Some(Frame::CodeBlock { .. })) {
            // Inside a fenced code block, `Event::Code` shouldn't
            // arrive — but if it does, treat as raw text.
            if let Some(Frame::CodeBlock { text: buf, .. }) = self.stack.last_mut() {
                buf.push_str(&s);
            }
            return;
        }
        if let Some(Frame::Image { alt, .. }) = self.stack.last_mut() {
            alt.push_str(&s);
            return;
        }
        self.ensure_inline_frame();
        let run = self.inline.apply(text(s).code());
        self.push_inline(run);
    }

    fn inline_math(&mut self, source: String) {
        let expr = parse_tex_or_error(&source);
        self.ensure_inline_frame();
        self.push_inline(math_inline(expr));
    }

    fn display_math(&mut self, source: String) {
        let expr = parse_tex_or_error(&source);
        self.push_block(math_block(expr));
    }

    /// Lazily open a `Paragraph` frame so an inline event arriving
    /// directly under an `Item` (CommonMark's tight-list shape — no
    /// wrapping `<p>`) has somewhere to land. The matching
    /// `TagEnd::Item` drains any such open paragraph back into the
    /// item before closing it. Table cells already accept inlines
    /// directly so they don't need the lazy paragraph.
    fn ensure_inline_frame(&mut self) {
        match self.stack.last() {
            Some(
                Frame::Paragraph(_)
                | Frame::Heading(_, _)
                | Frame::Link(_, _)
                | Frame::TableCell { .. },
            ) => {}
            Some(Frame::Item { .. }) => self.stack.push(Frame::Paragraph(Vec::new())),
            _ => {}
        }
    }

    /// Append a block-level El to the innermost block container, or
    /// the root if none is open.
    fn push_block(&mut self, el: El) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::BlockQuote { blocks, .. } | Frame::Item { blocks, .. } => {
                    blocks.push(el);
                    return;
                }
                _ => {}
            }
        }
        self.root.push(el);
    }

    /// Append an inline-level El to the innermost inline-accepting
    /// container (paragraph, heading, link, table cell). Drops if
    /// none is open — stray text outside a paragraph should not be
    /// reachable from a well-formed pulldown-cmark stream.
    fn push_inline(&mut self, el: El) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Paragraph(runs)
                | Frame::Heading(_, runs)
                | Frame::Link(_, runs)
                | Frame::TableCell { runs, .. } => {
                    runs.push(el);
                    return;
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> El {
        // Defensive: a malformed input could leave open frames. Drain
        // anything still on the stack into root order so we still
        // produce a valid El rather than panicking.
        while let Some(frame) = self.stack.pop() {
            match frame {
                Frame::Paragraph(runs) => self.root.push(build_paragraph(runs)),
                Frame::Heading(level, runs) => self.root.push(build_heading(level, runs)),
                Frame::BlockQuote { kind, blocks } => {
                    self.root.push(build_blockquote(kind, blocks))
                }
                Frame::List { start, items } => self.root.push(build_list(start, items)),
                Frame::Item { blocks, .. } => self.root.push(build_list_item(blocks)),
                Frame::CodeBlock { lang, text } => {
                    self.root.push(build_code_block(lang.as_deref(), text))
                }
                Frame::Link(_, runs) => {
                    for run in runs {
                        self.root.push(run);
                    }
                }
                Frame::Image {
                    alt,
                    dest_url,
                    title,
                } => self
                    .root
                    .push(build_image_placeholder(&alt, &dest_url, &title)),
                Frame::Table { head, body, .. } => self.root.push(build_table(head, body)),
                Frame::TableHead(_) | Frame::TableRow(_) | Frame::TableCell { .. } => {
                    // Cells / rows whose enclosing table never closed
                    // can't usefully be rendered standalone — drop.
                }
            }
        }
        column(self.root)
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }

    fn task_list_marker(&mut self, checked: bool) {
        for frame in self.stack.iter_mut().rev() {
            if let Frame::Item { task_checked, .. } = frame {
                *task_checked = Some(checked);
                return;
            }
        }
    }

    fn in_inline_container(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(
                Frame::Paragraph(_)
                    | Frame::Heading(_, _)
                    | Frame::Link(_, _)
                    | Frame::TableCell { .. }
            )
        )
    }

    fn next_table_cell_alignment(&self) -> Alignment {
        let index = match self.stack.last() {
            Some(Frame::TableHead(cells)) | Some(Frame::TableRow(cells)) => cells.len(),
            _ => 0,
        };
        self.stack
            .iter()
            .rev()
            .find_map(|frame| {
                if let Frame::Table { alignments, .. } = frame {
                    alignments.get(index).copied()
                } else {
                    None
                }
            })
            .unwrap_or(Alignment::None)
    }
}

/// Build a fenced/indented code block. With the `highlighting` feature
/// and a recognised `lang`, tokenise the body through `syntect` and
/// wrap the styled `text_runs([...])` paragraph in the standard
/// [`code_block_chrome`] surface — palette tokens flow through to
/// paint, so `Theme::aetna_light()` recolours the result without
/// re-rendering the markdown. Otherwise (feature off, no lang,
/// unknown lang) fall through to the plain-mono [`code_block`] path.
fn build_code_block(lang: Option<&str>, text: String) -> El {
    let body = strip_trailing_newline(text);
    #[cfg(feature = "highlighting")]
    if let Some(lang) = lang
        && let Some(syntax) = crate::highlight::find_syntax(lang)
    {
        let runs = crate::highlight::highlight_to_runs(&body, syntax);
        if !runs.is_empty() {
            return code_block_chrome(
                text_runs(runs)
                    .nowrap_text()
                    .font_size(tokens::TEXT_SM.size)
                    .width(Size::Hug)
                    .height(Size::Hug),
            );
        }
    }
    #[cfg(not(feature = "highlighting"))]
    let _ = lang;
    code_block(body)
}

fn parse_tex_or_error(source: &str) -> MathExpr {
    match parse_tex(source) {
        Ok(expr) => expr,
        Err(err) => MathExpr::Error(format!("math parse error at {}: {}", err.byte, err.message)),
    }
}

/// Build a paragraph block. A single plain `text(...)` run can become
/// a `paragraph(...)` (one wrapped string); anything richer collapses
/// to `text_runs([...])`.
fn build_paragraph(runs: Vec<El>) -> El {
    if let Some(plain) = single_plain_text(&runs) {
        return paragraph(plain);
    }
    text_runs(runs)
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Build a heading block. For headings whose only content is plain
/// text, use `h1` / `h2` / `h3` so the result carries the semantic
/// `Kind::Heading` (visible in tree dumps and inspect output). For
/// styled headings we fall back to a heading-roled `text_runs`.
fn build_heading(level: HeadingLevel, runs: Vec<El>) -> El {
    if let Some(plain) = single_plain_text(&runs) {
        return match level {
            HeadingLevel::H1 => h1(plain),
            HeadingLevel::H2 => h2(plain),
            // h4–h6 are rare and Aetna's heading vocabulary stops at
            // h3 — clamp the rest so deep nesting still renders.
            _ => h3(plain),
        };
    }
    let role = match level {
        HeadingLevel::H1 => TextRole::Display,
        HeadingLevel::H2 => TextRole::Heading,
        _ => TextRole::Title,
    };
    text_runs(runs)
        .text_role(role)
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn build_blockquote(kind: Option<BlockQuoteKind>, blocks: Vec<El>) -> El {
    let Some(kind) = kind else {
        return blockquote(blocks);
    };

    let title = match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    };
    let body = match blocks.len() {
        0 => column(Vec::<El>::new()),
        1 => blocks.into_iter().next().unwrap(),
        _ => column(blocks)
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
    };
    let alert = alert([alert_title(title), body]);
    match kind {
        BlockQuoteKind::Note | BlockQuoteKind::Important => alert.info(),
        BlockQuoteKind::Tip => alert.success(),
        BlockQuoteKind::Warning => alert.warning(),
        BlockQuoteKind::Caution => alert.destructive(),
    }
}

fn build_list(start: Option<u64>, items: Vec<ListItem>) -> El {
    match start {
        None if !items.is_empty() && items.iter().all(|item| item.task_checked.is_some()) => {
            task_list(
                items
                    .into_iter()
                    .map(|item| (item.task_checked.unwrap_or(false), item.content)),
            )
        }
        None => bullet_list(items.into_iter().map(|item| item.content)),
        Some(start) => numbered_list_from(start, items.into_iter().map(|item| item.content)),
    }
}

/// Collapse one item's accumulated blocks into a single El. Single
/// block → that block; multiple blocks → wrap in `column`.
fn build_list_item(mut blocks: Vec<El>) -> El {
    if blocks.len() == 1 {
        blocks.pop().unwrap()
    } else {
        column(blocks)
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }
}

/// Build a `widgets::table` block from the parsed header and body
/// rows. Falls back to body-only if no header was emitted.
fn build_table(head: Option<Vec<El>>, body: Vec<Vec<El>>) -> El {
    // `body` came in as `Vec<Vec<El>>` (one outer per row) but each
    // inner Vec is single-element since `TagEnd::TableRow` already
    // built one `table_row(...)` per row. Re-flatten into row-Els.
    let body_rows: Vec<El> = body.into_iter().flatten().collect();
    match head {
        Some(header_rows) => table([table_header(header_rows), table_body(body_rows)]),
        None => table([table_body(body_rows)]),
    }
}

/// Wrap accumulated inline runs into a header-styled or body-styled
/// table cell. Plain-text-only cells flow through `table_head` /
/// `text(...)` so the cell carries the right typography defaults; mixed
/// inline content uses `text_runs([...])` and keeps per-run styling.
fn build_table_cell(runs: Vec<El>, in_header: bool, alignment: Alignment) -> El {
    if in_header {
        let cell = if let Some(plain) = single_plain_text(&runs) {
            table_head(plain)
        } else if runs.is_empty() {
            table_head("")
        } else {
            table_head_el(text_runs(runs).width(Size::Fill(1.0)))
        };
        return apply_table_alignment(cell, alignment);
    }
    if let Some(plain) = single_plain_text(&runs) {
        return apply_table_alignment(table_cell(text(plain)), alignment);
    }
    if runs.is_empty() {
        return apply_table_alignment(table_cell(text("")), alignment);
    }
    apply_table_alignment(
        table_cell(text_runs(runs).width(Size::Fill(1.0))),
        alignment,
    )
}

fn apply_table_alignment(mut el: El, alignment: Alignment) -> El {
    let text_align = match alignment {
        Alignment::None | Alignment::Left => TextAlign::Start,
        Alignment::Center => TextAlign::Center,
        Alignment::Right => TextAlign::End,
    };
    apply_text_align(&mut el, text_align);
    el
}

fn apply_text_align(el: &mut El, text_align: TextAlign) {
    el.text_align = text_align;
    for child in &mut el.children {
        apply_text_align(child, text_align);
    }
}

fn build_image_placeholder(alt: &str, dest_url: &str, title: &str) -> El {
    // Phase 2 doesn't wire image loading. Surface the alt text plus
    // source metadata so the page reads sensibly until image resolution
    // lands; muted + italic so it doesn't look like first-class content.
    let mut label = match (alt.is_empty(), dest_url.is_empty()) {
        (true, true) => "[image]".to_string(),
        (false, true) => format!("[image: {alt}]"),
        (true, false) => format!("[image: {dest_url}]"),
        (false, false) => format!("[image: {alt}] {dest_url}"),
    };
    if !title.is_empty() {
        label.push_str(" \"");
        label.push_str(title);
        label.push('"');
    }
    let mut el = text(label).muted().italic();
    if !dest_url.is_empty() {
        el = el.link(dest_url.to_string());
    }
    el
}

/// Inspect a run vector and return a single plain string if every run
/// is a default-styled `Kind::Text` leaf (no bold, italic, strike,
/// link, code, custom color). Drives the heading + paragraph builder
/// fast paths. The `Body` role's auto-applied `FOREGROUND` text color
/// counts as "default"; a run that explicitly sets a different color
/// disqualifies the fast path.
fn single_plain_text(runs: &[El]) -> Option<String> {
    let mut out = String::new();
    for run in runs {
        if !matches!(run.kind, Kind::Text) {
            return None;
        }
        if run.font_weight != FontWeight::Regular
            || run.text_italic
            || run.text_strikethrough
            || run.text_underline
            || run.text_link.is_some()
            || run.text_role != TextRole::Body
        {
            return None;
        }
        // Body role auto-sets `text_color = Some(FOREGROUND)`. Treat
        // `None` and `Some(FOREGROUND)` both as "default"; anything
        // else (a `.color(...)` override) is styled.
        if let Some(c) = run.text_color
            && c != tokens::FOREGROUND
        {
            return None;
        }
        let s = run.text.as_deref()?;
        out.push_str(s);
    }
    Some(out)
}

/// Trim a single trailing `\n` (pulldown-cmark always emits one at
/// the end of a fenced or indented code block).
fn strip_trailing_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transformer always wraps blocks in a `column`. Reach into
    /// it for the test assertions.
    fn blocks(input: &str) -> Vec<El> {
        match md(input) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        }
    }

    fn blocks_with_options(input: &str, options: MarkdownOptions) -> Vec<El> {
        match md_with_options(input, options) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        }
    }

    #[test]
    fn empty_document_yields_an_empty_column() {
        let bs = blocks("");
        assert!(bs.is_empty());
    }

    #[test]
    fn h1_h2_h3_map_to_heading_constructors() {
        let bs = blocks("# Title\n\n## Subtitle\n\n### Section");
        assert_eq!(bs.len(), 3);
        assert_eq!(bs[0].kind, Kind::Heading);
        assert_eq!(bs[0].text.as_deref(), Some("Title"));
        assert_eq!(bs[0].text_role, TextRole::Display);
        assert_eq!(bs[1].text_role, TextRole::Heading);
        assert_eq!(bs[2].text_role, TextRole::Title);
    }

    #[test]
    fn h4_h5_h6_clamp_to_h3() {
        let bs = blocks("#### Four\n\n##### Five\n\n###### Six");
        for b in &bs {
            assert_eq!(b.kind, Kind::Heading);
            assert_eq!(b.text_role, TextRole::Title);
        }
    }

    #[test]
    fn plain_paragraph_collapses_to_paragraph_widget() {
        let bs = blocks("Just some prose.");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Text);
        assert_eq!(bs[0].text.as_deref(), Some("Just some prose."));
        assert_eq!(bs[0].text_wrap, TextWrap::Wrap);
    }

    #[test]
    fn paragraph_with_inline_styling_uses_text_runs() {
        let bs = blocks("Hello **world** and *italic* and `code`.");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Inlines);
        let runs: Vec<&El> = bs[0].children.iter().collect();
        // Plain "Hello " + bold "world" + " and " + italic "italic" +
        // " and " + code "code" + ".".
        assert!(
            runs.iter()
                .any(|r| r.font_weight == FontWeight::Bold && r.text.as_deref() == Some("world"))
        );
        assert!(
            runs.iter()
                .any(|r| r.text_italic && r.text.as_deref() == Some("italic"))
        );
        assert!(
            runs.iter()
                .any(|r| r.text_role == TextRole::Code && r.text.as_deref() == Some("code"))
        );
    }

    #[test]
    fn math_option_routes_inline_and_display_math_to_math_nodes() {
        let bs = blocks_with_options(
            "Euler $e^{i\\pi}+1=0$\n\n$$\\frac{a}{b}$$",
            MarkdownOptions::default().math(true),
        );
        assert_eq!(bs.len(), 2);
        assert_eq!(bs[0].kind, Kind::Inlines);
        assert!(
            bs[0]
                .children
                .iter()
                .any(|child| matches!(child.kind, Kind::Math) && child.math.is_some())
        );
        assert_eq!(bs[1].kind, Kind::Math);
        assert_eq!(bs[1].math_display, MathDisplay::Block);
    }

    #[test]
    fn link_groups_runs_under_the_same_url() {
        let bs = blocks("Check [the **bold** site](https://aetna.dev) for info.");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let linked: Vec<&El> = bs[0]
            .children
            .iter()
            .filter(|r| r.text_link.as_deref() == Some("https://aetna.dev"))
            .collect();
        assert!(!linked.is_empty(), "expected at least one linked run");
        // The bold word inside the link keeps its bold flag plus the
        // shared href.
        assert!(linked.iter().any(|r| r.font_weight == FontWeight::Bold));
    }

    #[test]
    fn bullet_list_emits_bullet_list_widget() {
        let bs = blocks("- one\n- two\n- three");
        assert_eq!(bs.len(), 1);
        // bullet_list returns a column of overlay-stack items — the
        // children count matches the item count.
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Column);
        assert_eq!(bs[0].children.len(), 3);
    }

    #[test]
    fn ordered_list_emits_numbered_list_widget() {
        let bs = blocks("1. alpha\n2. beta\n3. gamma");
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Column);
        assert_eq!(bs[0].children.len(), 3);
        // The first item's marker slot should carry the "1." label.
        let first_marker_slot = &bs[0].children[0].children[0];
        let first_marker = &first_marker_slot.children[0];
        assert_eq!(first_marker.text.as_deref(), Some("1."));
    }

    #[test]
    fn ordered_list_preserves_non_one_start_number() {
        let bs = blocks("42. alpha\n43. beta");
        assert_eq!(bs[0].children.len(), 2);
        let first_marker_slot = &bs[0].children[0].children[0];
        let second_marker_slot = &bs[0].children[1].children[0];
        assert_eq!(first_marker_slot.children[0].text.as_deref(), Some("42."));
        assert_eq!(second_marker_slot.children[0].text.as_deref(), Some("43."));
    }

    #[test]
    fn task_list_emits_static_task_markers() {
        let bs = blocks("- [x] done\n- [ ] todo");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].children.len(), 2);

        let checked = &bs[0].children[0].children[0].children[0];
        let unchecked = &bs[0].children[1].children[0].children[0];
        assert_eq!(checked.kind, Kind::Custom("task_marker"));
        assert_eq!(unchecked.kind, Kind::Custom("task_marker"));
        assert_eq!(checked.fill, Some(tokens::PRIMARY));
        assert_eq!(unchecked.fill, Some(tokens::CARD));
        assert!(!checked.focusable);
        assert!(!unchecked.focusable);
    }

    #[test]
    fn nested_list_lives_inside_the_outer_item() {
        let input = "- outer one\n  - inner a\n  - inner b\n- outer two";
        let bs = blocks(input);
        assert_eq!(bs.len(), 1);
        let outer = &bs[0];
        assert_eq!(outer.children.len(), 2);
        // First outer item collapses to a multi-block column (paragraph
        // + nested list). The transformer wraps multi-block items in
        // `column`; reach into it.
        let first_item_body = &outer.children[0].children[1];
        // first_item_body is the body slot in the overlay-stack item
        // shape. It contains a single child: the list-item content.
        let inner_content = &first_item_body.children[0];
        // For nested-list items, the content is a column of [paragraph,
        // nested bullet_list]. The second child is the nested list.
        assert_eq!(inner_content.kind, Kind::Group);
        assert!(inner_content.children.len() >= 2);
    }

    #[test]
    fn blockquote_wraps_inner_paragraphs() {
        let bs = blocks("> First line.\n>\n> Second line.");
        assert_eq!(bs.len(), 1);
        // blockquote is a stack of [rule, body_column].
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Overlay);
        assert_eq!(bs[0].children.len(), 2);
        let body = &bs[0].children[1];
        assert_eq!(body.children.len(), 2);
    }

    #[test]
    fn fenced_code_block_keeps_verbatim_text() {
        let bs = blocks("```\nfn main() {}\n```");
        assert_eq!(bs.len(), 1);
        // code_block surface contains a single mono text leaf that
        // resolves to the JBM monospace face via the El default
        // (themes can override with `with_mono_font_family`).
        let surface = &bs[0];
        assert_eq!(surface.surface_role, SurfaceRole::Sunken);
        let body = &surface.children[0];
        assert_eq!(body.text.as_deref(), Some("fn main() {}"));
        assert!(body.font_mono);
        assert_eq!(
            body.mono_font_family,
            aetna_core::tree::FontFamily::JetBrainsMono
        );
    }

    #[test]
    fn indented_code_block_keeps_verbatim_text() {
        let bs = blocks("    let x = 1;\n    let y = 2;");
        assert_eq!(bs.len(), 1);
        let body = &bs[0].children[0];
        assert_eq!(body.text.as_deref(), Some("let x = 1;\nlet y = 2;"));
    }

    /// Fenced block with an unrecognised language falls back to the
    /// plain-mono `code_block(...)` path (single text leaf, no inline
    /// runs) — same behaviour as a fence with no info string.
    #[test]
    fn fenced_code_block_unknown_language_falls_back_to_plain_mono() {
        let bs = blocks("```nothinglikethis\nfn x() {}\n```");
        assert_eq!(bs.len(), 1);
        let body = &bs[0].children[0];
        assert_eq!(body.kind, Kind::Text);
        assert_eq!(body.text.as_deref(), Some("fn x() {}"));
        assert!(body.font_mono);
    }

    /// With the `highlighting` feature enabled (default), a fenced
    /// block tagged with a recognised language tokenises into a
    /// `text_runs` paragraph wrapped in the same code-block chrome.
    /// Tokens carry palette colors (here we just confirm there's more
    /// than one Text run and at least one carries a non-default color);
    /// finer mapping assertions live in `highlight::tests`.
    #[cfg(feature = "highlighting")]
    #[test]
    fn fenced_rust_code_block_emits_highlighted_runs() {
        let bs = blocks("```rust\n// hi\nfn main() {}\n```");
        assert_eq!(bs.len(), 1);
        let surface = &bs[0];
        assert_eq!(surface.surface_role, SurfaceRole::Sunken);
        let body = &surface.children[0];
        assert_eq!(body.kind, Kind::Inlines);
        let text_runs: Vec<&El> = body
            .children
            .iter()
            .filter(|c| c.kind == Kind::Text)
            .collect();
        assert!(
            text_runs.len() > 2,
            "expected multiple highlighted runs, got {}",
            text_runs.len()
        );
        assert!(
            text_runs.iter().all(|r| r.font_mono),
            "every highlighted run should ride the mono path"
        );
        assert!(
            text_runs.iter().any(|r| r.text_color.is_some()),
            "expected at least one run to carry a syntax color"
        );
    }

    #[test]
    fn horizontal_rule_emits_a_divider() {
        let bs = blocks("Above.\n\n---\n\nBelow.");
        let kinds: Vec<&Kind> = bs.iter().map(|b| &b.kind).collect();
        assert!(kinds.iter().any(|k| matches!(k, Kind::Divider)));
    }

    #[test]
    fn hard_break_inside_paragraph_emits_hard_break_node() {
        // CommonMark hard break = trailing two spaces + newline.
        let bs = blocks("line one  \nline two");
        assert_eq!(bs[0].kind, Kind::Inlines);
        assert!(
            bs[0]
                .children
                .iter()
                .any(|c| matches!(c.kind, Kind::HardBreak))
        );
    }

    #[test]
    fn soft_break_renders_as_a_space() {
        let bs = blocks("line one\nline two");
        assert_eq!(bs[0].kind, Kind::Text);
        // Plain paragraph fast path; soft break became a single space.
        let s = bs[0].text.as_deref().unwrap();
        assert!(s.contains("line one line two"), "got {s:?}");
    }

    #[test]
    fn image_renders_as_alt_placeholder() {
        let bs = blocks("![diagram of pipeline](pipeline.png \"Pipeline\")");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Inlines);
        let run = &bs[0].children[0];
        let s = run.text.as_deref().unwrap_or("");
        assert!(s.contains("diagram of pipeline"), "got {s:?}");
        assert!(s.contains("pipeline.png"), "got {s:?}");
        assert!(s.contains("Pipeline"), "got {s:?}");
        assert_eq!(run.text_link.as_deref(), Some("pipeline.png"));
    }

    #[test]
    fn inline_image_placeholder_preserves_order() {
        let bs = blocks("Before ![alt](img.png) after");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let text: String = bs[0]
            .children
            .iter()
            .filter_map(|run| run.text.as_deref())
            .collect();
        assert!(
            text.contains("Before [image: alt] img.png after"),
            "got {text:?}"
        );
    }

    #[test]
    fn document_outer_column_carries_block_gap() {
        let el = md("# A\n\nb");
        assert_eq!(el.kind, Kind::Group);
        assert_eq!(el.gap, tokens::SPACE_4);
    }

    #[test]
    fn table_emits_header_plus_body_widget() {
        let bs = blocks(
            "\
| Name  | Role |\n\
|-------|------|\n\
| Ada   | dev  |\n\
| Grace | ops  |\n",
        );
        assert_eq!(bs.len(), 1);
        let t = &bs[0];
        // `widgets::table` is `Kind::Custom("table")` with a header
        // child and a body child.
        assert_eq!(t.kind, Kind::Custom("table"));
        assert_eq!(t.children.len(), 2);
        let header = &t.children[0];
        let body = &t.children[1];
        assert_eq!(header.kind, Kind::Custom("table_header"));
        assert_eq!(body.kind, Kind::Custom("table_body"));
        // Header has one row of two cells.
        assert_eq!(header.children.len(), 1);
        assert_eq!(header.children[0].children.len(), 2);
        // Body has two rows, each with two cells.
        assert_eq!(body.children.len(), 2);
        assert_eq!(body.children[0].children.len(), 2);
    }

    #[test]
    fn table_header_cells_carry_caption_styling() {
        let bs = blocks(
            "\
| Header |\n\
|--------|\n\
| body   |\n",
        );
        let t = &bs[0];
        let header_cell = &t.children[0].children[0].children[0];
        assert_eq!(header_cell.text.as_deref(), Some("Header"));
        // `table_head(...)` applies the caption role.
        assert_eq!(header_cell.text_role, TextRole::Caption);
    }

    #[test]
    fn table_body_cells_with_inline_styling_use_text_runs() {
        let bs = blocks(
            "\
| Col |\n\
|-----|\n\
| **bold** word |\n",
        );
        let t = &bs[0];
        let body_cell = &t.children[1].children[0].children[0];
        // Body cell wraps the styled content in an Inlines paragraph.
        assert_eq!(body_cell.kind, Kind::Inlines);
        assert!(
            body_cell
                .children
                .iter()
                .any(|r| r.font_weight == FontWeight::Bold && r.text.as_deref() == Some("bold"))
        );
    }

    #[test]
    fn table_alignment_applies_to_header_and_body_cells() {
        let bs = blocks(
            "\
| Left | Center | Right |\n\
|:-----|:------:|------:|\n\
| a    | b      | c     |\n",
        );
        let t = &bs[0];
        let header_row = &t.children[0].children[0];
        let body_row = &t.children[1].children[0];

        assert_eq!(header_row.children[0].text_align, TextAlign::Start);
        assert_eq!(header_row.children[1].text_align, TextAlign::Center);
        assert_eq!(header_row.children[2].text_align, TextAlign::End);
        assert_eq!(body_row.children[0].text_align, TextAlign::Start);
        assert_eq!(body_row.children[1].text_align, TextAlign::Center);
        assert_eq!(body_row.children[2].text_align, TextAlign::End);
    }

    #[test]
    fn table_header_cells_preserve_inline_styling() {
        let bs = blocks(
            "\
| **Header** |\n\
|------------|\n\
| body       |\n",
        );
        let header_cell = &bs[0].children[0].children[0].children[0];
        assert_eq!(header_cell.kind, Kind::Inlines);
        assert!(
            header_cell
                .children
                .iter()
                .any(|r| r.font_weight == FontWeight::Bold
                    && r.text_role == TextRole::Caption
                    && r.text.as_deref() == Some("Header"))
        );
    }

    #[test]
    fn strikethrough_inline_run_marks_text_strikethrough() {
        // GFM strikethrough is gated behind ENABLE_STRIKETHROUGH; the
        // walker already had the Tag matcher, but the parser only
        // emits the events when the option is on.
        let bs = blocks("Some ~~obsolete~~ text.");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let strike: Vec<&El> = bs[0]
            .children
            .iter()
            .filter(|r| r.text_strikethrough)
            .collect();
        assert!(!strike.is_empty(), "expected a strikethrough run");
        assert_eq!(strike[0].text.as_deref(), Some("obsolete"));
    }

    #[test]
    fn smart_punctuation_is_opt_in() {
        let plain = blocks("Wait...");
        assert_eq!(plain[0].text.as_deref(), Some("Wait..."));

        let smart = match md_with_options(
            "Wait...",
            MarkdownOptions::default().smart_punctuation(true),
        ) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        };
        assert_eq!(smart[0].text.as_deref(), Some("Wait\u{2026}"));
    }

    #[test]
    fn gfm_alerts_are_opt_in() {
        let plain = blocks("> [!WARNING]\n> Careful.");
        assert_eq!(plain[0].axis, Axis::Overlay);

        let alert_blocks = match md_with_options(
            "> [!WARNING]\n> Careful.",
            MarkdownOptions::default().gfm_alerts(true),
        ) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        };
        assert_eq!(alert_blocks[0].kind, Kind::Custom("alert"));
        assert_eq!(alert_blocks[0].children[0].text.as_deref(), Some("Warning"));
        assert_eq!(alert_blocks[0].fill, Some(tokens::WARNING.with_alpha(38)));
    }
}
