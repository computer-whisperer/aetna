use std::fs;
use ui_lib_demo::components::*;
use ui_lib_demo::*;

#[derive(Clone, Debug)]
enum Msg {
    Fetch,
    Commit,
    SelectCommit(usize),
    OpenCommandPalette,
    DangerReset,
}

fn main() -> std::io::Result<()> {
    let theme = Theme::dark_blue_gray();
    let mut root = build_ui(&theme);

    let width = 1400.0;
    let height = 860.0;
    layout_tree(&mut root, Rect::new(0.0, 0.0, width, height));

    let mut commands = Vec::new();
    render_commands(&root, &mut commands);

    fs::create_dir_all("out")?;
    fs::write("out/git_dashboard.svg", to_svg(width, height, &commands, theme.colors.app_bg))?;
    fs::write("out/git_dashboard.inspect.txt", inspect_tree(&root))?;
    fs::write("out/git_dashboard.lint.txt", lint_tree(&root).text())?;
    fs::write("out/git_dashboard.motion.svg", motion_contact_sheet(&theme, MotionPreset::ModalEnter, 280.0, 260.0))?;
    fs::write("out/git_dashboard.responsive.svg", responsive_tape(root.clone(), &theme, &[600.0, 900.0, 1200.0, 1600.0], 720.0))?;

    println!("wrote out/git_dashboard.svg");
    println!("wrote out/git_dashboard.inspect.txt");
    println!("wrote out/git_dashboard.lint.txt");
    println!("wrote out/git_dashboard.motion.svg");
    println!("wrote out/git_dashboard.responsive.svg");
    Ok(())
}

fn build_ui(theme: &Theme) -> El<Msg> {
    let branches = vec![
        list_row(theme, src_here!("ListRow"), "● main", true).key("main"),
        list_row(theme, src_here!("ListRow"), "  feature/ui-lib-demo", false).key("feature-ui"),
        list_row(theme, src_here!("ListRow"), "  vulkan-render-cleanup", false).key("vulkan"),
        list_row(theme, src_here!("ListRow"), "  agentic-inspector", false).key("agent"),
    ];

    let sidebar = sidebar(theme, src_here!("Sidebar"), vec![
        text(theme, src_here!("Text"), "WORKSPACE", TextKind::SmallMuted),
        badge(theme, src_here!("Badge"), "clean", BadgeVariant::Success),
        text(theme, src_here!("Text"), "BRANCHES", TextKind::SmallMuted),
        virtual_list(theme, src_here!("VirtualList"), branches),
        text(theme, src_here!("Text"), "Stable node IDs make screenshots editable by agents.", TextKind::BodyMuted),
    ]);

    let commits = (0..12).map(|i| {
        let label = match i {
            0 => "a18f2c3  Add retained UI inspector artifacts",
            1 => "7bc91da  Tokenize card and badge variants",
            2 => "52d0aa1  Prototype virtual commit list",
            3 => "c843f10  Add semantic motion presets",
            4 => "36f90db  Render SVG backend for headless review",
            _ => "19a88e4  Polish spacing and row density",
        };
        list_row(theme, src_here!("ListRow"), label, i == 0)
            .key(format!("commit-{i}"))
            .on_action(Msg::SelectCommit(i))
    }).collect();

    let graph_card = card(theme, src_here!("Card"), "Commit Graph", vec![
        text(theme, src_here!("Text"), "Virtualized rows; hover/selection/density are component defaults.", TextKind::BodyMuted),
        virtual_list(theme, src_here!("VirtualList"), commits),
    ]).height(Size::Fill(1.0));

    let staging_card = card(theme, src_here!("Card"), "Staging", vec![
        text(theme, src_here!("Text"), "3 modified files · 1 staged hunk", TextKind::Body),
        badge(theme, src_here!("Badge"), "subject under 72 chars", BadgeVariant::Info),
        toast(theme, src_here!("Toast"), "✓ Screenshot fixture generated"),
    ]).height(Size::Hug);

    let main = column(theme, src_here!("MainColumn"), vec![graph_card, staging_card])
        .width(Size::Fill(1.0));

    let inspector = card(theme, src_here!("Card"), "Agent Inspector", vec![
        text(theme, src_here!("Text"), "Point-pick: node → role → source → style token", TextKind::BodyMuted),
        text(theme, src_here!("Text"), "Artifacts: SVG + tree dump + lint + responsive tape", TextKind::BodyMuted),
        text(theme, src_here!("Text"), "Buttons carry typed Msg actions; layout uses Fill/Hug/Fixed.", TextKind::Mono),
        button(theme, src_here!("Button"), "Open command palette", ButtonVariant::Secondary)
            .width(Size::Fill(1.0))
            .on_action(Msg::OpenCommandPalette),
        button(theme, src_here!("Button"), "Reset --hard", ButtonVariant::Danger)
            .width(Size::Fill(1.0))
            .on_action(Msg::DangerReset),
    ]).width(Size::Fixed(420.0)).height(Size::Fill(1.0));

    let shell = row(theme, src_here!("ShellRow"), vec![sidebar, main, inspector])
        .height(Size::Fill(1.0));

    let toolbar = toolbar(theme, src_here!("Toolbar"), "Whisper Git · LLM-native UI sketch v2", vec![
        badge(theme, src_here!("Badge"), "agent-friendly", BadgeVariant::Info),
        button(theme, src_here!("Button"), "Fetch", ButtonVariant::Ghost).on_action(Msg::Fetch),
        button(theme, src_here!("Button"), "Commit", ButtonVariant::Primary).on_action(Msg::Commit),
    ]);

    app(theme, src_here!("App"), vec![toolbar, shell])
}
