use ncaa_api::RoundKind;
use tui::style::{Color, Modifier, Style};

pub const FRAME_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BannerColor {
    Primary,
    Secondary,
    Accent,
    Shadow,
    Dim,
    Winner,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BannerTheme {
    #[default]
    Dark,
}

pub fn resolve(color: BannerColor, _theme: BannerTheme) -> Style {
    match color {
        BannerColor::Primary => Style::default().fg(Color::Rgb(0, 122, 195)),
        BannerColor::Secondary => Style::default().fg(Color::Rgb(255, 103, 31)),
        BannerColor::Accent => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        BannerColor::Shadow | BannerColor::Dim => Style::default().fg(Color::Indexed(240)),
        BannerColor::Winner => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    }
}

pub fn ball_row(tick: u64, height: u16) -> u16 {
    if height == 0 {
        return 0;
    }
    let h = u64::from(height.saturating_sub(1));
    if h == 0 {
        return 0;
    }
    let period = 2 * h;
    let t = tick % period;
    (h.abs_diff(t)) as u16
}

pub fn basketball_frame(frame: usize) -> [&'static str; 5] {
    const FRAMES: [[&str; 5]; FRAME_COUNT] = [
        ["  .---.  ", " /  |  \\ ", "| --+-- |", " \\  |  / ", "  '---'  "],
        ["  .---.  ", " / / \\ \\ ", "| /   \\ |", " \\ \\ / / ", "  '---'  "],
        ["  .---.  ", " /  -  \\ ", "|-- + --|", " \\  -  / ", "  '---'  "],
        ["  .---.  ", " / \\ / \\ ", "| \\   / |", " / / \\ \\ ", "  '---'  "],
    ];
    FRAMES[frame % FRAME_COUNT]
}

pub fn title_rows() -> [&'static str; 4] {
    [
        " __  __   _   ___  ___ _  _    __  __   _   ___  _  _ ___ ___ ___ ",
        "|  \\/  | /_\\ | _ \\/ __| || |  |  \\/  | /_\\ |   \\| \\| | __/ __/ __|",
        "| |\\/| |/ _ \\|   / (__| __ |  | |\\/| |/ _ \\| |) | .` | _|\\__ \\__ \\",
        "|_|  |_/_/ \\_\\_|_\\\\___|_||_|  |_|  |_/_/ \\_\\___/|_|\\_|___|___/___/",
    ]
}

pub fn round_label(round: RoundKind) -> &'static str {
    match round {
        RoundKind::FirstFour => "FIRST FOUR",
        RoundKind::First => "ROUND OF 64",
        RoundKind::Second => "ROUND OF 32",
        RoundKind::Sweet16 => "SWEET 16",
        RoundKind::Elite8 => "ELITE 8",
        RoundKind::FinalFour => "FINAL FOUR",
        RoundKind::Championship => "CHAMPIONSHIP",
    }
}
