use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Line, Span};
use tui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs};
use tui::{Frame, Terminal};

use crate::app::{App, MenuItem};
use crate::components::banner::AnimatedBanner;
use crate::components::banner_frames::BannerTheme;
use crate::components::bracket::FinalFourView;
use crate::state::network::{ERROR_CHAR, LoadingState};
use crate::ui::layout::LayoutAreas;
use ncaa_api::{Game, GameStatus, Round, RoundKind, TeamSeed};

static TABS: &[&str; 5] = &["Bracket", "Scoreboard", "Game Detail", "Chat", "Pick Wizard"];

pub fn draw<B>(terminal: &mut Terminal<B>, app: &mut App, loading: LoadingState)
where
    B: Backend,
{
    let current_size = terminal.size().unwrap_or_default();
    if current_size.width <= 10 || current_size.height <= 10 {
        return;
    }

    let mut layout = LayoutAreas::new(current_size);

    terminal
        .draw(|f| {
            if app.state.show_intro {
                draw_intro(f, f.area(), app);
                return;
            }

            layout.update(f.area(), app.settings.full_screen);

            if !app.settings.full_screen {
                draw_tabs(f, layout.tab_bar, app);
            }

            match app.state.active_tab {
                MenuItem::Bracket => draw_bracket(f, layout.main, app),
                MenuItem::Scoreboard => draw_scoreboard(f, layout.main, app),
                MenuItem::GameDetail => draw_game_detail(f, layout.main, app),
                MenuItem::Chat => draw_chat(f, layout.main, app),
                MenuItem::PickWizard => draw_pick_wizard(f, layout.main, app),
                MenuItem::Help => draw_placeholder(
                    f,
                    layout.main,
                    "Help: q=quit  1=Bracket  2=Scoreboard  3=GameDetail  4=Chat  5=Wizard  ←/→=round  ↑/↓=game  Enter=select  r=region",
                ),
            }

            draw_loading_spinner(f, f.area(), app, loading);
        })
        .unwrap();
}

pub fn default_border<'a>(color: Color) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
}

fn draw_intro(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::DarkGray).title(" March Madness ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [_top_pad, banner_area, prompt_area, _bottom_pad] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    f.render_widget(
        AnimatedBanner {
            frame: app.state.animation.frame,
            tick: app.state.animation.tick,
            theme: BannerTheme::Dark,
            view_round: app.state.bracket.view_round,
            current_round: app.state.bracket.current_round,
        },
        banner_area,
    );
    f.render_widget(
        Paragraph::new("Press Enter to view bracket")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center),
        prompt_area,
    );
}

fn draw_tabs(f: &mut Frame, tab_bar: [Rect; 2], app: &App) {
    let style = Style::default().fg(Color::White);
    let border_type = BorderType::Rounded;

    let tab_index = match app.state.active_tab {
        MenuItem::Bracket => 0,
        MenuItem::Scoreboard => 1,
        MenuItem::GameDetail => 2,
        MenuItem::Chat => 3,
        MenuItem::PickWizard => 4,
        MenuItem::Help => 0,
    };

    let titles: Vec<Line> = TABS.iter().map(|t| Line::from(*t)).collect();
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::BOTTOM | Borders::TOP)
                .border_type(border_type),
        )
        .highlight_style(Style::default().add_modifier(Modifier::UNDERLINED))
        .select(tab_index)
        .style(style);
    f.render_widget(tabs, tab_bar[0]);

    let help = Paragraph::new("Help: ? ")
        .alignment(Alignment::Right)
        .block(
            Block::default()
                .borders(Borders::RIGHT | Borders::BOTTOM | Borders::TOP)
                .border_type(border_type),
        )
        .style(style);
    f.render_widget(help, tab_bar[1]);
}

