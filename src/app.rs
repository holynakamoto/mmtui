use crate::state::app_settings::AppSettings;
use crate::state::app_state::{AppState, BracketPicks, ChatMessage, CompareRow};
use crate::state::chat::ChatWireMessage;
use crate::state::custodian::{
    CustodianConfig, CustodianEntry, CustodianWizardState,
    bip67_sort, compute_threshold, custodian_config_path,
};
use bitcoin::address::Address;
use bitcoin::key::PublicKey;
use bitcoin::script::Builder;
use bitcoin::opcodes;
use bitcoin::Network;
use chrono::Local;
use ncaa_api::{Game, GameDetail, GameStatus, RoundKind, Tournament};
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum MenuItem {
    #[default]
    Bracket,
    Scoreboard,
    GameDetail,
    Chat,
    PickWizard,
    Compare,
    PrizePool,
    Help,
}

pub struct App {
    pub settings: AppSettings,
    pub state: AppState,
}

impl App {
    pub fn new() -> Self {
        let settings = AppSettings::load();

        let mut app = Self {
            state: AppState::new(),
            settings,
        };

        if let Some(level) = app.settings.log_level {
            log::set_max_level(level);
            tui_logger::set_default_level(level);
        }

        app.setup_prize_pool();
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

    pub fn on_prize_pool_balance_updated(&mut self, balance_sat: u64) {
        self.state.prize_pool.balance_sat = balance_sat;
        self.state.prize_pool.loading = false;
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

    pub fn setup_prize_pool(&mut self) {
        let entries = self.load_custodian_entries();
        self.apply_custodian_entries(entries);
    }

    /// Load custodian entries: file → env var → fake placeholders.
    fn load_custodian_entries(&self) -> Vec<CustodianEntry> {
        // 1. Try custodians.json
        let path = custodian_config_path();
        if let Ok(config) = CustodianConfig::load_from_path(&path) {
            if config.custodians.len() >= 2 {
                return config.custodians;
            }
        }

        // 2. Try env var
        if let Ok(keys_raw) = std::env::var("MMTUI_PRIZE_POOL_KEYS") {
            let entries: Vec<CustodianEntry> = keys_raw
                .split(',')
                .enumerate()
                .filter_map(|(i, s)| {
                    CustodianEntry::new(&format!("Custodian {}", i + 1), s.trim()).ok()
                })
                .collect();
            if entries.len() >= 2 {
                return entries;
            }
        }

        // 3. Fake placeholders — valid secp256k1 generator multiples so address still generates
        vec![
            CustodianEntry { label: "Custodian A (placeholder)".to_string(), pubkey: "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".to_string() },
            CustodianEntry { label: "Custodian B (placeholder)".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
            CustodianEntry { label: "Custodian C (placeholder)".to_string(), pubkey: "02f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9".to_string() },
        ]
    }

    /// Build multisig script and address from entries, update prize_pool state.
    pub fn apply_custodian_entries(&mut self, mut entries: Vec<CustodianEntry>) {
        bip67_sort(&mut entries);

        let keys: Vec<_> = entries
            .iter()
            .filter_map(|e| PublicKey::from_str(&e.pubkey).ok())
            .collect();

        if keys.len() < 2 {
            self.state.last_error = Some("Prize Pool: need at least 2 valid keys".to_string());
            return;
        }

        let threshold = compute_threshold(keys.len());
        let mut builder = Builder::new().push_int(threshold as i64);
        for key in &keys {
            builder = builder.push_key(key);
        }
        builder = builder
            .push_int(keys.len() as i64)
            .push_opcode(opcodes::all::OP_CHECKMULTISIG);

        let script = builder.into_script();
        let address = Address::p2wsh(&script, Network::Bitcoin);

        self.state.prize_pool.address = address.to_string();
        self.state.prize_pool.custodians = entries;
        self.state.prize_pool.threshold = threshold;
    }

    pub fn open_custodian_wizard(&mut self) {
        let existing = self.state.prize_pool.custodians.clone();
        self.state.custodian_wizard = CustodianWizardState::open(existing);
    }

    pub fn finalize_custodian_wizard(&mut self) {
        let entries = self.state.custodian_wizard.entries.clone();
        let config = CustodianConfig { custodians: entries.clone() };
        let path = custodian_config_path();
        if let Err(e) = config.save_to_path(&path) {
            self.state.last_error = Some(format!("Save failed: {e}"));
            return;
        }
        self.apply_custodian_entries(entries);
        self.state.custodian_wizard.discard();
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

    pub fn reset_pick_wizard(&mut self) {
        // Clear in-memory selections and reset wizard progress
        self.state.pick_wizard.selections.clear();
        self.state.pick_wizard.completed = false;
        self.state.pick_wizard.current_index = 0;

        // Attempt to remove saved picks file; ignore error if not present
        let path = pick_wizard_path(self.state.pick_wizard.year);
        match std::fs::remove_file(&path) {
            Ok(_) => self
                .state
                .chat
                .push_system("Picks reset and saved file removed."),
            Err(_) => self.state.chat.push_system("Picks reset (no saved file found)."),
        }
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
