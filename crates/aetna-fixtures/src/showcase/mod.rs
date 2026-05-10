//! Showcase — Aetna's hero demo.
//!
//! Sixteen pages across six groups: every shadcn-shaped widget gets a
//! demo on a category page, every system-level capability (theme swap,
//! hotkeys, animation, custom shaders, overlays, toasts) gets a page
//! that exercises it end-to-end. The sidebar's theme picker swaps the
//! active palette live so all pages can be browsed under any theme.

use aetna_core::prelude::*;

pub mod animation;
pub mod booleans;
pub mod buttons;
pub mod diagnostics;
pub mod forms;
pub mod hotkeys;
pub mod layout;
pub mod lists_tables;
pub mod media;
pub mod overlays;
pub mod page_chrome;
pub mod palette;
pub mod shell;
pub mod status;
pub mod surfaces;
pub mod tabs_accordion;
pub mod text_inputs;
pub mod theme_choice;
pub mod typography;

pub use shell::THEME_PICKER_KEY;
pub use theme_choice::ThemeChoice;

/// WGSL for the liquid-glass custom shader. Surfaced through
/// [`Showcase::shaders`] so host harnesses register it once at startup;
/// the shader source lives next to the fixture so the registration and
/// the consumer can never drift.
pub const LIQUID_GLASS_WGSL: &str = include_str!("../../shaders/liquid_glass.wgsl");

/// Which page is currently mounted in the content panel. Each variant
/// owns its own state struct on [`Showcase`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    /// Token swatches grid for the active theme — the hero shot.
    #[default]
    Palette,
    Typography,
    Surfaces,
    Layout,
    Buttons,
    Booleans,
    TextInputs,
    Forms,
    Status,
    Media,
    ListsTables,
    TabsAccordion,
    Overlays,
    PageChrome,
    Animation,
    Hotkeys,
}

/// Sidebar grouping. Each section belongs to exactly one group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Theme,
    Foundations,
    Inputs,
    Display,
    Navigation,
    Patterns,
}

impl Section {
    pub const ALL: [Section; 16] = [
        Section::Palette,
        Section::Typography,
        Section::Surfaces,
        Section::Layout,
        Section::Buttons,
        Section::Booleans,
        Section::TextInputs,
        Section::Forms,
        Section::Status,
        Section::Media,
        Section::ListsTables,
        Section::TabsAccordion,
        Section::Overlays,
        Section::PageChrome,
        Section::Animation,
        Section::Hotkeys,
    ];

    /// Sidebar label.
    pub fn label(self) -> &'static str {
        match self {
            Section::Palette => "Palette",
            Section::Typography => "Typography",
            Section::Surfaces => "Surfaces",
            Section::Layout => "Layout",
            Section::Buttons => "Buttons & toggles",
            Section::Booleans => "Booleans",
            Section::TextInputs => "Text & value",
            Section::Forms => "Forms",
            Section::Status => "Status & feedback",
            Section::Media => "Media",
            Section::ListsTables => "Lists & tables",
            Section::TabsAccordion => "Tabs & accordion",
            Section::Overlays => "Overlays",
            Section::PageChrome => "Page chrome",
            Section::Animation => "Animation",
            Section::Hotkeys => "Hotkeys",
        }
    }

    /// Slug used in routed-key prefixes and bin output filenames.
    pub fn slug(self) -> &'static str {
        match self {
            Section::Palette => "palette",
            Section::Typography => "typography",
            Section::Surfaces => "surfaces",
            Section::Layout => "layout",
            Section::Buttons => "buttons",
            Section::Booleans => "booleans",
            Section::TextInputs => "text-inputs",
            Section::Forms => "forms",
            Section::Status => "status",
            Section::Media => "media",
            Section::ListsTables => "lists-tables",
            Section::TabsAccordion => "tabs-accordion",
            Section::Overlays => "overlays",
            Section::PageChrome => "page-chrome",
            Section::Animation => "animation",
            Section::Hotkeys => "hotkeys",
        }
    }

    /// Sidebar grouping this section belongs to.
    pub fn group(self) -> Group {
        match self {
            Section::Palette => Group::Theme,
            Section::Typography | Section::Surfaces | Section::Layout => Group::Foundations,
            Section::Buttons | Section::Booleans | Section::TextInputs | Section::Forms => {
                Group::Inputs
            }
            Section::Status | Section::Media | Section::ListsTables => Group::Display,
            Section::TabsAccordion | Section::Overlays | Section::PageChrome => Group::Navigation,
            Section::Animation | Section::Hotkeys => Group::Patterns,
        }
    }

    /// Sidebar nav key — a click on the matching button switches sections.
    pub fn nav_key(self) -> String {
        format!("nav-{}", self.slug())
    }
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Theme,
        Group::Foundations,
        Group::Inputs,
        Group::Display,
        Group::Navigation,
        Group::Patterns,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Theme => "Theme",
            Group::Foundations => "Foundations",
            Group::Inputs => "Inputs",
            Group::Display => "Display",
            Group::Navigation => "Navigation",
            Group::Patterns => "Patterns",
        }
    }

    /// Sections in this group, in sidebar order.
    pub fn sections(self) -> Vec<Section> {
        Section::ALL
            .iter()
            .copied()
            .filter(|s| s.group() == self)
            .collect()
    }
}