fn draw_bracket(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::White).title(" Bracket ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(tournament) = app.state.bracket.tournament.as_ref() else {
        let msg = if let Some(err) = app.state.last_error.as_deref() {
            format!("Bracket load failed:\n{err}")
        } else {
            "Loading bracket data...".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    };

    let [header, key_legend, content] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);

    let region_label = if app.state.bracket.view_round.is_final_four() {
        "National".to_string()
    } else {
        tournament
            .regions
            .iter()
            .filter(|r| r.name != "National")
            .nth(app.state.bracket.selected_region)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Region".to_string())
    };

    let header_text = format!(
        "{} {} | {} | {}",
        tournament.name,
        tournament.year,
        app.state.bracket.view_round.label(),
        region_label
    );
    f.render_widget(Paragraph::new(header_text), header);
    f.render_widget(
        Paragraph::new("Keys: h/l=round  j/k=move  r=region  Enter=details  ?=help  q=quit")
            .style(Style::default().fg(Color::DarkGray)),
        key_legend,
    );

    let mut bracket_area = content;
    let mut live_feed_area: Option<Rect> = None;
    if content.width >= 90 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)]).areas(content);
        bracket_area = left;
        live_feed_area = Some(right);
    } else if content.height >= 16 {
        let [top, bottom] = Layout::vertical([Constraint::Fill(1), Constraint::Length(7)]).areas(content);
        bracket_area = top;
        live_feed_area = Some(bottom);
    }

    if app.state.bracket.view_round == RoundKind::Championship {
        draw_championship_view(f, bracket_area, tournament);
    } else if app.state.bracket.view_round.is_final_four() {
        draw_final_four_view(f, bracket_area, tournament, app);
    } else {
        draw_all_regions_view(f, bracket_area, tournament, app);
    }

    if let Some(feed) = live_feed_area {
        draw_live_feed(f, feed, app);
    }
}

