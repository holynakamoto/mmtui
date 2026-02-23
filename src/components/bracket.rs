use ncaa_api::{Game, GameStatus, RoundKind, TeamSeed};
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::widgets::Widget;

use crate::components::banner_frames::{BannerColor, BannerTheme, resolve};

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

/// Rows per game cell: top-team line, score/status line, bottom-team line.
pub const GAME_HEIGHT: u16 = 3;

/// Slot heights for each bracket depth (d=0 = First/R64 leaf, d=3 = Elite8 root).
/// Formula: SH[0] = GAME_HEIGHT; SH[d] = 2 * SH[d-1] + 1.
const SH: [u16; 4] = [
    GAME_HEIGHT,                                         // First:   3
    2 * GAME_HEIGHT + 1,                                 // Second:  7
    2 * (2 * GAME_HEIGHT + 1) + 1,                      // Sweet16: 15
    2 * (2 * (2 * GAME_HEIGHT + 1) + 1) + 1,            // Elite8:  31
];

/// Total terminal rows consumed by one regional bracket. Equals SH[3] = 31.
pub const REGION_HEIGHT: u16 = SH[3];

/// Width of the connector zone drawn between adjacent round columns.
pub const CONNECTOR_WIDTH: u16 = 3;

/// Maximum game cell width in wider terminals.
const CELL_W_FULL: u16 = 22;

// ---------------------------------------------------------------------------
// GameCell — pre-computed position for one game
// ---------------------------------------------------------------------------

/// Pre-computed layout position for one game within a regional bracket grid.
#[derive(Debug, Clone)]
pub struct GameCell {
    /// Row index of the score/status line (center of the 3-row cell).
    /// Relative to the bracket origin (0 = top of region). Not scroll-adjusted.
    pub center_row: u16,
    /// Starting x-column for this game cell within the grid (origin-relative).
    pub col: u16,
    /// Width of the game cell in terminal columns.
    pub cell_width: u16,
    /// Whether to draw a rightward connector from this cell to its parent round.
    /// False for Elite8 — those games have no parent column on the right.
    #[allow(dead_code)]
    pub draw_outbound_connector: bool,
    /// Round this cell belongs to.
    pub round: RoundKind,
    /// Index of this game within the round's game list (0-based).
    pub game_idx: usize,
}

// ---------------------------------------------------------------------------
// BracketGrid — layout engine for one regional bracket
// ---------------------------------------------------------------------------

/// Pre-computed bracket layout for one 4-round regional bracket (First → Elite8).
///
/// Column order left → right: First | conn | Second | conn | Sweet16 | conn | Elite8
#[derive(Debug, Clone)]
pub struct BracketGrid {
    /// All cells in depth-major order: First(8) + Second(4) + Sweet16(2) + Elite8(1) = 15 cells.
    pub cells: Vec<GameCell>,
    /// Starting x-column (within the grid, origin-relative) for each round column.
    /// Index: [0=First, 1=Second, 2=Sweet16, 3=Elite8].
    pub round_cols: [u16; 4],
    /// Total grid width in terminal columns.
    #[allow(dead_code)]
    pub total_width: u16,
    /// Total grid height in terminal rows (= REGION_HEIGHT = 31).
    #[allow(dead_code)]
    pub total_height: u16,
    /// Cell width used (chosen by terminal_width at compute time).
    pub cell_width: u16,
    /// When true, depth 0 (R64) is on the right and depth 3 (E8) is on the left.
    pub mirrored: bool,
    /// When true, row positions are flipped vertically (R64 at bottom, E8 at top).
    /// Used for bottom panes so the bracket points up toward the Final Four.
    pub flipped: bool,
}

impl BracketGrid {
    /// Compute the bracket layout for the given terminal width.
    ///
    /// Each pane has 4 game columns and 3 connector columns:
    /// `4 * cell_width + 3 * CONNECTOR_WIDTH <= terminal_width`.
    /// Cell width is chosen dynamically to fit the pane.
    ///
    /// Center rows follow the triangle formula:
    ///   center[d][i] = SH[d]/2 + i * (SH[d+1] - SH[d])
    ///
    /// Resulting center rows per round:
    ///   First   (d=0): [1, 5, 9, 13, 17, 21, 25, 29]  (spacing 4)
    ///   Second  (d=1): [3, 11, 19, 27]                 (spacing 8)
    ///   Sweet16 (d=2): [7, 23]                         (spacing 16)
    ///   Elite8  (d=3): [15]
    pub fn compute(terminal_width: u16) -> Self {
        Self::compute_inner(terminal_width, false, false)
    }

