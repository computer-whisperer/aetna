//! Component sizing and content-density vocabulary.
//!
//! Public names intentionally match familiar UI-kit conventions:
//! components have t-shirt `size`s, repeated information surfaces have
//! `density`, and theme defaults can set both globally.

use crate::tree::{El, Sides, Size};

/// T-shirt size for stock controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum ComponentSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
}

/// Information density for repeated or grouped content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

/// Theme-facing stock metrics role for a widget surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MetricsRole {
    Button,
    IconButton,
    Input,
    TextArea,
    Badge,
    Card,
    CardHeader,
    CardContent,
    CardFooter,
    Panel,
    MenuItem,
    ListItem,
    PreferenceRow,
    TableHeader,
    TableRow,
    TabTrigger,
    TabList,
    ChoiceControl,
    ChoiceItem,
    Slider,
    Progress,
}

/// Theme-owned layout metrics for stock widgets.
#[derive(Clone, Debug)]
pub struct ThemeMetrics {
    default_component_size: ComponentSize,
    default_density: Density,
    button_size: Option<ComponentSize>,
    input_size: Option<ComponentSize>,
    badge_size: Option<ComponentSize>,
    tab_size: Option<ComponentSize>,
    choice_size: Option<ComponentSize>,
    slider_size: Option<ComponentSize>,
    progress_size: Option<ComponentSize>,
    card_density: Option<Density>,
    panel_density: Option<Density>,
    menu_density: Option<Density>,
    list_density: Option<Density>,
    preference_density: Option<Density>,
    table_density: Option<Density>,
    tab_density: Option<Density>,
    choice_density: Option<Density>,
}

