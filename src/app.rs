use crate::state::app_settings::AppSettings;
use crate::state::app_state::{AppState, BracketPicks, ChatMessage, CompareRow};
use crate::state::chat::ChatWireMessage;
use chrono::Local;
use ncaa_api::{Game, GameDetail, GameStatus, RoundKind, Tournament};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum MenuItem {
    #[default]
    Bracket,
    Scoreboard,
    GameDetail,
    Chat,
    PickWizard,
    Compare,
    Help,
}

pub struct App {
    pub settings: AppSettings,
    pub state: AppState,
}

impl App {
    pub fn new() -> Self {
        let settings = AppSettings::load();

        let app = Self {
            state: AppState::new(),
            settings,
        };

        if let Some(level) = app.settings.log_level {
            log::set_max_level(level);
            tui_logger::set_default_level(level);
        }

        app
    }

    // -----------------------------------------------------------------------
    // Network response handlers — called from main_ui_loop
    // -----------------------------------------------------------------------

    pub fn on_bracket_loaded(&mut self, tournament: Tournament) {
        self.state.last_error = None;
        self.state.bracket.load(tournament);
        self.state.live_feed = Default::default();
    }

    pub fn on_scores_updated(&mut self, games: Vec<Game>) {
        self.state.bracket.merge_updates(games);
    }

    pub fn on_game_detail_loaded(&mut self, detail: GameDetail) {
        self.state.last_error = None;
        let previous_game_id = self.state.game_detail.detail.as_ref().map(|d| d.game_id.clone());
        let game_changed = previous_game_id.as_deref() != Some(detail.game_id.as_str());

        self.state.live_feed.update_from_detail(&detail);
        self.state.game_detail.detail = Some(detail);
        if game_changed {
            self.state.game_detail.scroll_offset = 0;
        }
    }

    // -----------------------------------------------------------------------
    // Tab management
    // -----------------------------------------------------------------------

    pub fn update_tab(&mut self, next: MenuItem) {
        if self.state.active_tab == next {
            return;
        }
        self.state.previous_tab = self.state.active_tab;
        self.state.active_tab = next;
        if self.state.active_tab == MenuItem::Chat {
            self.state.chat.scroll_offset = 0;
        }
        if self.state.active_tab == MenuItem::PickWizard {
            self.start_pick_wizard();
        }
        if self.state.active_tab == MenuItem::Compare {
            self.load_compare_sources();
        }
    }

    pub fn exit_help(&mut self) {
        if self.state.active_tab == MenuItem::Help {
            self.state.active_tab = self.state.previous_tab;
        }
    }

    pub fn toggle_show_logs(&mut self) {
        self.state.show_logs = !self.state.show_logs;
    }

    pub fn toggle_full_screen(&mut self) {
        self.settings.full_screen = !self.settings.full_screen;
    }

    pub fn dismiss_intro(&mut self) {
        self.state.show_intro = false;
    }

    // -----------------------------------------------------------------------
    // Bracket navigation — delegated to BracketState
    // -----------------------------------------------------------------------

    pub fn bracket_next_round(&mut self) {
        self.state.bracket.navigate_round_next();
    }

    pub fn bracket_prev_round(&mut self) {
        self.state.bracket.navigate_round_prev();
    }

    pub fn bracket_game_down(&mut self) {
        self.state.bracket.navigate_game_down();
    }

    pub fn bracket_game_up(&mut self) {
        self.state.bracket.navigate_game_up();
    }

    pub fn bracket_cycle_region(&mut self) {
        self.state.bracket.cycle_region();
    }

    /// Returns (bracket_id, espn_id) if the user pressed Enter on a game.
    /// Switches to the GameDetail tab as a side-effect.
    pub fn bracket_select_game(&mut self) -> Option<(String, Option<String>)> {
        let ids = self.state.bracket.selected_game_id()?;
        self.update_tab(MenuItem::GameDetail);
        Some(ids)
    }

    pub fn selected_game_id(&self) -> Option<(String, Option<String>)> {
        self.state.bracket.selected_game_id()
    }

    // -----------------------------------------------------------------------
    // Animation tick — called every 80ms from AnimationTick event
    // -----------------------------------------------------------------------

    pub fn advance_animation(&mut self, frame_count: usize) {
        self.state.animation.advance(frame_count);
    }

    pub fn on_error(&mut self, message: String) {
        self.state.last_error = Some(message);
    }

    pub fn on_chat_connected(&mut self) {
        self.state.chat.connected = true;
        self.state
            .chat
            .push_system(format!("connected to {}", self.state.chat.endpoint));
    }