    /// Depth 0 (R64) on the right, depth 3 (E8) on the left — for right-side panes.
    pub fn compute_mirrored(terminal_width: u16) -> Self {
        Self::compute_inner(terminal_width, true, false)
    }

    /// R64 at the bottom row, E8 at the top — for bottom-left panes (South).
    pub fn compute_flipped(terminal_width: u16) -> Self {
        Self::compute_inner(terminal_width, false, true)
    }

    /// Flipped vertically AND mirrored horizontally — for bottom-right panes (Midwest).
    pub fn compute_flipped_mirrored(terminal_width: u16) -> Self {
        Self::compute_inner(terminal_width, true, true)
    }

    /// Shared layout engine.
    ///
    /// `mirrored`: reverse horizontal column order (R64 right, E8 left).
    /// `flipped`:  reverse vertical row order (R64 bottom, E8 top).
    ///
    /// Center rows per depth (normal):
    ///   First   (d=0): [1, 5, 9, 13, 17, 21, 25, 29]  (spacing 4)
    ///   Second  (d=1): [3, 11, 19, 27]                 (spacing 8)
    ///   Sweet16 (d=2): [7, 23]                         (spacing 16)
    ///   Elite8  (d=3): [15]
    fn compute_inner(terminal_width: u16, mirrored: bool, flipped: bool) -> Self {
        let connector_total = CONNECTOR_WIDTH * 3;
        let per_col = terminal_width.saturating_sub(connector_total) / 4;
        let cell_width: u16 = per_col.max(1).min(CELL_W_FULL);
        let stride = cell_width + CONNECTOR_WIDTH;
        let round_cols = if mirrored {
            [stride * 3, stride * 2, stride, 0u16]
        } else {
            [0u16, stride, stride * 2, stride * 3]
        };
        let total_width = stride * 3 + cell_width;

        let first_center = [SH[0] / 2, SH[1] / 2, SH[2] / 2, SH[3] / 2]; // [1, 3, 7, 15]
        let spacing: [u16; 4] = [SH[1] - SH[0], SH[2] - SH[1], SH[3] - SH[2], 0];
        let game_counts = [8usize, 4, 2, 1];
        let round_kinds = [RoundKind::First, RoundKind::Second, RoundKind::Sweet16, RoundKind::Elite8];

        let mut cells = Vec::with_capacity(15);
        for d in 0..4usize {
            for i in 0..game_counts[d] {
                let center_normal = first_center[d] + i as u16 * spacing[d];
                let center_row = if flipped {
                    (REGION_HEIGHT - 1) - center_normal
                } else {
                    center_normal
                };
                cells.push(GameCell {
                    center_row,
                    col: round_cols[d],
                    cell_width,
                    draw_outbound_connector: d < 3,
                    round: round_kinds[d],
                    game_idx: i,
                });
            }
        }

        Self { cells, round_cols, total_width, total_height: REGION_HEIGHT, cell_width, mirrored, flipped }
    }

    /// Cells for a specific depth (0=First, 1=Second, 2=Sweet16, 3=Elite8).
    pub fn cells_for_depth(&self, depth: usize) -> &[GameCell] {
        const OFFSETS: [usize; 5] = [0, 8, 12, 14, 15];
        &self.cells[OFFSETS[depth]..OFFSETS[depth + 1]]
    }
}

// ---------------------------------------------------------------------------
// BracketView widget
// ---------------------------------------------------------------------------

