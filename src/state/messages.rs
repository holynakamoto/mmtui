use crate::state::network::LoadingState;
use crossterm::event::KeyEvent;
use ncaa_api::{Game, GameDetail, Tournament};

#[derive(Debug, Clone)]
pub enum NetworkRequest {
    LoadBracket,
    RefreshScores,
    LoadGameDetail { game_id: String },
}

#[derive(Debug)]
pub enum NetworkResponse {
    LoadingStateChanged { loading_state: LoadingState },
    BracketLoaded { tournament: Tournament },
    /// Partial update: only changed Game objects, merged into the bracket tree.
    BracketUpdated { games: Vec<Game> },
    GameDetailLoaded { detail: GameDetail },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    KeyPressed(KeyEvent),
    Resize,
    AppStarted,
    AnimationTick,
}
