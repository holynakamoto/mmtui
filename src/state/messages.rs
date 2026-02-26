use crate::state::network::LoadingState;
use crossterm::event::KeyEvent;
use ncaa_api::{Game, GameDetail, Tournament};

#[derive(Debug, Clone)]
pub enum NetworkRequest {
    LoadBracket,
    RefreshScores,
    RefreshPrizePoolBalance {
        address: String,
    },
    LoadGameDetail {
        bracket_id: String,
        /// ESPN event ID used to call fetch_game_detail. None pre-Selection Sunday
        /// for NCAA-sourced games; game detail is skipped gracefully when absent.
        espn_id: Option<String>,
    },
}

#[derive(Debug)]
pub enum NetworkResponse {
    LoadingStateChanged { loading_state: LoadingState },
    BracketLoaded { tournament: Tournament },
    /// Partial update: only changed Game objects, merged into the bracket tree.
    BracketUpdated { games: Vec<Game> },
    GameDetailLoaded { detail: GameDetail },
    PrizePoolBalanceUpdated { balance_sat: u64 },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    KeyPressed(KeyEvent),
    Resize,
    AppStarted,
    AnimationTick,
}