fn draw_all_regions_view(f: &mut Frame, area: Rect, tournament: &ncaa_api::Tournament, app: &App) {
    let regions: Vec<_> = tournament
        .regions
        .iter()
        .filter(|r| r.name != "National")
        .collect();

    if regions.is_empty() {
        f.render_widget(
            Paragraph::new("No region data found")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let [top_row, middle_gap, bottom_row] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [top_left, _top_mid, top_right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(top_row);
    let [bottom_left, _bottom_mid, bottom_right] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(bottom_row);

    let panes: [Rect; 4] = [top_left, top_right, bottom_left, bottom_right];
    let connector_center = Rect::new(
        area.x + area.width.saturating_sub(3) / 2,
        middle_gap.y,
        3,
        1,
    );

    for (idx, pane) in panes.into_iter().enumerate() {
        if let Some(region) = regions.get(idx) {
            let pane_block = default_border(if idx == app.state.bracket.selected_region {
                Color::Yellow
            } else {
                Color::DarkGray
            })
            .title(format!(" {} ", region.name));
            let pane_inner = pane_block.inner(pane);
            f.render_widget(pane_block, pane);

            draw_round_compact(
                f,
                pane_inner,
                round_games(region.rounds.as_slice(), app.state.bracket.view_round).unwrap_or(&[]),
                idx == app.state.bracket.selected_region,
                app.state.bracket.selected_game,
            );
        }
    }

    if app.state.bracket.view_round == RoundKind::Elite8 {
        draw_region_champion_connectors(f, panes, connector_center);
    }
}

fn draw_round_compact(
    f: &mut Frame,
    area: Rect,
    games: &[Game],
    selected_region: bool,
    selected_game: usize,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if games.is_empty() {
        f.render_widget(
            Paragraph::new("No games")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let entries = build_round_entries(games);
    let rows = area.height as usize;
    let cols = entries.len().max(1).div_ceil(rows.max(1));
    let col_width = (area.width as usize / cols.max(1)).max(1);
    let use_abbrev = col_width < 26;

    f.render_widget(Clear, area);

    for (idx, mut entry) in entries.into_iter().enumerate() {
        let row = idx % rows.max(1);
        let col = idx / rows.max(1);
        if col >= cols {
            break;
        }
        let game_idx = idx / 2;
        let marker = if selected_region && game_idx == selected_game && idx % 2 == 0 {
            '>'
        } else {
            ' '
        };
        if use_abbrev && entry.chars().count() > 5 {
            entry = abbreviate_entry(&entry);
        }
        let clipped: String = entry.chars().take(col_width.saturating_sub(2)).collect();
        let line = format!("{marker} {clipped}");
        let x = area.x + (col * col_width) as u16;
        let y = area.y + row as u16;
        f.render_widget(Paragraph::new(line), Rect::new(x, y, col_width as u16, 1));
    }
}

fn build_round_entries(games: &[Game]) -> Vec<String> {
    let mut entries = Vec::with_capacity(games.len() * 2);
    for g in games {
        let status = match g.status {
            GameStatus::Scheduled => "SCH",
            GameStatus::InProgress => "LIVE",
            GameStatus::Final => "FNL",
            GameStatus::Postponed => "PPD",
        };
        entries.push(format!(
            "{} {status}",
            compact_team(g.top.seed, &g.top, g.score.map(|(s, _)| s), false)
        ));
        entries.push(format!(
            "{} {status}",
            compact_team(g.bottom.seed, &g.bottom, g.score.map(|(_, s)| s), false)
        ));
    }
    entries
}

fn compact_team(seed: u8, team_seed: &TeamSeed, score: Option<u16>, use_abbrev: bool) -> String {
    let name = team_seed
        .team
        .as_ref()
        .map(|t| {
            if use_abbrev {
                t.abbrev.clone()
            } else {
                t.short_name.clone()
            }
        })
        .or_else(|| team_seed.placeholder.clone())
        .unwrap_or_else(|| "TBD".to_string());
    let score = score
        .map(|s| format!("{s:>2}"))
        .unwrap_or_else(|| "--".to_string());
    let seed = if seed > 0 {
        format!("{seed:>2}")
    } else {
        "--".to_string()
    };
    format!("{seed} {} {score}", truncate_name(&name, if use_abbrev { 5 } else { 10 }))
}

fn abbreviate_entry(entry: &str) -> String {
    let mut out = String::new();
    for token in entry.split_whitespace().take(3) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}

fn truncate_name(name: &str, max: usize) -> String {
    let mut s: String = name.chars().take(max).collect();
    while s.chars().count() < max {
        s.push(' ');
    }
    s
}

fn draw_region_champion_connectors(f: &mut Frame, panes: [Rect; 4], center: Rect) {
    let style = Style::default().fg(Color::DarkGray);
    let target_x = center.x + center.width / 2;
    let target_y = center.y;
    draw_text_cell(f, target_x, target_y, "★", style);

    // Bottom two regions feed upward toward the center/finals marker.
    for pane in [panes[2], panes[3]] {
        let start_x = pane.x + pane.width / 2;
        let start_y = if pane.y < target_y {
            pane.y + pane.height.saturating_sub(1)
        } else {
            pane.y
        };
        draw_diagonal(f, start_x, start_y, target_x, target_y, style);
    }
}

fn draw_diagonal(f: &mut Frame, mut x: u16, mut y: u16, tx: u16, ty: u16, style: Style) {
    while y != ty || x != tx {
        if y < ty {
            y += 1;
        } else if y > ty {
            y -= 1;
        }

        if x < tx {
            x += 1;
        } else if x > tx {
            x -= 1;
        }

        let ch = if x < tx {
            '╲'
        } else if x > tx {
            '╱'
        } else {
            '│'
        };
        draw_text_cell(f, x, y, &ch.to_string(), style);
    }
}

fn draw_text_cell(f: &mut Frame, x: u16, y: u16, text: &str, style: Style) {
    let area = f.area();
    if x >= area.x + area.width || y >= area.y + area.height {
        return;
    }
    f.render_widget(Paragraph::new(text).style(style), Rect::new(x, y, 1, 1));
}

fn draw_final_four_view(f: &mut Frame, area: Rect, tournament: &ncaa_api::Tournament, app: &App) {
    let national = tournament.regions.iter().find(|r| r.name == "National");
    let semifinals = national.and_then(|r| round_games(r.rounds.as_slice(), RoundKind::FinalFour));
    let championship = national.and_then(|r| round_games(r.rounds.as_slice(), RoundKind::Championship));

    let selected_idx = if app.state.bracket.view_round == RoundKind::Championship {
        2
    } else {
        app.state.bracket.selected_game.min(1)
    };

    f.render_widget(
        FinalFourView {
            semi_left: semifinals.and_then(|g| g.first()),
            semi_right: semifinals.and_then(|g| g.get(1)),
            championship: championship.and_then(|g| g.first()),
            selected_idx,
            theme: BannerTheme::Dark,
        },
        area,
    );
}

fn draw_championship_view(f: &mut Frame, area: Rect, tournament: &ncaa_api::Tournament) {
    let national = tournament.regions.iter().find(|r| r.name == "National");
    let championship = national.and_then(|r| round_games(r.rounds.as_slice(), RoundKind::Championship));
    let Some(game) = championship.and_then(|g| g.first()) else {
        f.render_widget(
            Paragraph::new("No championship game available")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };

    let top = format_seed_team(&game.top, game.score.map(|(s, _)| s));
    let bot = format_seed_team(&game.bottom, game.score.map(|(_, s)| s));
    let status = match game.status {
        GameStatus::Final => "FINAL".to_string(),
        GameStatus::InProgress => format!(
            "LIVE {} {}",
            game.period.unwrap_or_default(),
            game.clock.clone().unwrap_or_default()
        ),
        GameStatus::Postponed => "PPD".to_string(),
        GameStatus::Scheduled => game
            .start_time
            .map(|t| t.format("%m/%d %I:%M%p").to_string())
            .unwrap_or_else(|| "SCHEDULED".to_string()),
    };
    let text = format!("NCAA Championship\n\n{top}\nvs\n{bot}\n\n[{status}]");
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center),
        area,
    );
}

fn draw_scoreboard(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::White).title(" Scoreboard ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(tournament) = app.state.bracket.tournament.as_ref() else {
        f.render_widget(
            Paragraph::new("No tournament loaded. Return to Bracket tab.")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let region = if app.state.bracket.view_round.is_final_four() {
        tournament.regions.iter().find(|r| r.name == "National")
    } else {
        tournament
            .regions
            .iter()
            .filter(|r| r.name != "National")
            .nth(app.state.bracket.selected_region)
    };

    let Some(region) = region else {
        f.render_widget(Paragraph::new("No region available"), inner);
        return;
    };

    let games = round_games(region.rounds.as_slice(), app.state.bracket.view_round).unwrap_or(&[]);
    if games.is_empty() {
        f.render_widget(Paragraph::new("No games in this round"), inner);
        return;
    }

    let mut lines = Vec::with_capacity(games.len() + 3);
    lines.push(format!("{} | {}", region.name, app.state.bracket.view_round.label()));
    lines.push("j/k to move, Enter for detail, r to cycle region".to_string());
    lines.push(String::new());

    for (idx, game) in games.iter().enumerate() {
        let marker = if idx == app.state.bracket.selected_game { ">" } else { " " };
        let status = match game.status {
            GameStatus::Final => "FINAL".to_string(),
            GameStatus::InProgress => format!("LIVE {} {}", game.period.unwrap_or_default(), game.clock.clone().unwrap_or_default()),
            GameStatus::Postponed => "PPD".to_string(),
            GameStatus::Scheduled => game
                .start_time
                .map(|t| t.format("%m/%d %I:%M%p").to_string())
                .unwrap_or_else(|| "SCHEDULED".to_string()),
        };

        let top = format_seed_team(&game.top, game.score.map(|(s, _)| s));
        let bot = format_seed_team(&game.bottom, game.score.map(|(_, s)| s));
        lines.push(format!("{marker} {top} vs {bot}  [{status}]"));
    }

    f.render_widget(Paragraph::new(lines.join("\n")), inner);
}

fn draw_game_detail(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::White).title(" Game Detail ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(detail) = app.state.game_detail.detail.as_ref() else {
        let msg = if let Some(err) = app.state.last_error.as_deref() {
            format!("Load failed:\n{err}")
        } else {
            "Select a game in Bracket or Scoreboard and press Enter".to_string()
        };
        f.render_widget(Paragraph::new(msg), inner);
        return;
    };

    let mut lines = Vec::new();
    lines.push(format!("Game ID: {}", detail.game_id));
    lines.push(String::new());

    let away_name = detail
        .away_box
        .team
        .as_ref()
        .map(|t| t.short_name.as_str())
        .unwrap_or("Away");
    let home_name = detail
        .home_box
        .team
        .as_ref()
        .map(|t| t.short_name.as_str())
        .unwrap_or("Home");

    lines.push(format!("{} totals: {} PTS, {} REB, {} AST", away_name, detail.away_box.totals.points, detail.away_box.totals.rebounds, detail.away_box.totals.assists));
    lines.push(format!("{} totals: {} PTS, {} REB, {} AST", home_name, detail.home_box.totals.points, detail.home_box.totals.rebounds, detail.home_box.totals.assists));
    lines.push(String::new());
    lines.push("Recent Plays: (j/k scroll)".to_string());

    let max_lines = inner.height.saturating_sub(lines.len() as u16) as usize;
    let offset = app.state.game_detail.scroll_offset as usize;
    for p in detail.plays.iter().skip(offset).take(max_lines.max(1)) {
        lines.push(format!("P{} {}  {}-{}  {}", p.period, p.clock, p.away_score, p.home_score, p.description));
    }

    f.render_widget(Paragraph::new(lines.join("\n")), inner);
}

fn draw_chat(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::White).title(" Chat ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let [messages_area, input_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(inner);

    let mut lines = Vec::new();
    let status = if app.state.chat.connected { "online" } else { "offline" };
    lines.push(Line::from(vec![
        Span::styled("room ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.state.chat.room.as_str(), Style::default().fg(Color::Gray)),
        Span::styled("  status ", Style::default().fg(Color::DarkGray)),
        Span::styled(status, Style::default().fg(if app.state.chat.connected { Color::Green } else { Color::Red })),
    ]));
    lines.push(Line::from(""));

    for msg in &app.state.chat.messages {
        let prefix = format!("[{}] {}: ", msg.timestamp, msg.author);
        let style = if msg.is_system {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let body_width = messages_area
            .width
            .saturating_sub(prefix.chars().count() as u16)
            .max(8) as usize;
        let clipped: String = msg.body.chars().take(body_width).collect();
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(clipped, style),
        ]));
    }

    let visible = messages_area.height as usize;
    let total = lines.len();
    let offset = app.state.chat.scroll_offset as usize;
    let end = total.saturating_sub(offset);
    let start = end.saturating_sub(visible);
    let window = if start < end { lines[start..end].to_vec() } else { Vec::new() };
    f.render_widget(Paragraph::new(window), messages_area);

    let mode = if app.state.chat.composing { "typing" } else { "idle" };
    let input = if app.state.chat.composing {
        format!("> {}_", app.state.chat.input)
    } else {
        "Press Enter/i to type. Esc cancel. j/k scroll. Set MMTUI_CHAT_WS for remote relay."
            .to_string()
    };
    let input_style = if app.state.chat.composing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let input_block = default_border(Color::DarkGray).title(format!(" {} ", mode));
    let input_inner = input_block.inner(input_area);
    f.render_widget(input_block, input_area);
    f.render_widget(
        Paragraph::new(input).style(input_style),
        input_inner,
    );
}

fn format_seed_team(seed: &TeamSeed, score: Option<u16>) -> String {
    let seed_str = if seed.seed > 0 {
        format!("{}", seed.seed)
    } else {
        "-".to_string()
    };
    let team = seed
        .team
        .as_ref()
        .map(|t| t.short_name.clone())
        .or_else(|| seed.placeholder.clone())
        .unwrap_or_else(|| "TBD".to_string());
    let score = score.map_or("--".to_string(), |s| s.to_string());
    format!("({seed_str}) {team} {score}")
}

fn round_games(rounds: &[Round], kind: RoundKind) -> Option<&[Game]> {
    rounds
        .iter()
        .find(|r| r.kind == kind)
        .map(|r| r.games.as_slice())
}

fn draw_placeholder(f: &mut Frame, area: Rect, msg: &str) {
    let block = default_border(Color::DarkGray);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        inner,
    );
}

fn draw_loading_spinner(f: &mut Frame, area: Rect, app: &App, loading: LoadingState) {
    if !loading.is_loading && loading.spinner_char != ERROR_CHAR {
        return;
    }
    let style = match loading.spinner_char {
        ERROR_CHAR => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::White),
    };
    let spinner = Paragraph::new(loading.spinner_char.to_string())
        .alignment(Alignment::Right)
        .style(style);
    let area = if app.settings.full_screen {
        Rect::new(area.width.saturating_sub(3), area.height.saturating_sub(2), 1, 1)
    } else {
        Rect::new(area.width.saturating_sub(11), 1, 1, 1)
    };
    f.render_widget(spinner, area);
}

fn draw_live_feed(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::DarkGray).title(" Live Feed ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let Some(game_id) = app.state.live_feed.game_id.as_deref() else {
        f.render_widget(
            Paragraph::new("Select a game to load live plays")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    };

    if app.state.live_feed.plays.is_empty() {
        f.render_widget(
            Paragraph::new(format!("Game {game_id}\nNo plays yet"))
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Game: ", Style::default().fg(Color::Gray)),
        Span::raw(game_id),
    ]));
    lines.push(Line::from(""));

    let max_plays = inner.height.saturating_sub(2) as usize;
    for play in app.state.live_feed.plays.iter().rev().take(max_plays) {
        let style = if play.is_new {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let text = format!(
            "{} P{} {}-{} {}",
            play.clock, play.period, play.away_score, play.home_score, play.description
        );
        let clipped: String = text.chars().take(inner.width.saturating_sub(1) as usize).collect();
        lines.push(Line::from(Span::styled(clipped, style)));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_pick_wizard(f: &mut Frame, area: Rect, app: &App) {
    let block = default_border(Color::White).title(" Pick Wizard (2025 Template) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let wizard = &app.state.pick_wizard;
    if wizard.games.is_empty() {
        f.render_widget(
            Paragraph::new("No wizard games loaded yet. Load bracket then press 5 again.")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "Progress: {}/{} picks",
        wizard.selections.len(),
        wizard.games.len()
    )));
    lines.push(Line::from("Keys: 1=top  2=bottom  j/k=next/prev  s=save  Esc=back"));
    lines.push(Line::from(""));

    if wizard.completed {
        lines.push(Line::from(Span::styled(
            "Wizard complete. Picks saved to ~/.config/mmtui/picks_2025.json",
            Style::default().fg(Color::Green),
        )));
    } else if let Some(game) = wizard.current_game() {
        lines.push(Line::from(format!(
            "Game {}/{}  |  {}",
            wizard.current_index + 1,
            wizard.games.len(),
            game.round.label()
        )));
        lines.push(Line::from(""));

        let top_selected = wizard
            .selections
            .get(&game.game_id)
            .and_then(|w| game.top_team_id.as_ref().map(|id| id == w))
            .unwrap_or(false);
        let bottom_selected = wizard
            .selections
            .get(&game.game_id)
            .and_then(|w| game.bottom_team_id.as_ref().map(|id| id == w))
            .unwrap_or(false);

        lines.push(Line::from(vec![
            Span::styled(
                "1) ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                game.top_label.clone(),
                if top_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                "2) ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                game.bottom_label.clone(),
                if bottom_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}