/// Renders a single-region bracket (First Round → Elite Eight).
pub struct BracketView<'a> {
    /// Game slices per bracket depth: [First(8), Second(4), Sweet16(2), Elite8(1)].
    /// Each inner slice may be shorter than expected if the round hasn't started yet.
    pub rounds: [&'a [Game]; 4],
    /// Pre-computed layout. Rebuild only on terminal resize.
    pub grid: &'a BracketGrid,
    /// Bracket depth index (0–3) of the highlighted round.
    pub selected_depth: usize,
    /// Game index within the selected depth that is highlighted.
    pub selected_game: usize,
    /// Vertical scroll offset in terminal rows (supports tall brackets on short terminals).
    pub scroll_offset: u16,
    /// Color theme.
    pub theme: BannerTheme,
    /// Mirror the bracket so depth 0 (R64) is on the right (for right-side panes).
    pub mirrored: bool,
}

impl<'a> Widget for BracketView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 20 || area.height < GAME_HEIGHT {
            return;
        }

        // Pass 1: draw all game cells (3-row boxes)
        for cell in &self.grid.cells {
            let depth = round_to_depth(cell.round);
            let game = self.rounds[depth].get(cell.game_idx);
            let selected =
                depth == self.selected_depth && cell.game_idx == self.selected_game;
            draw_game_cell(game, cell, selected, area, self.scroll_offset, self.theme, buf);
        }

        // Pass 2: draw box-drawing connectors between adjacent rounds.
        // For depth d, each parent at depth d+1 connects to two children at depth d.
        for depth in 0..3usize {
            let child_cells = self.grid.cells_for_depth(depth);
            let parent_cells = self.grid.cells_for_depth(depth + 1);
            // Normal: connector zone is to the right of the child column.
            // Mirrored: connector zone is to the left of the child column.
            let conn_x_base = if self.mirrored {
                area.x + self.grid.round_cols[depth].saturating_sub(CONNECTOR_WIDTH)
            } else {
                area.x + self.grid.round_cols[depth] + self.grid.cell_width
            };

            for (j, parent) in parent_cells.iter().enumerate() {
                // Sort by center_row so r_top < r_mid < r_bot in both normal and flipped modes.
                let ca = &child_cells[2 * j];
                let cb = &child_cells[2 * j + 1];
                let (child_top, child_bot) = if ca.center_row <= cb.center_row { (ca, cb) } else { (cb, ca) };
                draw_connector(
                    child_top.center_row,
                    parent.center_row,
                    child_bot.center_row,
                    conn_x_base,
                    area,
                    self.scroll_offset,
                    self.theme,
                    self.mirrored,
                    buf,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FinalFourView widget
// ---------------------------------------------------------------------------

/// Renders the Final Four + Championship in a centered tri-panel layout:
///
/// ```text
///           ── FINAL FOUR ──
///
///  1 TeamA  87  ──────────────────  1 TeamC  68
///    FINAL       1 Winner           FINAL
/// 16 TeamB  72   championship    16 TeamD  59
///                1 Winner
/// ```
pub struct FinalFourView<'a> {
    pub semi_left: Option<&'a Game>,
    pub semi_right: Option<&'a Game>,
    pub championship: Option<&'a Game>,
    /// 0 = semi_left, 1 = semi_right, 2 = championship.
    pub selected_idx: usize,
    pub theme: BannerTheme,
}

impl<'a> Widget for FinalFourView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 5 {
            return;
        }

        let accent = resolve(BannerColor::Accent, self.theme);
        let dim = resolve(BannerColor::Dim, self.theme);

        // Title
        let title = "── FINAL FOUR ──";
        let tx = area.x + area.width.saturating_sub(title.len() as u16) / 2;
        if area.height >= 1 {
            buf.set_string(tx, area.y, title, accent);
        }

        if area.height < 5 {
            return;
        }

        // Layout: three columns side-by-side, connected by horizontal lines.
        let cell_w: u16 = if area.width >= 72 { 22 } else { 18 };
        let gap: u16 = 4;
        let total_w = cell_w * 3 + gap * 2;

        if total_w + 2 > area.width {
            // Fallback: stack vertically
            render_ff_vertical(&self, area, buf);
            return;
        }

        let x0 = area.x + (area.width - total_w) / 2;
        let col_left = x0;
        let col_mid = x0 + cell_w + gap;
        let col_right = x0 + (cell_w + gap) * 2;

        // All three games at the same vertical center_row (relative to area).
        let center_y = area.y + 2; // leave row 0 for title, row 1 blank

        // Draw game cells
        draw_ff_game_at(self.semi_left, col_left, center_y, cell_w, self.selected_idx == 0, self.theme, buf, area);
        draw_ff_game_at(self.championship, col_mid, center_y, cell_w, self.selected_idx == 2, self.theme, buf, area);
        draw_ff_game_at(self.semi_right, col_right, center_y, cell_w, self.selected_idx == 1, self.theme, buf, area);

        // Horizontal connectors at center_y (score row):
        //   [semi_left right edge] ──── [champ left edge]
        //   [champ right edge]    ──── [semi_right left edge]
        let limit_x = area.x + area.width;
        let left_conn_start = col_left + cell_w;
        let left_conn_end = col_mid;
        for cx in left_conn_start..left_conn_end {
            if cx >= limit_x { break; }
            put_char(buf, cx, center_y, '─', dim);
        }

        let right_conn_start = col_mid + cell_w;
        let right_conn_end = col_right;
        for cx in right_conn_start..right_conn_end {
            if cx >= limit_x { break; }
            put_char(buf, cx, center_y, '─', dim);
        }

        // T-junction marks at championship borders on the score row
        if col_mid > 0 && col_mid - 1 >= area.x && col_mid - 1 < limit_x {
            put_char(buf, col_mid - 1, center_y, '┤', dim);
        }
        let rj = col_mid + cell_w;
        if rj < limit_x {
            put_char(buf, rj, center_y, '├', dim);
        }
    }
}

