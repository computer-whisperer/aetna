//! ui_lib_demo — v2 sketch of an LLM-native retained UI toolkit.
//!
//! Focus of this iteration:
//! - replace `desired_w/h` magic hints with sizing intents (`Fixed`, `Fill`, `Hug`)
//! - capture useful source maps from call sites via a tiny macro
//! - allow typed message actions on nodes (`El<Msg>`)
//! - generate inspector/lint/responsive/motion artifacts from the same semantic tree

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Write as _};

pub type El<Msg = ()> = Node<Msg>;

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self { Self { x, y, w, h } }
    pub fn inset(self, p: Padding) -> Self {
        Self::new(
            self.x + p.left,
            self.y + p.top,
            (self.w - p.left - p.right).max(0.0),
            (self.h - p.top - p.bottom).max(0.0),
        )
    }
    pub fn right(self) -> f32 { self.x + self.w }
    pub fn bottom(self) -> f32 { self.y + self.h }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Padding { pub left: f32, pub right: f32, pub top: f32, pub bottom: f32 }
impl Padding {
    pub fn all(v: f32) -> Self { Self { left: v, right: v, top: v, bottom: v } }
    pub fn xy(x: f32, y: f32) -> Self { Self { left: x, right: x, top: y, bottom: y } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Fixed(f32),
    Fill(f32),
    Hug,
}
impl Default for Size { fn default() -> Self { Size::Fill(1.0) } }

#[derive(Clone, Copy, Debug)]
pub enum Align { Start, Center, End, Stretch }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TokenKind { Color, Space, Radius, Shadow, Text, Motion }

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokenRef { pub kind: TokenKind, pub name: &'static str }
impl TokenRef { pub const fn new(kind: TokenKind, name: &'static str) -> Self { Self { kind, name } } }
impl Debug for TokenRef { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{:?}.{}", self.kind, self.name) } }

#[derive(Clone, Copy, Debug)]
pub struct Color { pub rgba: (u8, u8, u8, u8), pub token: Option<TokenRef> }
impl Color {
    pub const fn raw(r: u8, g: u8, b: u8, a: u8) -> Self { Self { rgba: (r, g, b, a), token: None } }
    pub const fn token(name: &'static str, r: u8, g: u8, b: u8, a: u8) -> Self { Self { rgba: (r, g, b, a), token: Some(TokenRef::new(TokenKind::Color, name)) } }
    pub fn svg(self) -> String {
        let (r, g, b, a) = self.rgba;
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("rgba({r}, {g}, {b}, {:.3})", a as f32 / 255.0) }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TokF32 { pub value: f32, pub token: Option<TokenRef> }
impl TokF32 {
    pub const fn raw(value: f32) -> Self { Self { value, token: None } }
    pub const fn token(kind: TokenKind, name: &'static str, value: f32) -> Self { Self { value, token: Some(TokenRef::new(kind, name)) } }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub colors: Colors,
    pub space: Space,
    pub radius: Radius,
    pub shadow: Shadow,
    pub text: TextTokens,
    pub motion: MotionTokens,
}

#[derive(Clone, Debug)]
pub struct Colors {
    pub app_bg: Color,
    pub surface: Color,
    pub surface_raised: Color,
    pub surface_hover: Color,
    pub border: Color,
    pub border_strong: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub overlay_scrim: Color,
}
#[derive(Clone, Debug)] pub struct Space { pub xs: TokF32, pub sm: TokF32, pub md: TokF32, pub lg: TokF32, pub xl: TokF32 }
#[derive(Clone, Debug)] pub struct Radius { pub sm: TokF32, pub md: TokF32, pub lg: TokF32, pub xl: TokF32, pub pill: TokF32 }
#[derive(Clone, Debug)] pub struct Shadow { pub card: TokF32, pub overlay: TokF32 }
#[derive(Clone, Debug)] pub struct TextTokens { pub small: TokF32, pub body: TokF32, pub title: TokF32, pub mono: TokF32 }
#[derive(Clone, Debug)] pub struct MotionTokens { pub fast_ms: TokF32, pub standard_ms: TokF32, pub modal_ms: TokF32 }

impl Theme {
    pub fn dark_blue_gray() -> Self {
        Self {
            colors: Colors {
                app_bg: Color::token("app_bg", 17, 20, 25, 255),
                surface: Color::token("surface", 26, 30, 36, 255),
                surface_raised: Color::token("surface_raised", 33, 38, 46, 255),
                surface_hover: Color::token("surface_hover", 43, 50, 60, 255),
                border: Color::token("border", 58, 66, 78, 255),
                border_strong: Color::token("border_strong", 88, 104, 128, 255),
                text: Color::token("text", 232, 238, 246, 255),
                text_muted: Color::token("text_muted", 148, 160, 176, 255),
                accent: Color::token("accent", 92, 170, 255, 255),
                accent_soft: Color::token("accent_soft", 92, 170, 255, 38),
                success: Color::token("success", 80, 210, 140, 255),
                warning: Color::token("warning", 245, 190, 85, 255),
                danger: Color::token("danger", 245, 95, 110, 255),
                overlay_scrim: Color::token("overlay_scrim", 0, 0, 0, 135),
            },
            space: Space {
                xs: TokF32::token(TokenKind::Space, "xs", 4.0),
                sm: TokF32::token(TokenKind::Space, "sm", 8.0),
                md: TokF32::token(TokenKind::Space, "md", 12.0),
                lg: TokF32::token(TokenKind::Space, "lg", 18.0),
                xl: TokF32::token(TokenKind::Space, "xl", 28.0),
            },
            radius: Radius {
                sm: TokF32::token(TokenKind::Radius, "sm", 5.0),
                md: TokF32::token(TokenKind::Radius, "md", 8.0),
                lg: TokF32::token(TokenKind::Radius, "lg", 12.0),
                xl: TokF32::token(TokenKind::Radius, "xl", 18.0),
                pill: TokF32::token(TokenKind::Radius, "pill", 999.0),
            },
            shadow: Shadow { card: TokF32::token(TokenKind::Shadow, "card", 16.0), overlay: TokF32::token(TokenKind::Shadow, "overlay", 32.0) },
            text: TextTokens {
                small: TokF32::token(TokenKind::Text, "small", 12.0),
                body: TokF32::token(TokenKind::Text, "body", 14.0),
                title: TokF32::token(TokenKind::Text, "title", 20.0),
                mono: TokF32::token(TokenKind::Text, "mono", 13.0),
            },
            motion: MotionTokens {
                fast_ms: TokF32::token(TokenKind::Motion, "fast_ms", 120.0),
                standard_ms: TokF32::token(TokenKind::Motion, "standard_ms", 180.0),
                modal_ms: TokF32::token(TokenKind::Motion, "modal_ms", 240.0),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub enum Role { App, Toolbar, Sidebar, Card, Button, Badge, Text, List, ListRow, Modal, Toast, Overlay, Spacer, Group(&'static str) }

#[derive(Clone, Debug)]
pub struct SourceMap { pub component: &'static str, pub file: &'static str, pub line: u32 }
impl SourceMap { pub const fn new(component: &'static str, file: &'static str, line: u32) -> Self { Self { component, file, line } } }

#[macro_export]
macro_rules! src_here {
    ($component:expr) => { $crate::SourceMap::new($component, file!(), line!()) };
}

#[derive(Clone, Debug)]
pub enum Layout {
    None,
    Column { gap: TokF32, padding: Padding, align_x: Align },
    Row { gap: TokF32, padding: Padding, align_y: Align },
    Overlay,
    VirtualList { row_h: TokF32, gap: TokF32, padding: Padding, max_rows: usize },
}

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub text: Option<Color>,
    pub radius: Option<TokF32>,
    pub shadow: Option<TokF32>,
    pub font_size: Option<TokF32>,
    pub mono: bool,
}
impl Style {
    pub fn fill(mut self, c: Color) -> Self { self.fill = Some(c); self }
    pub fn stroke(mut self, c: Color) -> Self { self.stroke = Some(c); self }
    pub fn text(mut self, c: Color) -> Self { self.text = Some(c); self }
    pub fn radius(mut self, r: TokF32) -> Self { self.radius = Some(r); self }
    pub fn shadow(mut self, s: TokF32) -> Self { self.shadow = Some(s); self }
    pub fn font(mut self, size: TokF32) -> Self { self.font_size = Some(size); self }
    pub fn mono(mut self) -> Self { self.mono = true; self }
}

#[derive(Clone, Debug)]
pub struct Node<Msg = ()> {
    pub id: Option<String>,
    pub key: Option<String>,
    pub role: Role,
    pub source: SourceMap,
    pub layout: Layout,
    pub style: Style,
    pub text: Option<String>,
    pub width: Size,
    pub height: Size,
    pub min_w: Option<f32>,
    pub max_w: Option<f32>,
    pub min_h: Option<f32>,
    pub max_h: Option<f32>,
    pub rect: Rect,
    pub action: Option<Msg>,
    pub children: Vec<Node<Msg>>,
}

impl<Msg> Node<Msg> {
    pub fn new(role: Role, source: SourceMap) -> Self {
        Self {
            id: None, key: None, role, source, layout: Layout::None, style: Style::default(), text: None,
            width: Size::Fill(1.0), height: Size::Fill(1.0), min_w: None, max_w: None, min_h: None, max_h: None,
            rect: Rect::default(), action: None, children: vec![],
        }
    }
    pub fn id(mut self, id: impl Into<String>) -> Self { self.id = Some(id.into()); self }
    pub fn key(mut self, key: impl Into<String>) -> Self { self.key = Some(key.into()); self }
    pub fn layout(mut self, layout: Layout) -> Self { self.layout = layout; self }
    pub fn style(mut self, style: Style) -> Self { self.style = style; self }
    pub fn text(mut self, text: impl Into<String>) -> Self { self.text = Some(text.into()); self }
    pub fn width(mut self, width: Size) -> Self { self.width = width; self }
    pub fn height(mut self, height: Size) -> Self { self.height = height; self }
    pub fn min_w(mut self, v: f32) -> Self { self.min_w = Some(v); self }
    pub fn max_w(mut self, v: f32) -> Self { self.max_w = Some(v); self }
    pub fn min_h(mut self, v: f32) -> Self { self.min_h = Some(v); self }
    pub fn max_h(mut self, v: f32) -> Self { self.max_h = Some(v); self }
    pub fn on_action(mut self, msg: Msg) -> Self { self.action = Some(msg); self }
    pub fn child(mut self, child: Node<Msg>) -> Self { self.children.push(child); self }
    pub fn children(mut self, children: impl IntoIterator<Item = Node<Msg>>) -> Self { self.children.extend(children); self }
}

impl<Msg> Node<Msg> where Msg: Clone {
    pub fn map_msg<Msg2>(self, f: impl Fn(Msg) -> Msg2 + Copy) -> Node<Msg2> {
        Node {
            id: self.id, key: self.key, role: self.role, source: self.source, layout: self.layout, style: self.style, text: self.text,
            width: self.width, height: self.height, min_w: self.min_w, max_w: self.max_w, min_h: self.min_h, max_h: self.max_h,
            rect: self.rect, action: self.action.map(f), children: self.children.into_iter().map(|c| c.map_msg(f)).collect(),
        }
    }
}

fn node_label<Msg>(node: &Node<Msg>, fallback_path: &str) -> String {
    node.id.clone().unwrap_or_else(|| {
        let role = match &node.role { Role::Group(name) => *name, other => match other {
            Role::App => "app", Role::Toolbar => "toolbar", Role::Sidebar => "sidebar", Role::Card => "card", Role::Button => "button", Role::Badge => "badge", Role::Text => "text", Role::List => "list", Role::ListRow => "row", Role::Modal => "modal", Role::Toast => "toast", Role::Overlay => "overlay", Role::Spacer => "spacer", Role::Group(_) => unreachable!(),
        }};
        format!("{fallback_path}.{role}")
    })
}

pub fn layout_tree<Msg>(root: &mut Node<Msg>, bounds: Rect) { root.rect = bounds; layout_children(root); }

fn intrinsic<Msg>(node: &Node<Msg>) -> (f32, f32) {
    if let Some(text) = &node.text {
        let size = node.style.font_size.map(|s| s.value).unwrap_or(14.0);
        return ((text.chars().count() as f32 * size * 0.58 + 16.0).min(900.0), (size + 14.0).max(28.0));
    }
    match &node.layout {
        Layout::Column { gap, padding, .. } => {
            let mut w: f32 = 0.0; let mut h: f32 = padding.top + padding.bottom;
            for (i, c) in node.children.iter().enumerate() {
                let (cw, ch) = intrinsic(c); w = w.max(cw); h += ch + if i > 0 { gap.value } else { 0.0 };
            }
            (w + padding.left + padding.right, h)
        }
        Layout::Row { gap, padding, .. } => {
            let mut w = padding.left + padding.right; let mut h: f32 = 0.0;
            for (i, c) in node.children.iter().enumerate() {
                let (cw, ch) = intrinsic(c); w += cw + if i > 0 { gap.value } else { 0.0 }; h = h.max(ch);
            }
            (w, h + padding.top + padding.bottom)
        }
        Layout::VirtualList { row_h, gap, padding, max_rows } => {
            let rows = node.children.len().min(*max_rows) as f32;
            (280.0, padding.top + padding.bottom + rows * row_h.value + rows.saturating_sub(1.0) * gap.value)
        }
        _ => (120.0, 40.0),
    }
}

trait SaturatingSubF32 { fn saturating_sub(self, rhs: f32) -> f32; }
impl SaturatingSubF32 for f32 { fn saturating_sub(self, rhs: f32) -> f32 { (self - rhs).max(0.0) } }

fn apply_constraints<Msg>(node: &Node<Msg>, mut w: f32, mut h: f32) -> (f32, f32) {
    if let Some(v) = node.min_w { w = w.max(v); } if let Some(v) = node.max_w { w = w.min(v); }
    if let Some(v) = node.min_h { h = h.max(v); } if let Some(v) = node.max_h { h = h.min(v); }
    (w, h)
}

fn layout_children<Msg>(node: &mut Node<Msg>) {
    match node.layout.clone() {
        Layout::None => { for c in &mut node.children { c.rect = node.rect; layout_children(c); } }
        Layout::Overlay => { for c in &mut node.children { c.rect = node.rect; layout_children(c); } }
        Layout::Column { gap, padding, align_x } => {
            let inner = node.rect.inset(padding);
            let total_gap = gap.value * node.children.len().saturating_sub(1) as f32;
            let mut fixed = 0.0; let mut fill_weight = 0.0;
            let intrinsics: Vec<_> = node.children.iter().map(intrinsic).collect();
            for (c, (_, ih)) in node.children.iter().zip(intrinsics.iter()) {
                match c.height { Size::Fixed(v) => fixed += v, Size::Hug => fixed += ih, Size::Fill(w) => fill_weight += w.max(0.001) }
            }
            let remaining = (inner.h - fixed - total_gap).max(0.0);
            let mut y = inner.y;
            for (c, (iw, ih)) in node.children.iter_mut().zip(intrinsics.into_iter()) {
                let h = match c.height { Size::Fixed(v) => v, Size::Hug => ih, Size::Fill(w) => remaining * w.max(0.001) / fill_weight.max(0.001) };
                let raw_w = match c.width { Size::Fixed(v) => v, Size::Hug => iw, Size::Fill(_) => inner.w };
                let (w, h) = apply_constraints(c, raw_w, h);
                let x = match align_x { Align::Start | Align::Stretch => inner.x, Align::Center => inner.x + (inner.w - w) / 2.0, Align::End => inner.right() - w };
                c.rect = Rect::new(x, y, if matches!(align_x, Align::Stretch) && !matches!(c.width, Size::Fixed(_) | Size::Hug) { inner.w } else { w }, h);
                y += h + gap.value;
                layout_children(c);
            }
        }
        Layout::Row { gap, padding, align_y } => {
            let inner = node.rect.inset(padding);
            let total_gap = gap.value * node.children.len().saturating_sub(1) as f32;
            let mut fixed = 0.0; let mut fill_weight = 0.0;
            let intrinsics: Vec<_> = node.children.iter().map(intrinsic).collect();
            for (c, (iw, _)) in node.children.iter().zip(intrinsics.iter()) {
                match c.width { Size::Fixed(v) => fixed += v, Size::Hug => fixed += iw, Size::Fill(w) => fill_weight += w.max(0.001) }
            }
            let remaining = (inner.w - fixed - total_gap).max(0.0);
            let mut x = inner.x;
            for (c, (iw, ih)) in node.children.iter_mut().zip(intrinsics.into_iter()) {
                let w = match c.width { Size::Fixed(v) => v, Size::Hug => iw, Size::Fill(weight) => remaining * weight.max(0.001) / fill_weight.max(0.001) };
                let raw_h = match c.height { Size::Fixed(v) => v, Size::Hug => ih, Size::Fill(_) => inner.h };
                let (w, h) = apply_constraints(c, w, raw_h);
                let y = match align_y { Align::Start | Align::Stretch => inner.y, Align::Center => inner.y + (inner.h - h) / 2.0, Align::End => inner.bottom() - h };
                c.rect = Rect::new(x, y, w, if matches!(align_y, Align::Stretch) && !matches!(c.height, Size::Fixed(_) | Size::Hug) { inner.h } else { h });
                x += w + gap.value;
                layout_children(c);
            }
        }
        Layout::VirtualList { row_h, gap, padding, max_rows } => {
            let inner = node.rect.inset(padding);
            let mut y = inner.y;
            for c in node.children.iter_mut().take(max_rows) {
                c.rect = Rect::new(inner.x, y, inner.w, row_h.value);
                y += row_h.value + gap.value;
                layout_children(c);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum RenderCommand { Rect { id: String, rect: Rect, fill: Color, stroke: Option<Color>, radius: f32, shadow: f32 }, Text { id: String, rect: Rect, text: String, color: Color, size: f32, mono: bool } }

pub fn render_commands<Msg>(node: &Node<Msg>, out: &mut Vec<RenderCommand>) { render_node(node, "root", out); }
fn render_node<Msg>(node: &Node<Msg>, path: &str, out: &mut Vec<RenderCommand>) {
    let id = node_label(node, path);
    if let Some(fill) = node.style.fill { out.push(RenderCommand::Rect { id: id.clone(), rect: node.rect, fill, stroke: node.style.stroke, radius: node.style.radius.map(|r| r.value).unwrap_or(0.0), shadow: node.style.shadow.map(|s| s.value).unwrap_or(0.0) }); }
    if let Some(text) = &node.text { out.push(RenderCommand::Text { id: id.clone(), rect: node.rect, text: text.clone(), color: node.style.text.unwrap_or(Color::raw(235,235,235,255)), size: node.style.font_size.map(|s| s.value).unwrap_or(14.0), mono: node.style.mono }); }
    for (i, c) in node.children.iter().enumerate() { render_node(c, &format!("{id}.{i}"), out); }
}

pub fn inspect_tree<Msg: Debug>(root: &Node<Msg>) -> String { let mut s = String::new(); inspect_node(root, "root", 0, &mut s); s }
fn inspect_node<Msg: Debug>(n: &Node<Msg>, path: &str, depth: usize, s: &mut String) {
    let id = node_label(n, path);
    let action = n.action.as_ref().map(|a| format!(" action={a:?}")).unwrap_or_default();
    let _ = writeln!(s, "{indent}{id} role={role:?} size=({w:?},{h:?}) rect=({:.1},{:.1},{:.1},{:.1}) source={component} {file}:{line}{action}",
        n.rect.x, n.rect.y, n.rect.w, n.rect.h, indent="  ".repeat(depth), role=n.role, w=n.width, h=n.height, component=n.source.component, file=n.source.file, line=n.source.line);
    for (i, c) in n.children.iter().enumerate() { inspect_node(c, &format!("{id}.{i}"), depth + 1, s); }
}

#[derive(Clone, Debug, Default)]
pub struct LintReport { pub duplicate_ids: Vec<String>, pub raw_colors: usize, pub raw_numbers: usize, pub tokens: BTreeMap<TokenKind, BTreeSet<&'static str>> }
impl LintReport { pub fn text(&self) -> String { format!("duplicate_ids={:?}\nraw_colors={}\nraw_numbers={}\ntokens={:#?}\n", self.duplicate_ids, self.raw_colors, self.raw_numbers, self.tokens) } }

pub fn lint_tree<Msg>(root: &Node<Msg>) -> LintReport {
    let mut r = LintReport::default(); let mut ids = BTreeMap::<String, usize>::new(); lint_node(root, "root", &mut r, &mut ids);
    r.duplicate_ids = ids.into_iter().filter_map(|(id, n)| (n > 1).then_some(id)).collect(); r
}
fn record_tok(r: &mut LintReport, tok: Option<TokenRef>) { if let Some(t) = tok { r.tokens.entry(t.kind).or_default().insert(t.name); } }
fn record_color(r: &mut LintReport, c: Option<Color>) { if let Some(c) = c { if let Some(t) = c.token { r.tokens.entry(t.kind).or_default().insert(t.name); } else { r.raw_colors += 1; } } }
fn lint_node<Msg>(n: &Node<Msg>, path: &str, r: &mut LintReport, ids: &mut BTreeMap<String, usize>) {
    let id = node_label(n, path); *ids.entry(id.clone()).or_default() += 1;
    record_color(r, n.style.fill); record_color(r, n.style.stroke); record_color(r, n.style.text);
    record_tok(r, n.style.radius.map(|v| v.token).flatten()); record_tok(r, n.style.shadow.map(|v| v.token).flatten()); record_tok(r, n.style.font_size.map(|v| v.token).flatten());
    match &n.layout { Layout::Column { gap, .. } | Layout::Row { gap, .. } => record_tok(r, gap.token), Layout::VirtualList { row_h, gap, .. } => { record_tok(r, row_h.token); record_tok(r, gap.token); }, _ => {} }
    for (i, c) in n.children.iter().enumerate() { lint_node(c, &format!("{id}.{i}"), r, ids); }
}

pub fn to_svg(width: f32, height: f32, commands: &[RenderCommand], bg: Color) -> String {
    let mut s = String::new();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#);
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="{}"/>"#, bg.svg());
    let _ = writeln!(s, r##"<defs><filter id="softShadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="8" stdDeviation="10" flood-color="#000" flood-opacity="0.28"/></filter></defs>"##);
    for cmd in commands { match cmd {
        RenderCommand::Rect { id, rect, fill, stroke, radius, shadow } => {
            let filter = if *shadow > 0.0 { r#" filter="url(#softShadow)""# } else { "" };
            let stroke_attr = stroke.map(|c| format!(r#" stroke="{}" stroke-width="1""#, c.svg())).unwrap_or_default();
            let _ = writeln!(s, r#"<rect data-node="{}" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{}"{}{} />"#, esc(id), rect.x, rect.y, rect.w, rect.h, radius.min(rect.h / 2.0), fill.svg(), stroke_attr, filter);
        }
        RenderCommand::Text { id, rect, text, color, size, mono } => {
            let family = if *mono { "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" } else { "Inter, ui-sans-serif, system-ui, sans-serif" };
            let y = rect.y + rect.h * 0.5 + size * 0.36;
            let _ = writeln!(s, r#"<text data-node="{}" x="{:.1}" y="{:.1}" font-family="{}" font-size="{:.1}" fill="{}">{}</text>"#, esc(id), rect.x + 8.0, y, family, size, color.svg(), esc(text));
        }
    }}
    s.push_str("</svg>\n"); s
}
fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

pub fn responsive_tape<Msg: Clone + Debug>(mut root: Node<Msg>, theme: &Theme, widths: &[f32], height: f32) -> String {
    let mut s = String::new(); let total_w: f32 = widths.iter().sum();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="{height}" viewBox="0 0 {total_w} {height}">"#);
    let mut xoff = 0.0;
    for &w in widths {
        layout_tree(&mut root, Rect::new(0.0, 0.0, w, height));
        let mut cmds = vec![]; render_commands(&root, &mut cmds);
        let svg = to_svg(w, height, &cmds, theme.colors.app_bg);
        let inner = svg.lines().skip(2).take_while(|l| !l.starts_with("</svg>")).collect::<Vec<_>>().join("\n");
        let _ = writeln!(s, r##"<g transform="translate({xoff},0)"><rect x="0" y="0" width="{w}" height="28" fill="#000" opacity="0.35"/><text x="10" y="20" fill="#94a0b0" font-size="13">{w:.0}px</text>{inner}</g>"##);
        xoff += w;
    }
    s.push_str("</svg>\n"); s
}

#[derive(Clone, Copy, Debug)] pub enum MotionPreset { ModalEnter, ToastSlide, HoverLift, PressSink }
pub fn motion_contact_sheet(theme: &Theme, preset: MotionPreset, width: f32, height: f32) -> String {
    let frames = [0.0, 50.0, 100.0, 160.0, 240.0, 320.0]; let cell_w = width; let total_w = cell_w * frames.len() as f32; let mut s = String::new();
    let duration = match preset { MotionPreset::ModalEnter => theme.motion.modal_ms.value, MotionPreset::ToastSlide => theme.motion.standard_ms.value, MotionPreset::HoverLift | MotionPreset::PressSink => theme.motion.fast_ms.value };
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="{height}" viewBox="0 0 {total_w} {height}">"#);
    for (i, t) in frames.iter().enumerate() {
        let x0 = i as f32 * cell_w; let p = (*t / duration).clamp(0.0, 1.0); let eased = 1.0 - (1.0 - p).powi(3);
        let scale = match preset { MotionPreset::PressSink => 1.0 - 0.025 * eased, _ => 0.94 + 0.06 * eased };
        let alpha = eased; let modal_w = 280.0 * scale; let modal_h = 150.0 * scale; let mx = x0 + (cell_w - modal_w) / 2.0; let my = 70.0 + (1.0 - eased) * 22.0;
        let _ = writeln!(s, r#"<rect x="{x0}" y="0" width="{cell_w}" height="{height}" fill="{}"/><text x="{}" y="24" font-size="13" fill="{}">{:?} · {}ms p={:.2}</text><rect x="{x0}" y="36" width="{cell_w}" height="{}" fill="rgba(0,0,0,{:.3})"/><rect x="{mx}" y="{my}" width="{modal_w}" height="{modal_h}" rx="16" fill="{}" stroke="{}" opacity="{alpha:.3}"/><text x="{}" y="{}" font-size="18" fill="{}" opacity="{alpha:.3}">semantic motion</text></g>"#, theme.colors.app_bg.svg(), x0 + 12.0, theme.colors.text_muted.svg(), preset, *t as u32, eased, height - 36.0, 0.45 * alpha, theme.colors.surface_raised.svg(), theme.colors.border.svg(), mx + 20.0, my + 45.0, theme.colors.text.svg());
    }
    s.push_str("</svg>\n"); s
}

pub mod components {
    use super::*;

    pub fn app<Msg>(theme: &Theme, source: SourceMap, children: impl IntoIterator<Item = Node<Msg>>) -> Node<Msg> {
        Node::new(Role::App, source).id("app").layout(Layout::Column { gap: theme.space.md, padding: Padding::all(theme.space.lg.value), align_x: Align::Stretch }).children(children)
    }
    pub fn row<Msg>(theme: &Theme, source: SourceMap, children: impl IntoIterator<Item = Node<Msg>>) -> Node<Msg> {
        Node::new(Role::Group("row"), source).layout(Layout::Row { gap: theme.space.md, padding: Padding::all(0.0), align_y: Align::Stretch }).children(children)
    }
    pub fn column<Msg>(theme: &Theme, source: SourceMap, children: impl IntoIterator<Item = Node<Msg>>) -> Node<Msg> {
        Node::new(Role::Group("column"), source).layout(Layout::Column { gap: theme.space.md, padding: Padding::all(0.0), align_x: Align::Stretch }).children(children)
    }
    pub fn toolbar<Msg>(theme: &Theme, source: SourceMap, title: &str, actions: Vec<Node<Msg>>) -> Node<Msg> {
        Node::new(Role::Toolbar, source).id("toolbar").height(Size::Fixed(64.0)).layout(Layout::Row { gap: theme.space.sm, padding: Padding::all(theme.space.md.value), align_y: Align::Center }).style(Style::default().fill(theme.colors.surface).stroke(theme.colors.border).radius(theme.radius.lg).shadow(theme.shadow.card)).child(text(theme, src_here!("Text"), title, TextKind::Title).width(Size::Hug)).child(spacer(src_here!("Spacer")).width(Size::Fill(1.0))).children(actions)
    }
    pub fn sidebar<Msg>(theme: &Theme, source: SourceMap, children: Vec<Node<Msg>>) -> Node<Msg> {
        Node::new(Role::Sidebar, source).id("sidebar").width(Size::Fixed(250.0)).layout(Layout::Column { gap: theme.space.sm, padding: Padding::all(theme.space.md.value), align_x: Align::Stretch }).style(Style::default().fill(theme.colors.surface).stroke(theme.colors.border).radius(theme.radius.lg)).children(children)
    }
    pub fn card<Msg>(theme: &Theme, source: SourceMap, title: &str, children: Vec<Node<Msg>>) -> Node<Msg> {
        Node::new(Role::Card, source).height(Size::Hug).layout(Layout::Column { gap: theme.space.sm, padding: Padding::all(theme.space.md.value), align_x: Align::Stretch }).style(Style::default().fill(theme.colors.surface_raised).stroke(theme.colors.border).radius(theme.radius.xl).shadow(theme.shadow.card)).child(text(theme, src_here!("Text"), title, TextKind::Title).height(Size::Hug)).children(children)
    }
    pub fn button<Msg>(theme: &Theme, source: SourceMap, label: &str, variant: ButtonVariant) -> Node<Msg> {
        let (fill, stroke, text_c) = match variant { ButtonVariant::Primary => (theme.colors.accent, theme.colors.accent, Color::raw(8,16,25,255)), ButtonVariant::Secondary => (theme.colors.surface_hover, theme.colors.border_strong, theme.colors.text), ButtonVariant::Ghost => (Color::raw(255,255,255,0), theme.colors.border, theme.colors.text_muted), ButtonVariant::Danger => (theme.colors.danger, theme.colors.danger, Color::raw(30,5,8,255)) };
        Node::new(Role::Button, source).width(Size::Hug).height(Size::Fixed(40.0)).style(Style::default().fill(fill).stroke(stroke).text(text_c).radius(theme.radius.md).font(theme.text.body)).text(label)
    }
    pub fn badge<Msg>(theme: &Theme, source: SourceMap, label: &str, variant: BadgeVariant) -> Node<Msg> {
        let c = match variant { BadgeVariant::Success => theme.colors.success, BadgeVariant::Warning => theme.colors.warning, BadgeVariant::Info => theme.colors.accent, BadgeVariant::Danger => theme.colors.danger };
        let (r,g,b,_) = c.rgba;
        Node::new(Role::Badge, source).width(Size::Hug).height(Size::Fixed(28.0)).style(Style::default().fill(Color::raw(r,g,b,38)).stroke(c).text(c).radius(theme.radius.pill).font(theme.text.small)).text(label)
    }
    pub fn virtual_list<Msg>(theme: &Theme, source: SourceMap, rows: Vec<Node<Msg>>) -> Node<Msg> {
        Node::new(Role::List, source).height(Size::Fill(1.0)).layout(Layout::VirtualList { row_h: TokF32::token(TokenKind::Space, "list_row_h", 36.0), gap: theme.space.xs, padding: Padding::all(theme.space.xs.value), max_rows: 16 }).children(rows)
    }
    pub fn list_row<Msg>(theme: &Theme, source: SourceMap, label: &str, selected: bool) -> Node<Msg> {
        let fill = if selected { theme.colors.accent_soft } else { Color::raw(255,255,255,0) };
        Node::new(Role::ListRow, source).height(Size::Fixed(36.0)).style(Style::default().fill(fill).text(if selected { theme.colors.text } else { theme.colors.text_muted }).radius(theme.radius.md).font(theme.text.body)).text(label)
    }
    pub fn toast<Msg>(theme: &Theme, source: SourceMap, label: &str) -> Node<Msg> {
        Node::new(Role::Toast, source).height(Size::Fixed(48.0)).style(Style::default().fill(theme.colors.surface_raised).stroke(theme.colors.success).text(theme.colors.text).radius(theme.radius.lg).shadow(theme.shadow.overlay).font(theme.text.body)).text(label)
    }
    pub fn text<Msg>(theme: &Theme, source: SourceMap, value: &str, kind: TextKind) -> Node<Msg> {
        let (size, color, mono) = match kind { TextKind::Title => (theme.text.title, theme.colors.text, false), TextKind::Body => (theme.text.body, theme.colors.text, false), TextKind::BodyMuted => (theme.text.body, theme.colors.text_muted, false), TextKind::SmallMuted => (theme.text.small, theme.colors.text_muted, false), TextKind::Mono => (theme.text.mono, theme.colors.text, true) };
        let mut st = Style::default().text(color).font(size); if mono { st = st.mono(); }
        Node::new(Role::Text, source).height(Size::Hug).style(st).text(value)
    }
    pub fn spacer<Msg>(source: SourceMap) -> Node<Msg> { Node::new(Role::Spacer, source) }

    #[derive(Clone, Copy, Debug)] pub enum ButtonVariant { Primary, Secondary, Ghost, Danger }
    #[derive(Clone, Copy, Debug)] pub enum BadgeVariant { Success, Warning, Info, Danger }
    #[derive(Clone, Copy, Debug)] pub enum TextKind { Title, Body, BodyMuted, SmallMuted, Mono }
}