impl ThemeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_component_size(&self) -> ComponentSize {
        self.default_component_size
    }

    pub fn default_density(&self) -> Density {
        self.default_density
    }

    pub fn with_default_component_size(mut self, size: ComponentSize) -> Self {
        self.default_component_size = size;
        self
    }

    pub fn with_default_density(mut self, density: Density) -> Self {
        self.default_density = density;
        self
    }

    pub fn with_button_size(mut self, size: ComponentSize) -> Self {
        self.button_size = Some(size);
        self
    }

    pub fn with_input_size(mut self, size: ComponentSize) -> Self {
        self.input_size = Some(size);
        self
    }

    pub fn with_badge_size(mut self, size: ComponentSize) -> Self {
        self.badge_size = Some(size);
        self
    }

    pub fn with_tab_size(mut self, size: ComponentSize) -> Self {
        self.tab_size = Some(size);
        self
    }

    pub fn with_choice_size(mut self, size: ComponentSize) -> Self {
        self.choice_size = Some(size);
        self
    }

    pub fn with_slider_size(mut self, size: ComponentSize) -> Self {
        self.slider_size = Some(size);
        self
    }

    pub fn with_progress_size(mut self, size: ComponentSize) -> Self {
        self.progress_size = Some(size);
        self
    }

    pub fn with_card_density(mut self, density: Density) -> Self {
        self.card_density = Some(density);
        self
    }

    pub fn with_panel_density(mut self, density: Density) -> Self {
        self.panel_density = Some(density);
        self
    }

    pub fn with_menu_density(mut self, density: Density) -> Self {
        self.menu_density = Some(density);
        self
    }

    pub fn with_list_density(mut self, density: Density) -> Self {
        self.list_density = Some(density);
        self
    }

    pub fn with_preference_density(mut self, density: Density) -> Self {
        self.preference_density = Some(density);
        self
    }

    pub fn with_table_density(mut self, density: Density) -> Self {
        self.table_density = Some(density);
        self
    }

    pub fn with_tab_density(mut self, density: Density) -> Self {
        self.tab_density = Some(density);
        self
    }

    pub fn with_choice_density(mut self, density: Density) -> Self {
        self.choice_density = Some(density);
        self
    }

    pub(crate) fn apply_to_tree(&self, root: &mut El) {
        self.apply_to_el(root);
        for child in &mut root.children {
            self.apply_to_tree(child);
        }
    }

    fn apply_to_el(&self, el: &mut El) {
        match el.metrics_role {
            Some(MetricsRole::Button) => {
                let size = el
                    .component_size
                    .or(self.button_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Button));
            }
            Some(MetricsRole::IconButton) => {
                let size = el
                    .component_size
                    .or(self.button_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::IconButton));
            }
            Some(MetricsRole::Input) => {
                let size = el
                    .component_size
                    .or(self.input_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Input));
            }
            Some(MetricsRole::TextArea) => {
                let density = el.density.unwrap_or(self.default_density);
                apply_text_area(el, text_area_metrics(density));
            }
            Some(MetricsRole::Badge) => {
                let size = el
                    .component_size
                    .or(self.badge_size)
                    .unwrap_or(self.default_component_size);
                apply_badge(el, badge_metrics(size));
            }
            Some(MetricsRole::Card) => {
                let density = el
                    .density
                    .or(self.card_density)
                    .unwrap_or(self.default_density);
                apply_card_shell(el, card_shell_metrics(density));
                apply_card_density_to_children(el);
            }
            Some(MetricsRole::CardHeader) => {
                let density = el
                    .density
                    .or(self.card_density)
                    .unwrap_or(self.default_density);
                apply_card_section(el, card_header_metrics(density));
            }
            Some(MetricsRole::CardContent) => {
                let density = el
                    .density
                    .or(self.card_density)
                    .unwrap_or(self.default_density);
                apply_card_section(el, card_content_metrics(density));
            }
            Some(MetricsRole::CardFooter) => {
                let density = el
                    .density
                    .or(self.card_density)
                    .unwrap_or(self.default_density);
                apply_card_section(el, card_footer_metrics(density));
            }
            Some(MetricsRole::Panel) => {
                let density = el
                    .density
                    .or(self.panel_density)
                    .unwrap_or(self.default_density);
                apply_panel(el, card_metrics(density));
            }
            Some(MetricsRole::MenuItem) => {
                let density = el
                    .density
                    .or(self.menu_density)
                    .unwrap_or(self.default_density);
                apply_menu_item(el, menu_item_metrics(density));
            }
            Some(MetricsRole::ListItem) => {
                let density = el
                    .density
                    .or(self.list_density)
                    .unwrap_or(self.default_density);
                apply_list_item(el, list_item_metrics(density));
            }
            Some(MetricsRole::PreferenceRow) => {
                let density = el
                    .density
                    .or(self.preference_density)
                    .unwrap_or(self.default_density);
                apply_preference_row(el, preference_row_metrics(density));
            }
            Some(MetricsRole::TableHeader) => {
                let density = el
                    .density
                    .or(self.table_density)
                    .unwrap_or(self.default_density);
                apply_table_header(el, table_header_metrics(density));
            }
            Some(MetricsRole::TableRow) => {
                let density = el
                    .density
                    .or(self.table_density)
                    .unwrap_or(self.default_density);
                apply_table_row(el, table_row_metrics(density));
            }
            Some(MetricsRole::TabTrigger) => {
                let size = el
                    .component_size
                    .or(self.tab_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Button));
            }
            Some(MetricsRole::TabList) => {
                let density = el
                    .density
                    .or(self.tab_density)
                    .unwrap_or(self.default_density);
                apply_tab_list(el, tab_list_metrics(density));
                if let Some(size) = el.component_size {
                    apply_tab_trigger_size_to_children(el, size);
                }
            }
            Some(MetricsRole::ChoiceControl) => {
                let size = el
                    .component_size
                    .or(self.choice_size)
                    .unwrap_or(self.default_component_size);
                apply_choice_control(el, choice_control_metrics(size));
            }
            Some(MetricsRole::ChoiceItem) => {
                let density = el
                    .density
                    .or(self.choice_density)
                    .unwrap_or(self.default_density);
                apply_choice_item(el, choice_item_metrics(density));
                if let Some(size) = el.component_size {
                    apply_choice_control_size_to_children(el, size);
                }
            }
            Some(MetricsRole::Slider) => {
                let size = el
                    .component_size
                    .or(self.slider_size)
                    .unwrap_or(self.default_component_size);
                apply_single_axis_height(el, slider_metrics(size));
            }
            Some(MetricsRole::Progress) => {
                let size = el
                    .component_size
                    .or(self.progress_size)
                    .unwrap_or(self.default_component_size);
                apply_single_axis_height(el, progress_metrics(size));
            }
            None => {}
        }
    }
}