/// The showcase app. Per-section state persists across switches so
/// jumping in and out of a page is non-destructive. Fields are
/// `pub(crate)` so per-page layer-factory helpers (in their own
/// modules) can read both the active section and the relevant page
/// state to decide whether to mount their floating layer.
#[derive(Default)]
pub struct Showcase {
    pub(crate) section: Section,
    pub(crate) theme_choice: ThemeChoice,
    pub(crate) theme_picker_open: bool,

    pub(crate) surfaces: surfaces::State,
    pub(crate) layout: layout::State,
    pub(crate) buttons: buttons::State,
    pub(crate) booleans: booleans::State,
    pub(crate) text_inputs: text_inputs::State,
    pub(crate) forms: forms::State,
    pub(crate) status: status::State,
    pub(crate) lists_tables: lists_tables::State,
    pub(crate) tabs_accordion: tabs_accordion::State,
    pub(crate) overlays: overlays::State,
    pub(crate) animation: animation::State,
    pub(crate) hotkeys: hotkeys::State,

    /// Optional app-owned GPU texture surfaced on the Media page to
    /// demonstrate `surface()`. The fixtures crate is backend-neutral,
    /// so it can't allocate one itself — the host wires it up (see
    /// `examples/src/bin/showcase.rs` for the wgpu side that
    /// allocates a `wgpu::Texture` in `gpu_setup` and writes a
    /// procedurally-animated frame in `before_paint`).
    pub(crate) animated_surface: Option<aetna_core::surface::AppTexture>,
}

impl Showcase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with a specific starting page. Used by headless render
    /// bins to pin the showcase on one section without driving the
    /// navigation through events.
    pub fn with_section(section: Section) -> Self {
        Self {
            section,
            ..Default::default()
        }
    }

    /// Hand the showcase an app-owned GPU texture for the Media page's
    /// `surface()` demo. Pass `None` to clear it (the page falls back
    /// to a placeholder explaining the demo needs a wgpu host).
    pub fn set_animated_surface(&mut self, tex: Option<aetna_core::surface::AppTexture>) {
        self.animated_surface = tex;
    }

    /// Borrow the registered animated-surface texture, if any.
    pub fn animated_surface(&self) -> Option<&aetna_core::surface::AppTexture> {
        self.animated_surface.as_ref()
    }
}

