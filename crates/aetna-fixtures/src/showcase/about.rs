//! About — landing page for the Showcase.
//!
//! First-touch page for visitors hitting the published wasm build. Two
//! short paragraphs framing what Aetna is, a small live-widget card
//! that proves the page is interactive, and a pointer at the sidebar
//! plus the source repo. Counter + switch + slider were picked as the
//! teaser set because they cover three different interaction modes
//! (button click, toggle, drag/keyboard range) in a few rows.

use aetna_core::prelude::*;

const COUNTER_DEC_KEY: &str = "about-counter-dec";
const COUNTER_INC_KEY: &str = "about-counter-inc";
const NOTIFICATIONS_KEY: &str = "about-notifications";
const BRIGHTNESS_KEY: &str = "about-brightness";

pub struct State {
    pub counter: i32,
    pub notifications: bool,
    pub brightness: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            counter: 0,
            notifications: true,
            brightness: 0.6,
        }
    }
}

pub fn view(state: &State) -> El {
    scroll([column([
        h1("Aetna"),
        paragraph(
            "Aetna is a GPU UI rendering library that mounts inside an \
             existing Vulkan or wgpu host rather than owning the device, \
             queue, or swapchain. The page you're looking at is \
             wasm-compiled Rust running through a WebGPU canvas — the \
             same widget vocabulary every native build of this app uses.",
        )
        .muted(),
        paragraph(
            "Browse the sidebar for every widget category and system \
             feature. Switch themes from the picker above the nav and \
             watch each page re-resolve live.",
        )
        .muted(),
        section_label("Try it"),
        teaser_card(state),
        section_label("Source"),
        text_runs([
            text("Code, docs, and the open work list at "),
            text("github.com/computer-whisperer/aetna")
                .link("https://github.com/computer-whisperer/aetna"),
            text("."),
        ])
        .wrap_text(),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if switch::apply_event(&mut state.notifications, &e, NOTIFICATIONS_KEY) {
        return;
    }
    if slider::apply_input(&mut state.brightness, &e, BRIGHTNESS_KEY, 0.05, 0.25) {
        return;
    }
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        match e.route() {
            Some(COUNTER_DEC_KEY) => state.counter -= 1,
            Some(COUNTER_INC_KEY) => state.counter += 1,
            _ => {}
        }
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn teaser_card(state: &State) -> El {
    titled_card(
        "Live widgets",
        [
            counter_row(state.counter),
            switch_row(state.notifications),
            slider_row(state.brightness),
        ],
    )
}

fn counter_row(value: i32) -> El {
    row([
        text("Counter").label().width(Size::Fill(1.0)),
        button("−").secondary().small().key(COUNTER_DEC_KEY),
        mono(format!("{value:>3}")).label(),
        button("+").secondary().small().key(COUNTER_INC_KEY),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn switch_row(value: bool) -> El {
    let label = if value {
        "Notifications on"
    } else {
        "Notifications off"
    };
    row([
        text(label).label().width(Size::Fill(1.0)),
        switch(value).key(NOTIFICATIONS_KEY),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn slider_row(value: f32) -> El {
    column([
        row([
            text("Brightness").label().width(Size::Fill(1.0)),
            mono(format!("{}%", (value * 100.0).round() as i32))
                .small()
                .muted(),
        ])
        .align(Align::Center),
        slider(value, tokens::PRIMARY).key(BRIGHTNESS_KEY),
    ])
    .gap(tokens::SPACE_1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn counter_buttons_increment_and_decrement() {
        let mut s = State::default();
        on_event(&mut s, click(COUNTER_INC_KEY));
        on_event(&mut s, click(COUNTER_INC_KEY));
        on_event(&mut s, click(COUNTER_DEC_KEY));
        assert_eq!(s.counter, 1);
    }

    #[test]
    fn notifications_switch_toggles() {
        let mut s = State::default();
        let before = s.notifications;
        on_event(&mut s, click(NOTIFICATIONS_KEY));
        assert_ne!(s.notifications, before);
    }
}
