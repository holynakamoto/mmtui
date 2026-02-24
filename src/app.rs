use crate::state::app_settings::AppSettings;
use crate::state::app_state::{AppState, BracketPicks, ChatMessage};
use crate::state::chat::ChatWireMessage;
use ncaa_api::{Game, GameDetail, Tournament};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum MenuItem {
    #[default]
    Bracket,
    Scoreboard,
    GameDetail,
    Chat,
    PickWizard,
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

    /// Returns the game ID to load if the user pressed Enter on a game.
    pub fn bracket_select_game(&mut self) -> Option<String> {
        let game_id = self.state.bracket.selected_game_id()?;
        self.update_tab(MenuItem::GameDetail);
        Some(game_id)
    }

    pub fn selected_game_id(&self) -> Option<String> {
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
