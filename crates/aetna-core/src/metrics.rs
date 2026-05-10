//! Component sizing vocabulary.
//!
//! Stock controls (button / input / badge / tab / choice / slider /
//! progress) carry a t-shirt `size` that maps 1:1 to shadcn's `size`
//! prop. Container surfaces (card / form / list / menu / table / panel)
//! bake their padding / gap / height / radius recipes directly in their
//! constructors — there is no global density knob, the way Tailwind /
//! shadcn picks padding per component class.

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
    Form,
    FormItem,
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
    button_size: Option<ComponentSize>,
    input_size: Option<ComponentSize>,
    badge_size: Option<ComponentSize>,
    tab_size: Option<ComponentSize>,
    choice_size: Option<ComponentSize>,
    slider_size: Option<ComponentSize>,
    progress_size: Option<ComponentSize>,
}

impl ThemeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_component_size(&self) -> ComponentSize {
        self.default_component_size
    }

    pub fn with_default_component_size(mut self, size: ComponentSize) -> Self {
        self.default_component_size = size;
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
                // TextArea bakes its padding + radius recipe directly
                // in the constructor (`widgets/text_area.rs`). The
                // metrics pass leaves it alone.
            }
            Some(MetricsRole::Badge) => {
                let size = el
                    .component_size
                    .or(self.badge_size)
                    .unwrap_or(self.default_component_size);
                apply_badge(el, badge_metrics(size));
            }
            Some(
                MetricsRole::Card
                | MetricsRole::CardHeader
                | MetricsRole::CardContent
                | MetricsRole::CardFooter,
            ) => {
                // Card surfaces do not participate in the metrics-driven
                // density override. Padding, gap, and radius are baked
                // into the constructors in `widgets/card.rs` (shadcn's
                // stock recipe). Override per-call with `.padding(...)`,
                // `.pt(...)` / `.px(...)` / etc.
            }
            Some(
                MetricsRole::Form
                | MetricsRole::FormItem
                | MetricsRole::Panel
                | MetricsRole::MenuItem
                | MetricsRole::ListItem
                | MetricsRole::PreferenceRow
                | MetricsRole::TableHeader
                | MetricsRole::TableRow,
            ) => {
                // These surfaces bake their padding / gap / height /
                // radius recipe directly in their constructors (see
                // `widgets/{form,alert,dialog,sheet,overlay,popover,
                // dropdown_menu,accordion,sidebar,command,table}.rs`).
                // The metrics pass does not touch them. Override per
                // call with `.padding(...)` / `.height(...)` / etc.
            }
            Some(MetricsRole::TabTrigger) => {
                let size = el
                    .component_size
                    .or(self.tab_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Button));
            }
            Some(MetricsRole::TabList) => {
                // Padding, gap, and radius are baked into
                // `tabs_list()`. The metrics pass only propagates the
                // optional `ComponentSize` down to TabTrigger children.
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
                // Padding, gap, and radius are baked into `radio_item()`.
                // The metrics pass only propagates `ComponentSize` down
                // to the ChoiceControl child.
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
            // Aetna's baseline component size is `Sm` so desktop apps
            // land in a denser-than-web baseline. Bump everything one
            // rung with `Theme::with_default_component_size(Md)`, or
            // override per-call with `.size(...)` / `.medium()` /
            // `.large()`.
            default_component_size: ComponentSize::Sm,
            button_size: None,
            input_size: None,
            badge_size: None,
            tab_size: None,
            choice_size: None,
            slider_size: None,
            progress_size: None,
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
        el.radius = crate::tree::Corners::all(metrics.radius);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, tabs_list, text_input, titled_card, tokens};

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
    fn card_slot_defaults_match_shadcn_stock() {
        // card_header / card_content / card_footer bake shadcn's `p-6`
        // / `p-6 pt-0` recipe directly via `default_padding(...)` in
        // the constructor. The metrics pass leaves those slots alone.
        let mut t = titled_card("Settings", [crate::text("Body")]);
        ThemeMetrics::default().apply_to_tree(&mut t);

        // Outer card is unpadded; the slots own all the spacing.
        assert_eq!(t.padding, Sides::zero());
        // Header: SPACE_6 on all four sides.
        assert_eq!(t.children[0].padding, Sides::all(tokens::SPACE_6));
        // Content: SPACE_6 on left / right / bottom, 0 on top (`p-6 pt-0`).
        assert_eq!(
            t.children[1].padding,
            Sides {
                left: tokens::SPACE_6,
                right: tokens::SPACE_6,
                top: 0.0,
                bottom: tokens::SPACE_6,
            }
        );
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
    fn raw_metrics_role_tags_no_longer_override_widget_defaults() {
        // After the density removal, surfaces like Form / FormItem /
        // ListItem / MenuItem / TableRow / PreferenceRow / ChoiceItem /
        // TextArea / TabList / Panel bake their padding / gap / height /
        // radius recipes into their constructors. The metrics pass does
        // not stamp anything onto bare-tagged Els (it only propagates
        // ComponentSize down to TabTrigger / ChoiceControl children).
        // This test asserts the absence — a bare El tagged with one of
        // those roles comes out with zero padding, zero gap, and Hug
        // height, exactly as if the role was unset.
        for role in [
            MetricsRole::Form,
            MetricsRole::FormItem,
            MetricsRole::ListItem,
            MetricsRole::MenuItem,
            MetricsRole::PreferenceRow,
            MetricsRole::TableRow,
            MetricsRole::TableHeader,
            MetricsRole::ChoiceItem,
            MetricsRole::TextArea,
            MetricsRole::TabList,
            MetricsRole::Panel,
        ] {
            let mut el = El::new(crate::Kind::Custom("bare")).metrics_role(role);
            ThemeMetrics::default().apply_to_tree(&mut el);
            assert_eq!(el.padding, Sides::zero(), "role {role:?} stamped padding");
            assert_eq!(el.gap, 0.0, "role {role:?} stamped gap");
            assert_eq!(el.height, Size::Hug, "role {role:?} stamped height");
            assert_eq!(
                el.radius,
                crate::tree::Corners::ZERO,
                "role {role:?} stamped radius"
            );
        }
    }

    #[test]
    fn form_constructor_bakes_default_gap() {
        // Smoke test for the constructor-baked recipe: form() picks up
        // SPACE_3 between items, form_item() picks up SPACE_2.
        let mut f = crate::form([crate::form_item([crate::text("body")])]);
        ThemeMetrics::default().apply_to_tree(&mut f);
        assert_eq!(f.gap, tokens::SPACE_3);
        assert_eq!(f.children[0].gap, tokens::SPACE_2);
    }

    #[test]
    fn default_metrics_are_compact_desktop_defaults() {
        let metrics = ThemeMetrics::default();

        assert_eq!(metrics.default_component_size(), ComponentSize::Sm);
    }
}