    pub fn on_chat_disconnected(&mut self) {
        if self.state.chat.connected {
            self.state.chat.push_system("chat disconnected, retrying...");
        }
        self.state.chat.connected = false;
    }

    pub fn on_chat_error(&mut self, message: String) {
        self.state.chat.push_system(format!("chat error: {message}"));
    }

    pub fn on_chat_message(&mut self, msg: ChatWireMessage) {
        self.state.chat.ingest_message(ChatMessage {
            id: msg.id,
            author: msg.author,
            body: msg.body,
            timestamp: msg.timestamp,
            is_system: false,
        });
    }

    pub fn start_pick_wizard(&mut self) {
        let Some(tournament) = self.state.bracket.tournament.clone() else {
            self.state
                .chat
                .push_system("Load bracket first before using Pick Wizard.");
            self.state.last_error = Some("Pick Wizard needs bracket data".to_string());
            return;
        };
        self.state
            .pick_wizard
            .load_from_tournament_2025_template(&tournament);
        if let Ok(saved) = self.load_pick_wizard_file() {
            self.state.pick_wizard.apply_saved_selections(saved.selections);
        }
    }

    pub fn pick_wizard_select_top(&mut self) {
        self.state.pick_wizard.select_top();
        let _ = self.save_pick_wizard_file();
    }

    pub fn pick_wizard_select_bottom(&mut self) {
        self.state.pick_wizard.select_bottom();
        let _ = self.save_pick_wizard_file();
    }

    pub fn pick_wizard_back(&mut self) {
        self.state.pick_wizard.back();
    }

    pub fn save_pick_wizard_file(&mut self) -> Result<(), String> {
        let picks = self.state.pick_wizard.to_export(self.state.chat.username.clone());
        let path = pick_wizard_path(picks.year);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir failed: {e}"))?;
        }
        let payload = serde_json::to_string_pretty(&picks)
            .map_err(|e| format!("serialize picks failed: {e}"))?;
        std::fs::write(&path, payload).map_err(|e| format!("write picks failed: {e}"))?;
        Ok(())
    }

    pub fn load_pick_wizard_file(&self) -> Result<BracketPicks, String> {
        let path = pick_wizard_path(self.state.pick_wizard.year);
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("read picks failed: {e}"))?;
        serde_json::from_str::<BracketPicks>(&content)
            .map_err(|e| format!("parse picks failed: {e}"))
    }

    pub fn load_compare_sources(&mut self) {
        let Some(tournament) = self.state.bracket.tournament.as_ref() else {
            self.state.last_error = Some("Compare needs bracket data".to_string());
            return;
        };

        let mut loaded: Vec<(String, BracketPicks)> = Vec::new();
        let mut source_errors = Vec::new();

        for source in self.compare_sources() {
            match load_picks_source(&source) {
                Ok(picks) => loaded.push((source, picks)),
                Err(e) => source_errors.push(e),
            }
        }

        let eliminated = build_eliminated_set(tournament);
        let mut rows = Vec::new();
        for (source, picks) in loaded {
            rows.push(score_picks(tournament, &source, &picks, &eliminated));
        }
        rows.sort_by(|a, b| {
            b.points
                .cmp(&a.points)
                .then_with(|| b.correct.cmp(&a.correct))
                .then_with(|| a.user_id.cmp(&b.user_id))
        });

        self.state.compare.rows = rows;
        self.state.compare.source_errors = source_errors;
        self.state.compare.last_loaded_at = Some(Local::now().format("%H:%M").to_string());
        self.state.compare.scroll_offset = 0;
    }

    pub fn compare_scroll_down(&mut self) {
        let max = self.state.compare.rows.len().saturating_sub(1) as u16;
        self.state.compare.scroll_offset = (self.state.compare.scroll_offset + 1).min(max);
    }

    pub fn compare_scroll_up(&mut self) {
        self.state.compare.scroll_offset = self.state.compare.scroll_offset.saturating_sub(1);
    }

    fn compare_sources(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(pick_wizard_path(2025).display().to_string());

        if let Some(compare_dir) = pick_wizard_path(2025).parent().map(|p| p.join("compare"))
            && let Ok(entries) = std::fs::read_dir(compare_dir)
        {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("json") {
                    out.push(p.display().to_string());
                }
            }
        }

        if let Ok(extra) = std::env::var("MMTUI_COMPARE_SOURCES") {
            out.extend(
                extra
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string),
            );
        }

        out.sort();
        out.dedup();
        out
    }
}