impl Default for ThemeMetrics {
    fn default() -> Self {
        Self {
            // Aetna's baseline is intentionally denser than generic
            // web defaults; apps can opt back to `comfortable()` or
            // `spacious()` at the theme boundary.
            default_component_size: ComponentSize::Sm,
            default_density: Density::Compact,
            button_size: None,
            input_size: None,
            badge_size: None,
            tab_size: None,
            choice_size: None,
            slider_size: None,
            progress_size: None,
            card_density: None,
            panel_density: None,
            menu_density: None,
            list_density: None,
            preference_density: None,
            table_density: None,
            tab_density: None,
            choice_density: None,
        }
    }
}

#[derive(Clone, Copy)]
enum ControlKind {
    Button,
    IconButton,
    Input,
}

#[derive(Clone, Copy)]
struct ControlMetrics {
    height: f32,
    padding_x: f32,
    radius: f32,
    gap: f32,
}

fn control_metrics(size: ComponentSize, kind: ControlKind) -> ControlMetrics {
    let (mut height, padding_x, radius, gap): (f32, f32, f32, f32) = match size {
        ComponentSize::Xs => (28.0, 8.0, 5.0, 4.0),
        ComponentSize::Sm => (32.0, 10.0, 6.0, 6.0),
        ComponentSize::Md => (36.0, 12.0, 7.0, 8.0),
        ComponentSize::Lg => (40.0, 14.0, 8.0, 8.0),
    };
    if matches!(kind, ControlKind::Input) && matches!(size, ComponentSize::Lg) {
        height = 44.0;
    }
    match kind {
        ControlKind::IconButton => ControlMetrics {
            height,
            padding_x: 0.0,
            radius,
            gap,
        },
        ControlKind::Input => ControlMetrics {
            height,
            padding_x: padding_x.max(10.0),
            radius,
            gap,
        },
        ControlKind::Button => ControlMetrics {
            height,
            padding_x,
            radius,
            gap,
        },
    }
}