/// Vertical fallback for FinalFourView when the terminal is too narrow.
fn render_ff_vertical(view: &FinalFourView, area: Rect, buf: &mut Buffer) {
    let accent = resolve(BannerColor::Accent, view.theme);
    let cell_w = (area.width.saturating_sub(2)) as usize;

    let mut y = area.y + 1;
    let games = [
        ("── Semi 1 ──", view.semi_left, view.selected_idx == 0),
        ("── Semi 2 ──", view.semi_right, view.selected_idx == 1),
        ("── Championship ──", view.championship, view.selected_idx == 2),
    ];

    for (label, game, selected) in games {
        if y >= area.y + area.height { break; }
        buf.set_string(area.x, y, label, accent);
        y += 1;

        for slot in 0u8..3 {
            if y >= area.y + area.height { break; }
            let content = format_game_row(game, slot, cell_w, view.theme);
            let style = if selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            buf.set_string(area.x + 1, y, &content, style);
            y += 1;
        }
        y += 1; // blank spacer
    }
}

/// Draw a 3-row game cell at absolute screen coordinates (no scroll).
fn draw_ff_game_at(
    game: Option<&Game>,
    x: u16,
    center_y: u16,
    cell_w: u16,
    selected: bool,
    theme: BannerTheme,
    buf: &mut Buffer,
    area: Rect,
) {
    let base_style = if selected {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let primary = resolve(BannerColor::Primary, theme);
    let dim = resolve(BannerColor::Dim, theme);
    let winner_style = resolve(BannerColor::Winner, theme);

    let limit_x = area.x + area.width;
    let avail = limit_x.saturating_sub(x) as usize;

    for (dy, slot_idx) in [(0u16, 0u8), (1, 1), (2, 2)] {
        let y = center_y.saturating_sub(1) + dy;
        if y < area.y || y >= area.y + area.height { continue; }
        if x >= limit_x { continue; }

        let content = format_game_row(game, slot_idx, cell_w as usize, theme);
        let text: String = content.chars().take(avail).collect();

        let style = match slot_idx {
            1 => match game.map(|g| &g.status) {
                Some(GameStatus::InProgress) => primary,
                _ => dim,
            },
            _ => {
                let is_winner = game.map(|g| {
                    let ts = if slot_idx == 0 { &g.top } else { &g.bottom };
                    ts.team.as_ref()
                        .and_then(|t| g.winner_id.as_deref().map(|wid| t.id == wid))
                        .unwrap_or(false)
                }).unwrap_or(false);
                if is_winner { winner_style.add_modifier(Modifier::BOLD) } else { base_style }
            }
        };

        buf.set_string(x, y, &text, style);
    }
}

// ---------------------------------------------------------------------------
// Shared drawing helpers
// ---------------------------------------------------------------------------

pub fn round_to_depth(round: RoundKind) -> usize {
    match round {
        RoundKind::First => 0,
        RoundKind::Second => 1,
        RoundKind::Sweet16 => 2,
        RoundKind::Elite8 => 3,
        _ => 0,
    }
}

/// Convert a bracket-relative row to an absolute screen y, applying scroll + area bounds.
/// Returns `None` if the row is off-screen.
fn screen_y(bracket_row: u16, scroll: u16, area: Rect) -> Option<u16> {
    if bracket_row < scroll {
        return None;
    }
    let rel = bracket_row - scroll;
    if rel >= area.height {
        return None;
    }
    Some(area.y + rel)
}

/// Draw one game cell (3 rows) into the buffer, with scroll + clip handling.
fn draw_game_cell(
    game: Option<&Game>,
    cell: &GameCell,
    selected: bool,
    area: Rect,
    scroll: u16,
    theme: BannerTheme,
    buf: &mut Buffer,
) {
    let primary = resolve(BannerColor::Primary, theme);
    let winner_style = resolve(BannerColor::Winner, theme);
    let dim = resolve(BannerColor::Dim, theme);

    let x = area.x + cell.col;
    if x >= area.x + area.width {
        return;
    }
    let avail_w = (area.x + area.width).saturating_sub(x) as usize;

    let base_style = if selected {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    // The three bracket rows: top-team = center-1, status = center, bottom-team = center+1.
    // center_row is always >= 1 (minimum is 1 for the first First-round game), so
    // saturating_sub avoids underflow even if something unexpected happens.
    let top_row = cell.center_row.saturating_sub(1);
    let mid_row = cell.center_row;
    let bot_row = cell.center_row.saturating_add(1);

    for (bracket_row, slot_idx) in [(top_row, 0u8), (mid_row, 1), (bot_row, 2)] {
        let Some(sy) = screen_y(bracket_row, scroll, area) else {
            continue;
        };

        let content = format_game_row(game, slot_idx, cell.cell_width as usize, theme);
        let text: String = content.chars().take(avail_w).collect();

        let style = match slot_idx {
            1 => match game.map(|g| &g.status) {
                Some(GameStatus::InProgress) => primary,
                _ => dim,
            },
            _ => {
                let is_winner = game.map(|g| {
                    let ts = if slot_idx == 0 { &g.top } else { &g.bottom };
                    ts.team
                        .as_ref()
                        .and_then(|t| g.winner_id.as_deref().map(|wid| t.id == wid))
                        .unwrap_or(false)
                }).unwrap_or(false);

                if is_winner {
                    winner_style.add_modifier(Modifier::BOLD)
                } else {
                    base_style
                }
            }
        };

        buf.set_string(x, sy, &text, style);
    }
}

/// Format a single game cell row.
/// `slot_idx`: 0 = top-team line, 1 = score/status line, 2 = bottom-team line.
fn format_game_row(game: Option<&Game>, slot_idx: u8, width: usize, theme: BannerTheme) -> String {
    let _ = theme; // reserved for future per-theme score color variations
    match game {
        None => " ".repeat(width),
        Some(g) => match slot_idx {
            0 => format_team_line(&g.top, g.score.map(|(s, _)| s), width),
            2 => format_team_line(&g.bottom, g.score.map(|(_, s)| s), width),
            _ => format_status_line(g, width),
        },
    }
}

/// Format a team/seed line: `"[seed] [name       ] [score]"`
///
/// Total width = seed(2) + " " + name(width-7) + " " + score(3) + " " = width.
fn format_team_line(ts: &TeamSeed, score: Option<u16>, width: usize) -> String {
    let seed = if ts.seed > 0 {
        format!("{:2}", ts.seed)
    } else {
        "  ".to_string()
    };
    let name = ts.team.as_ref().map(|t| t.short_name.as_str())
        .or(ts.placeholder.as_deref())
        .unwrap_or("TBD");
    let score_str = match score {
        Some(s) => format!("{:3}", s),
        None => "   ".to_string(),
    };
    // name_w = width - (seed=2 + sp=1 + sp=1 + score=3 + sp=1) = width - 8
    // But we also want a trailing space → total = 2+1+name_w+1+3+1 = name_w+8
    let name_w = width.saturating_sub(8);
    let name_trunc: String = name.chars().take(name_w).collect();
    let padded_name = format!("{:<width$}", name_trunc, width = name_w);
    format!("{} {} {} ", seed, padded_name, score_str)
}

/// Format the center score/status row.
fn format_status_line(game: &Game, width: usize) -> String {
    let raw = match &game.status {
        GameStatus::Scheduled => game
            .start_time
            .map(|t| format!(" {}", t.format("%I:%M %p")))
            .unwrap_or_else(|| " Scheduled".to_string()),
        GameStatus::InProgress => {
            let period = game
                .period
                .map(|p| format!("{}H", p))
                .unwrap_or_default();
            let clock = game.clock.as_deref().unwrap_or("");
            format!(" {} {}", period, clock)
        }
        GameStatus::Final => " FINAL".to_string(),
        GameStatus::Postponed => " PPD".to_string(),
    };
    let padded = format!("{:<width$}", raw, width = width);
    if padded.chars().count() > width {
        padded.chars().take(width).collect()
    } else {
        padded
    }
}

/// Draw box-drawing connectors between one parent and its two children.
///
/// ```text
///  child_top  ──┐         (col_a='─'  col_b='┐')
///               │         (col_b='│')
///  parent     ──├──       (col_a='─'  col_b='├'  col_c='─')
///               │         (col_b='│')
///  child_bot  ──┘         (col_a='─'  col_b='┘')
/// ```
fn draw_connector(
    r_top: u16,
    r_mid: u16,
    r_bot: u16,
    conn_base_x: u16, // absolute screen x of connector column 0
    area: Rect,
    scroll: u16,
    theme: BannerTheme,
    mirrored: bool,
    buf: &mut Buffer,
) {
    let style = resolve(BannerColor::Dim, theme);
    let col_a = conn_base_x;
    let col_b = conn_base_x + 1;
    let col_c = conn_base_x + 2;
    let limit_x = area.x + area.width;

    macro_rules! put {
        ($x:expr, $row:expr, $ch:expr) => {
            if $x < limit_x {
                if let Some(sy) = screen_y($row, scroll, area) {
                    put_char(buf, $x, sy, $ch, style);
                }
            }
        };
    }

    if mirrored {
        // Mirrored: children are on the right, parent is on the left.
        //   col_b='┌' col_c='─'   child_top
        //   col_b='│'
        //   col_a='─' col_b='┤'   parent
        //   col_b='│'
        //   col_b='└' col_c='─'   child_bot
        put!(col_b, r_top, '┌');
        put!(col_c, r_top, '─');
        for row in (r_top + 1)..r_mid {
            put!(col_b, row, '│');
        }
        put!(col_a, r_mid, '─');
        put!(col_b, r_mid, '┤');
        for row in (r_mid + 1)..r_bot {
            put!(col_b, row, '│');
        }
        put!(col_b, r_bot, '└');
        put!(col_c, r_bot, '─');
    } else {
        // Normal: children are on the left, parent is on the right.
        //   col_a='─' col_b='┐'   child_top
        //              col_b='│'
        //   col_a='─' col_b='├' col_c='─'   parent
        //              col_b='│'
        //   col_a='─' col_b='┘'   child_bot
        put!(col_a, r_top, '─');
        put!(col_b, r_top, '┐');
        for row in (r_top + 1)..r_mid {
            put!(col_b, row, '│');
        }
        put!(col_a, r_mid, '─');
        put!(col_b, r_mid, '├');
        put!(col_c, r_mid, '─');
        for row in (r_mid + 1)..r_bot {
            put!(col_b, row, '│');
        }
        put!(col_a, r_bot, '─');
        put!(col_b, r_bot, '┘');
    }
}

fn put_char(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_height_is_31() {
        assert_eq!(REGION_HEIGHT, 31);
    }

    #[test]
    fn test_slot_heights() {
        assert_eq!(SH, [3, 7, 15, 31]);
    }

    #[test]
    fn test_bracket_grid_cell_count() {
        let grid = BracketGrid::compute(80);
        assert_eq!(grid.cells.len(), 15); // 8 + 4 + 2 + 1
    }

    #[test]
    fn test_first_round_centers() {
        let grid = BracketGrid::compute(80);
        let first = grid.cells_for_depth(0);
        assert_eq!(first.len(), 8);
        let centers: Vec<u16> = first.iter().map(|c| c.center_row).collect();
        assert_eq!(centers, vec![1, 5, 9, 13, 17, 21, 25, 29]);
    }

    #[test]
    fn test_second_round_centers() {
        let grid = BracketGrid::compute(80);
        let second = grid.cells_for_depth(1);
        assert_eq!(second.len(), 4);
        let centers: Vec<u16> = second.iter().map(|c| c.center_row).collect();
        assert_eq!(centers, vec![3, 11, 19, 27]);
    }

    #[test]
    fn test_sweet16_centers() {
        let grid = BracketGrid::compute(80);
        let s16 = grid.cells_for_depth(2);
        assert_eq!(s16.len(), 2);
        let centers: Vec<u16> = s16.iter().map(|c| c.center_row).collect();
        assert_eq!(centers, vec![7, 23]);
    }

    #[test]
    fn test_elite8_center() {
        let grid = BracketGrid::compute(80);
        let e8 = grid.cells_for_depth(3);
        assert_eq!(e8.len(), 1);
        assert_eq!(e8[0].center_row, 15);
    }

    #[test]
    fn test_elite8_no_outbound_connector() {
        let grid = BracketGrid::compute(80);
        let e8 = grid.cells_for_depth(3);
        assert!(!e8[0].draw_outbound_connector);
    }

    #[test]
    fn test_first_round_has_outbound_connector() {
        let grid = BracketGrid::compute(80);
        let first = grid.cells_for_depth(0);
        assert!(first.iter().all(|c| c.draw_outbound_connector));
    }

    #[test]
    fn test_parent_center_is_midpoint_of_children() {
        // Each parent's center should be the arithmetic midpoint of its two children.
        let grid = BracketGrid::compute(80);
        for depth in 0..3usize {
            let children = grid.cells_for_depth(depth);
            let parents = grid.cells_for_depth(depth + 1);
            for (j, parent) in parents.iter().enumerate() {
                let c_top = children[2 * j].center_row;
                let c_bot = children[2 * j + 1].center_row;
                let expected_mid = (c_top + c_bot) / 2;
                assert_eq!(
                    parent.center_row, expected_mid,
                    "depth={depth} parent={j}: expected midpoint of {c_top},{c_bot}={expected_mid}"
                );
            }
        }
    }

    #[test]
    fn test_cell_width_is_computed_from_available_width() {
        let width: u16 = 99;
        let expected = width.saturating_sub(CONNECTOR_WIDTH * 3) / 4;
        let grid = BracketGrid::compute(width);
        assert_eq!(grid.cell_width, expected.min(CELL_W_FULL));
        for cell in &grid.cells {
            assert_eq!(cell.cell_width, grid.cell_width);
        }
    }

    #[test]
    fn test_cell_width_caps_at_full_width_limit() {
        let grid = BracketGrid::compute(200);
        assert_eq!(grid.cell_width, CELL_W_FULL);
    }

    #[test]
    fn test_format_team_line_width() {
        use ncaa_api::{Team, TeamSeed};
        let ts = TeamSeed {
            seed: 1,
            team: Some(Team {
                id: "1".into(),
                name: "Duke Blue Devils".into(),
                short_name: "Duke".into(),
                abbrev: "DUKE".into(),
                color: None,
            }),
            placeholder: None,
        };
        let line = format_team_line(&ts, Some(87), 14);
        assert_eq!(line.chars().count(), 14, "line: {:?}", line);
    }

    #[test]
    fn test_format_team_line_width_full() {
        use ncaa_api::{Team, TeamSeed};
        let ts = TeamSeed {
            seed: 16,
            team: Some(Team {
                id: "2".into(),
                name: "Jacksonville State Gamecocks".into(),
                short_name: "Jksnvlle St".into(),
                abbrev: "JKST".into(),
                color: None,
            }),
            placeholder: None,
        };
        let line = format_team_line(&ts, Some(72), 22);
        assert_eq!(line.chars().count(), 22, "line: {:?}", line);
    }
}