impl App for Showcase {
    fn build(&self, cx: &BuildCx) -> El {
        let theme = self.theme();
        let body = match self.section {
            Section::Palette => palette::view(theme.palette()),
            Section::Typography => typography::view(),
            Section::Surfaces => surfaces::view(&self.surfaces),
            Section::Layout => layout::view(&self.layout),
            Section::Buttons => buttons::view(&self.buttons),
            Section::Booleans => booleans::view(&self.booleans),
            Section::TextInputs => text_inputs::view(&self.text_inputs),
            Section::Forms => forms::view(&self.forms),
            Section::Status => status::view(&self.status),
            Section::Media => media::view(self.animated_surface.as_ref()),
            Section::ListsTables => lists_tables::view(&self.lists_tables),
            Section::TabsAccordion => tabs_accordion::view(&self.tabs_accordion),
            Section::Overlays => overlays::view(&self.overlays),
            Section::PageChrome => page_chrome::view(),
            Section::Animation => animation::view(&self.animation),
            Section::Hotkeys => hotkeys::view(&self.hotkeys),
        };
        let (main, mut layers) = shell::frame(self, body);
        // Mount the diagnostic overlay on top of every page when the
        // host attached a `HostDiagnostics`. Hosts that opt out (the
        // headless render bins, vulkano-demo, anything that doesn't
        // call `BuildCx::with_diagnostics`) get the showcase exactly
        // as before — no overlay, no extra widgets in the tree.
        if let Some(diag) = cx.diagnostics() {
            layers.push(Some(diagnostics::layer(diag)));
        }
        overlay_root(main, layers)
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        match self.section {
            Section::Hotkeys => hotkeys::hotkeys(&self.hotkeys),
            _ => Vec::new(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme_choice.theme()
    }

    fn on_event(&mut self, event: UiEvent) {
        // Sidebar nav — any click on a `nav-*` key switches section.
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(k) = event.route()
            && let Some(target) = nav_section(k)
        {
            self.section = target;
            return;
        }

        // Theme picker (sidebar select dropdown).
        let mut token = self.theme_choice.token().to_string();
        if select::apply_event(
            &mut token,
            &mut self.theme_picker_open,
            &event,
            THEME_PICKER_KEY,
            |s| ThemeChoice::from_token(&s).map(|c| c.token().to_string()),
        ) {
            if let Some(choice) = ThemeChoice::from_token(&token) {
                self.theme_choice = choice;
            }
            return;
        }

        match self.section {
            Section::Palette => {}    // static — no events
            Section::Typography => {} // static
            Section::Surfaces => surfaces::on_event(&mut self.surfaces, event),
            Section::Layout => layout::on_event(&mut self.layout, event),
            Section::Buttons => buttons::on_event(&mut self.buttons, event),
            Section::Booleans => booleans::on_event(&mut self.booleans, event),
            Section::TextInputs => text_inputs::on_event(&mut self.text_inputs, event),
            Section::Forms => forms::on_event(&mut self.forms, event),
            Section::Status => status::on_event(&mut self.status, event),
            Section::Media => {} // static
            Section::ListsTables => lists_tables::on_event(&mut self.lists_tables, event),
            Section::TabsAccordion => tabs_accordion::on_event(&mut self.tabs_accordion, event),
            Section::Overlays => overlays::on_event(&mut self.overlays, event),
            Section::PageChrome => {} // static
            Section::Animation => animation::on_event(&mut self.animation, event),
            Section::Hotkeys => hotkeys::on_event(&mut self.hotkeys, event),
        }
    }

    fn drain_toasts(&mut self) -> Vec<ToastSpec> {
        std::mem::take(&mut self.status.pending_toasts)
    }

    fn shaders(&self) -> Vec<AppShader> {
        // The Surfaces page mounts the liquid_glass card. Register the
        // custom shader so the runtime has it at paint time regardless
        // of which section is currently shown.
        vec![AppShader {
            name: "liquid_glass",
            wgsl: LIQUID_GLASS_WGSL,
            samples_backdrop: true,
            samples_time: false,
        }]
    }
}

fn nav_section(key: &str) -> Option<Section> {
    Section::ALL.into_iter().find(|s| s.nav_key() == key)
}

/// Wrap the main view + floating layers in an overlay root. Layers are
/// only present when their controlling open-flag is set, otherwise they
/// drop out of the tree entirely.
fn overlay_root(main: El, layers: Vec<Option<El>>) -> El {
    overlays(main, layers)
}
