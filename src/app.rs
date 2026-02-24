use crate::state::app_settings::AppSettings;
use crate::state::app_state::AppState;
use ncaa_api::{Game, GameDetail, Tournament};

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum MenuItem {
    #[default]
    Bracket,
    Scoreboard,
    GameDetail,
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
}
