//! Virtual list — interactive proof of the v0.5 virtualized list primitive.
//!
//! 100,000 rows in a fixed viewport. Wheel scrolls; only on-screen rows
//! are realized each frame. Click any row to bump a centred counter
//! (proves hit-test routes through virtualization-produced rects).
//!
//! Run: `cargo run -p aetna-demo --bin virtual_list`

use aetna_core::*;

const ROW_COUNT: usize = 100_000;
const ROW_HEIGHT: f32 = 44.0;

struct VirtualListApp {
    last_clicked: Option<usize>,
    clicks: u32,
}

fn build_row(i: usize) -> El {
    let badge_el = match i % 5 {
        0 => badge("info").muted(),
        1 => badge("warn").warning(),
        2 => badge("ok").success(),
        3 => badge("err").destructive(),
        _ => spacer(),
    };
    row([
        text(format!("#{i:06}")).mono(),
        spacer(),
        text(format!("entry {i}")),
        spacer(),
        badge_el,
    ])
    .key(format!("row-{i}"))
    .focusable()
    .gap(tokens::SPACE_MD)
    .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
    .height(Size::Fixed(ROW_HEIGHT))
}

impl App for VirtualListApp {
    fn build(&self) -> El {
        let header = match self.last_clicked {
            Some(i) => format!("clicked row {i} ({} times total)", self.clicks),
            None => format!("{ROW_COUNT} rows · scroll with the wheel · click any row"),
        };
        column([
            h1("Virtualized list"),
            text(header).muted(),
            virtual_list(ROW_COUNT, ROW_HEIGHT, build_row)
                .key("entries")
                .height(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
    }

    fn on_event(&mut self, event: UiEvent) {
        if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        let key = match event.key.as_deref() {
            Some(k) => k,
            None => return,
        };
        if let Some(idx_str) = key.strip_prefix("row-")
            && let Ok(idx) = idx_str.parse::<usize>()
        {
            self.last_clicked = Some(idx);
            self.clicks += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 600.0, 540.0);
    aetna_demo::run(
        "Aetna — virtual list",
        viewport,
        VirtualListApp {
            last_clicked: None,
            clicks: 0,
        },
    )
}