fn apply_control(el: &mut El, metrics: ControlMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if matches!(el.metrics_role, Some(MetricsRole::IconButton)) && !el.explicit_width {
        el.width = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding && !matches!(el.metrics_role, Some(MetricsRole::IconButton)) {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
}

#[derive(Clone, Copy)]
struct BadgeMetrics {
    height: f32,
    padding_x: f32,
}

fn badge_metrics(size: ComponentSize) -> BadgeMetrics {
    match size {
        ComponentSize::Xs => BadgeMetrics {
            height: 18.0,
            padding_x: 6.0,
        },
        ComponentSize::Sm => BadgeMetrics {
            height: 20.0,
            padding_x: 7.0,
        },
        ComponentSize::Md => BadgeMetrics {
            height: 24.0,
            padding_x: 8.0,
        },
        ComponentSize::Lg => BadgeMetrics {
            height: 28.0,
            padding_x: 10.0,
        },
    }
}

fn apply_badge(el: &mut El, metrics: BadgeMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
}

#[derive(Clone, Copy)]
struct CardMetrics {
    padding: f32,
    gap: f32,
    radius: f32,
}

fn card_metrics(density: Density) -> CardMetrics {
    match density {
        Density::Compact => CardMetrics {
            padding: 12.0,
            gap: 8.0,
            radius: 7.0,
        },
        Density::Comfortable => CardMetrics {
            padding: 16.0,
            gap: 12.0,
            radius: 8.0,
        },
        Density::Spacious => CardMetrics {
            padding: 20.0,
            gap: 16.0,
            radius: 12.0,
        },
    }
}

#[derive(Clone, Copy)]
struct CardShellMetrics {
    radius: f32,
}

fn card_shell_metrics(density: Density) -> CardShellMetrics {
    let radius = match density {
        Density::Compact => 7.0,
        Density::Comfortable => 8.0,
        Density::Spacious => 12.0,
    };
    CardShellMetrics { radius }
}

#[derive(Clone, Copy)]
struct CardSectionMetrics {
    padding: Sides,
    gap: f32,
}

fn card_header_metrics(density: Density) -> CardSectionMetrics {
    match density {
        Density::Compact => CardSectionMetrics {
            padding: Sides {
                left: 12.0,
                right: 12.0,
                top: 12.0,
                bottom: 6.0,
            },
            gap: 4.0,
        },
        Density::Comfortable => CardSectionMetrics {
            padding: Sides {
                left: 16.0,
                right: 16.0,
                top: 16.0,
                bottom: 8.0,
            },
            gap: 4.0,
        },
        Density::Spacious => CardSectionMetrics {
            padding: Sides {
                left: 20.0,
                right: 20.0,
                top: 20.0,
                bottom: 10.0,
            },
            gap: 6.0,
        },
    }
}

fn card_content_metrics(density: Density) -> CardSectionMetrics {
    match density {
        Density::Compact => CardSectionMetrics {
            padding: Sides {
                left: 12.0,
                right: 12.0,
                top: 6.0,
                bottom: 12.0,
            },
            gap: 8.0,
        },
        Density::Comfortable => CardSectionMetrics {
            padding: Sides {
                left: 16.0,
                right: 16.0,
                top: 8.0,
                bottom: 16.0,
            },
            gap: 12.0,
        },
        Density::Spacious => CardSectionMetrics {
            padding: Sides {
                left: 20.0,
                right: 20.0,
                top: 10.0,
                bottom: 20.0,
            },
            gap: 16.0,
        },
    }
}

fn card_footer_metrics(density: Density) -> CardSectionMetrics {
    match density {
        Density::Compact => CardSectionMetrics {
            padding: Sides::all(12.0),
            gap: 8.0,
        },
        Density::Comfortable => CardSectionMetrics {
            padding: Sides::all(16.0),
            gap: 12.0,
        },
        Density::Spacious => CardSectionMetrics {
            padding: Sides::all(20.0),
            gap: 16.0,
        },
    }
}

fn apply_card_shell(el: &mut El, metrics: CardShellMetrics) {
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
    if !el.explicit_gap {
        el.gap = 0.0;
    }
}

fn apply_card_section(el: &mut El, metrics: CardSectionMetrics) {
    if !el.explicit_padding {
        el.padding = metrics.padding;
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
}

fn apply_card_density_to_children(el: &mut El) {
    let Some(density) = el.density else {
        return;
    };
    for child in &mut el.children {
        if matches!(
            child.metrics_role,
            Some(MetricsRole::CardHeader | MetricsRole::CardContent | MetricsRole::CardFooter)
        ) && child.density.is_none()
        {
            child.density = Some(density);
        }
    }
}

fn apply_card(el: &mut El, metrics: CardMetrics) {
    if !el.explicit_padding {
        el.padding = Sides::all(metrics.padding);
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
}

fn apply_panel(el: &mut El, metrics: CardMetrics) {
    apply_card(el, metrics);
}

#[derive(Clone, Copy)]
struct TextAreaMetrics {
    padding_x: f32,
    padding_y: f32,
    radius: f32,
}

fn text_area_metrics(density: Density) -> TextAreaMetrics {
    match density {
        Density::Compact => TextAreaMetrics {
            padding_x: 10.0,
            padding_y: 6.0,
            radius: 7.0,
        },
        Density::Comfortable => TextAreaMetrics {
            padding_x: 12.0,
            padding_y: 8.0,
            radius: 7.0,
        },
        Density::Spacious => TextAreaMetrics {
            padding_x: 14.0,
            padding_y: 10.0,
            radius: 8.0,
        },
    }
}

fn apply_text_area(el: &mut El, metrics: TextAreaMetrics) {
    if !el.explicit_padding {
        el.padding = Sides::xy(metrics.padding_x, metrics.padding_y);
    }
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
}

fn tab_list_metrics(density: Density) -> CardMetrics {
    match density {
        Density::Compact => CardMetrics {
            padding: 3.0,
            gap: 3.0,
            radius: 7.0,
        },
        Density::Comfortable => CardMetrics {
            padding: 4.0,
            gap: 4.0,
            radius: 8.0,
        },
        Density::Spacious => CardMetrics {
            padding: 6.0,
            gap: 6.0,
            radius: 10.0,
        },
    }
}

fn apply_tab_list(el: &mut El, metrics: CardMetrics) {
    apply_card(el, metrics);
}

fn apply_tab_trigger_size_to_children(el: &mut El, size: ComponentSize) {
    for child in &mut el.children {
        if matches!(child.metrics_role, Some(MetricsRole::TabTrigger))
            && child.component_size.is_none()
        {
            child.component_size = Some(size);
        }
    }
}

#[derive(Clone, Copy)]
struct ChoiceControlMetrics {
    edge: f32,
}

fn choice_control_metrics(size: ComponentSize) -> ChoiceControlMetrics {
    let edge = match size {
        ComponentSize::Xs => 14.0,
        ComponentSize::Sm => 16.0,
        ComponentSize::Md => 16.0,
        ComponentSize::Lg => 18.0,
    };
    ChoiceControlMetrics { edge }
}

fn apply_choice_control(el: &mut El, metrics: ChoiceControlMetrics) {
    if !el.explicit_width {
        el.width = Size::Fixed(metrics.edge);
    }
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.edge);
    }
}

#[derive(Clone, Copy)]
struct ChoiceItemMetrics {
    padding_y: f32,
    gap: f32,
    radius: f32,
}

fn choice_item_metrics(density: Density) -> ChoiceItemMetrics {
    match density {
        Density::Compact => ChoiceItemMetrics {
            padding_y: 2.0,
            gap: 6.0,
            radius: 5.0,
        },
        Density::Comfortable => ChoiceItemMetrics {
            padding_y: 4.0,
            gap: 8.0,
            radius: 6.0,
        },
        Density::Spacious => ChoiceItemMetrics {
            padding_y: 6.0,
            gap: 10.0,
            radius: 8.0,
        },
    }
}

fn apply_choice_item(el: &mut El, metrics: ChoiceItemMetrics) {
    if !el.explicit_padding {
        el.padding = Sides::xy(0.0, metrics.padding_y);
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
}

fn apply_choice_control_size_to_children(el: &mut El, size: ComponentSize) {
    for child in &mut el.children {
        if matches!(child.metrics_role, Some(MetricsRole::ChoiceControl))
            && child.component_size.is_none()
        {
            child.component_size = Some(size);
        }
    }
}

fn slider_metrics(size: ComponentSize) -> f32 {
    match size {
        ComponentSize::Xs => 14.0,
        ComponentSize::Sm => 16.0,
        ComponentSize::Md => 18.0,
        ComponentSize::Lg => 22.0,
    }
}

fn progress_metrics(size: ComponentSize) -> f32 {
    match size {
        ComponentSize::Xs => 4.0,
        ComponentSize::Sm => 6.0,
        ComponentSize::Md => 8.0,
        ComponentSize::Lg => 10.0,
    }
}

fn apply_single_axis_height(el: &mut El, height: f32) {
    if !el.explicit_height {
        el.height = Size::Fixed(height);
    }
}

#[derive(Clone, Copy)]
struct MenuItemMetrics {
    height: f32,
    padding_x: f32,
}

fn menu_item_metrics(density: Density) -> MenuItemMetrics {
    match density {
        Density::Compact => MenuItemMetrics {
            height: 30.0,
            padding_x: 8.0,
        },
        Density::Comfortable => MenuItemMetrics {
            height: 32.0,
            padding_x: 10.0,
        },
        Density::Spacious => MenuItemMetrics {
            height: 34.0,
            padding_x: 12.0,
        },
    }
}

fn apply_menu_item(el: &mut El, metrics: MenuItemMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
}

#[derive(Clone, Copy)]
struct RowMetrics {
    height: f32,
    padding_x: f32,
    gap: f32,
    radius: f32,
}

fn list_item_metrics(density: Density) -> RowMetrics {
    match density {
        Density::Compact => RowMetrics {
            height: 32.0,
            padding_x: 8.0,
            gap: 6.0,
            radius: 6.0,
        },
        Density::Comfortable => RowMetrics {
            height: 40.0,
            padding_x: 10.0,
            gap: 8.0,
            radius: 7.0,
        },
        Density::Spacious => RowMetrics {
            height: 44.0,
            padding_x: 12.0,
            gap: 10.0,
            radius: 8.0,
        },
    }
}

fn preference_row_metrics(density: Density) -> RowMetrics {
    match density {
        Density::Compact => RowMetrics {
            height: 52.0,
            padding_x: 12.0,
            gap: 16.0,
            radius: 0.0,
        },
        Density::Comfortable => RowMetrics {
            height: 60.0,
            padding_x: 16.0,
            gap: 16.0,
            radius: 0.0,
        },
        Density::Spacious => RowMetrics {
            height: 68.0,
            padding_x: 20.0,
            gap: 16.0,
            radius: 0.0,
        },
    }
}

fn table_header_metrics(density: Density) -> RowMetrics {
    match density {
        Density::Compact => RowMetrics {
            height: 32.0,
            padding_x: 8.0,
            gap: 8.0,
            radius: 0.0,
        },
        Density::Comfortable => RowMetrics {
            height: 36.0,
            padding_x: 10.0,
            gap: 10.0,
            radius: 0.0,
        },
        Density::Spacious => RowMetrics {
            height: 40.0,
            padding_x: 12.0,
            gap: 12.0,
            radius: 0.0,
        },
    }
}

fn table_row_metrics(density: Density) -> RowMetrics {
    match density {
        Density::Compact => RowMetrics {
            height: 40.0,
            padding_x: 8.0,
            gap: 8.0,
            radius: 6.0,
        },
        Density::Comfortable => RowMetrics {
            height: 52.0,
            padding_x: 10.0,
            gap: 10.0,
            radius: 7.0,
        },
        Density::Spacious => RowMetrics {
            height: 56.0,
            padding_x: 12.0,
            gap: 12.0,
            radius: 8.0,
        },
    }
}

fn apply_list_item(el: &mut El, metrics: RowMetrics) {
    apply_row_metrics(el, metrics);
}

fn apply_preference_row(el: &mut El, metrics: RowMetrics) {
    apply_row_metrics(el, metrics);
}

fn apply_table_header(el: &mut El, metrics: RowMetrics) {
    apply_row_metrics(el, metrics);
}

fn apply_table_row(el: &mut El, metrics: RowMetrics) {
    apply_row_metrics(el, metrics);
}

fn apply_row_metrics(el: &mut El, metrics: RowMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
    if !el.explicit_radius {
        el.radius = metrics.radius;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, tabs_list, text_input, titled_card};

    #[test]
    fn theme_default_component_size_applies_to_stock_control() {
        let mut el = button("Save");

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(40.0));
    }

    #[test]
    fn local_component_size_overrides_theme_default() {
        let mut el = button("Save").large();

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Xs)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(40.0));
    }

    #[test]
    fn input_uses_spacious_field_height_at_large_size() {
        let mut el = text_input("Search", &crate::Selection::default(), "search").large();

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(44.0));
    }

    #[test]
    fn explicit_height_overrides_component_metrics() {
        let mut el = button("Save").height(Size::Fixed(44.0));

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Sm)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(44.0));
    }

    #[test]
    fn theme_density_applies_to_card_defaults() {
        let mut el = titled_card("Settings", [crate::text("Body")]);

        ThemeMetrics::default()
            .with_default_density(Density::Compact)
            .apply_to_tree(&mut el);

        assert_eq!(el.padding, Sides::zero());
        assert_eq!(el.gap, 0.0);
        assert_eq!(el.children[0].padding.top, 12.0);
        assert_eq!(el.children[1].padding.bottom, 12.0);
    }

    #[test]
    fn theme_tab_size_applies_to_tab_triggers() {
        let mut el = tabs_list("settings", &"account", [("account", "Account")]);

        ThemeMetrics::default()
            .with_tab_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);

        assert_eq!(el.children[0].height, Size::Fixed(40.0));
    }

    #[test]
    fn local_tab_list_size_applies_to_tab_triggers() {
        let mut el =
            tabs_list("settings", &"account", [("account", "Account")]).size(ComponentSize::Lg);

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.children[0].height, Size::Fixed(40.0));
    }

    #[test]
    fn theme_choice_density_applies_to_radio_like_items() {
        let mut el = El::new(crate::Kind::Custom("choice")).metrics_role(MetricsRole::ChoiceItem);

        ThemeMetrics::default()
            .with_choice_density(Density::Spacious)
            .apply_to_tree(&mut el);

        assert_eq!(el.padding, Sides::xy(0.0, 6.0));
        assert_eq!(el.gap, 10.0);
    }

    #[test]
    fn local_choice_item_size_applies_to_child_control() {
        let control =
            El::new(crate::Kind::Custom("choice-control")).metrics_role(MetricsRole::ChoiceControl);
        let mut el = El::new(crate::Kind::Custom("choice"))
            .metrics_role(MetricsRole::ChoiceItem)
            .child(control)
            .size(ComponentSize::Lg);

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.children[0].width, Size::Fixed(18.0));
        assert_eq!(el.children[0].height, Size::Fixed(18.0));
    }

    #[test]
    fn progress_size_uses_component_scale() {
        let mut el = El::new(crate::Kind::Custom("progress")).metrics_role(MetricsRole::Progress);

        ThemeMetrics::default()
            .with_progress_size(ComponentSize::Sm)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(6.0));
    }

    #[test]
    fn list_density_applies_to_list_item_defaults() {
        let mut el = El::new(crate::Kind::Custom("list-item")).metrics_role(MetricsRole::ListItem);

        ThemeMetrics::default()
            .with_list_density(Density::Compact)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(32.0));
        assert_eq!(el.padding, Sides::xy(8.0, 0.0));
        assert_eq!(el.gap, 6.0);
    }

    #[test]
    fn preference_density_applies_to_two_line_settings_rows() {
        let mut el =
            El::new(crate::Kind::Custom("preference-row")).metrics_role(MetricsRole::PreferenceRow);

        ThemeMetrics::default()
            .with_preference_density(Density::Spacious)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(68.0));
        assert_eq!(el.padding, Sides::xy(20.0, 0.0));
        assert_eq!(el.gap, 16.0);
    }

    #[test]
    fn table_density_applies_to_table_rows() {
        let mut header =
            El::new(crate::Kind::Custom("header")).metrics_role(MetricsRole::TableHeader);
        let mut row = El::new(crate::Kind::Custom("row")).metrics_role(MetricsRole::TableRow);
        let metrics = ThemeMetrics::default().with_table_density(Density::Spacious);

        metrics.apply_to_tree(&mut header);
        metrics.apply_to_tree(&mut row);

        assert_eq!(header.height, Size::Fixed(40.0));
        assert_eq!(row.height, Size::Fixed(56.0));
    }

    #[test]
    fn default_metrics_are_compact_desktop_defaults() {
        let metrics = ThemeMetrics::default();

        assert_eq!(metrics.default_component_size(), ComponentSize::Sm);
        assert_eq!(metrics.default_density(), Density::Compact);
    }
}