fn pick_wizard_path(year: u16) -> PathBuf {
    if let Ok(config_dir) = std::env::var("XDG_CONFIG_HOME")
        && !config_dir.trim().is_empty()
    {
        return PathBuf::from(config_dir).join("mmtui").join(format!("picks_{year}.json"));
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".config")
            .join("mmtui")
            .join(format!("picks_{year}.json"));
    }
    PathBuf::from(format!("picks_{year}.json"))
}

fn round_weight(round: RoundKind) -> u32 {
    match round {
        RoundKind::FirstFour => 1,
        RoundKind::First => 1,
        RoundKind::Second => 2,
        RoundKind::Sweet16 => 4,
        RoundKind::Elite8 => 8,
        RoundKind::FinalFour => 16,
        RoundKind::Championship => 32,
    }
}

/// Build the set of eliminated team IDs from all Final games in the tournament.
/// A team is eliminated if it participated in a Final game but was not the winner.
fn build_eliminated_set(tournament: &Tournament) -> HashSet<String> {
    let mut eliminated = HashSet::new();
    for region in &tournament.regions {
        for round in &region.rounds {
            for game in &round.games {
                if game.status != GameStatus::Final {
                    continue;
                }
                let Some(winner_id) = game.winner_id.as_deref() else {
                    continue;
                };
                for slot in [&game.top, &game.bottom] {
                    if let Some(team) = &slot.team {
                        if team.id != winner_id {
                            eliminated.insert(team.id.clone());
                        }
                    }
                }
            }
        }
    }
    eliminated
}

/// Returns true if the pick is still viable (picked team has not been eliminated).
/// Handles both direct team ID picks and placeholder picks ("top:{game_id}", "bottom:{game_id}").
/// Treats TBD slots (team = None) as alive — a team that hasn't entered the bracket can't be out.
fn is_viable(selection: &str, game: &Game, eliminated: &HashSet<String>) -> bool {
    let team_id: Option<&str> = if selection == format!("top:{}", game.id) {
        game.top.team.as_ref().map(|t| t.id.as_str())
    } else if selection == format!("bottom:{}", game.id) {
        game.bottom.team.as_ref().map(|t| t.id.as_str())
    } else {
        Some(selection)
    };
    match team_id {
        Some(id) => !eliminated.contains(id),
        None => true, // TBD slot — not yet in the bracket, cannot be eliminated
    }
}

fn load_picks_source(source: &str) -> Result<BracketPicks, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let body = reqwest::blocking::get(source)
            .map_err(|e| format!("{source}: fetch failed: {e}"))?
            .text()
            .map_err(|e| format!("{source}: read body failed: {e}"))?;
        serde_json::from_str(&body).map_err(|e| format!("{source}: invalid picks json: {e}"))
    } else {
        let content = std::fs::read_to_string(source)
            .map_err(|e| format!("{source}: read failed: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("{source}: invalid picks json: {e}"))
    }
}

fn score_picks(tournament: &Tournament, source: &str, picks: &BracketPicks, eliminated: &HashSet<String>) -> CompareRow {
    let mut points = 0u32;
    let mut max_points = 0u32;
    let mut correct = 0u32;
    let mut total = 0u32;

    for region in &tournament.regions {
        for round in &region.rounds {
            for game in &round.games {
                let Some(selection) = picks.selections.get(&game.id) else {
                    continue;
                };
                total += 1;
                let weight = round_weight(round.kind);
                let winner_side = if game.winner_id.as_ref() == game.top.team.as_ref().map(|t| &t.id) {
                    Some("top")
                } else if game.winner_id.as_ref() == game.bottom.team.as_ref().map(|t| &t.id) {
                    Some("bottom")
                } else {
                    None
                };

                let picked_correct = match game.status {
                    GameStatus::Final => {
                        let is_team_match = game.winner_id.as_deref() == Some(selection.as_str());
                        let is_side_match = selection == &format!("top:{}", game.id)
                            && winner_side == Some("top")
                            || selection == &format!("bottom:{}", game.id)
                                && winner_side == Some("bottom");
                        is_team_match || is_side_match
                    }
                    _ => false,
                };

                if picked_correct {
                    correct += 1;
                    points += weight;
                }

                match game.status {
                    GameStatus::Final => {
                        if picked_correct {
                            max_points += weight;
                        }
                    }
                    _ => {
                        if is_viable(selection, game, eliminated) {
                            max_points += weight;
                        }
                    }
                }
            }
        }
    }

    CompareRow {
        user_id: picks.user_id.clone(),
        source: source.to_string(),
        points,
        max_points,
        correct,
        total,
    }
}
