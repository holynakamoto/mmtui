use crate::app::{App, MenuItem};
use crate::state::messages::NetworkRequest;
use crossterm::event::KeyCode::Char;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub async fn handle_key_bindings(
    key_event: KeyEvent,
    app: &Arc<Mutex<App>>,
    network_requests: &mpsc::Sender<NetworkRequest>,
) {
    let mut guard = app.lock().await;
    let mut refresh_live_feed = false;

    if guard.state.show_intro {
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Enter, _) => guard.dismiss_intro(),
            (Char('q'), _) | (Char('c'), KeyModifiers::CONTROL) => {
                crate::cleanup_terminal();
                std::process::exit(0);
            }
            _ => {}
        }
        return;
    }

    match (guard.state.active_tab, key_event.code, key_event.modifiers) {
        // Quit
        (_, Char('q'), _) | (_, Char('c'), KeyModifiers::CONTROL) => {
            crate::cleanup_terminal();
            std::process::exit(0);
        }

        // Tab switching
        (_, Char('1'), _) => guard.update_tab(MenuItem::Bracket),
        (_, Char('2'), _) => guard.update_tab(MenuItem::Scoreboard),
        (_, Char('3'), _) => guard.update_tab(MenuItem::GameDetail),
        (_, Char('?'), _) => guard.update_tab(MenuItem::Help),
        (MenuItem::Help, KeyCode::Esc, _) => guard.exit_help(),

        // Bracket navigation
        (MenuItem::Bracket, Char('l') | KeyCode::Right, _) => {
            guard.bracket_next_round();
            refresh_live_feed = true;
        }
        (MenuItem::Bracket, Char('h') | KeyCode::Left, _) => {
            guard.bracket_prev_round();
            refresh_live_feed = true;
        }
        (MenuItem::Bracket, Char('j') | KeyCode::Down, _) => {
            guard.bracket_game_down();
            refresh_live_feed = true;
        }
        (MenuItem::Bracket, Char('k') | KeyCode::Up, _) => {
            guard.bracket_game_up();
            refresh_live_feed = true;
        }
        (MenuItem::Bracket, Char('r'), _) => {
            guard.bracket_cycle_region();
            refresh_live_feed = true;
        }
        (MenuItem::Bracket, KeyCode::Enter, _) => {
            if let Some(game_id) = guard.bracket_select_game() {
                drop(guard);
                let _ = network_requests
                    .send(NetworkRequest::LoadGameDetail { game_id })
                    .await;
                return;
            }
        }

        // Game detail navigation
        (MenuItem::GameDetail, Char('j') | KeyCode::Down, _) => {
            guard.state.game_detail.scroll_offset =
                guard.state.game_detail.scroll_offset.saturating_add(1);
        }
        (MenuItem::GameDetail, Char('k') | KeyCode::Up, _) => {
            guard.state.game_detail.scroll_offset =
                guard.state.game_detail.scroll_offset.saturating_sub(1);
        }
        (MenuItem::GameDetail, KeyCode::Esc, _) => guard.update_tab(MenuItem::Bracket),

        // Scoreboard navigation
        (MenuItem::Scoreboard, Char('j') | KeyCode::Down, _) => {
            guard.bracket_game_down();
            refresh_live_feed = true;
        }
        (MenuItem::Scoreboard, Char('k') | KeyCode::Up, _) => {
            guard.bracket_game_up();
            refresh_live_feed = true;
        }
        (MenuItem::Scoreboard, Char('r'), _) => {
            guard.bracket_cycle_region();
            refresh_live_feed = true;
        }
        (MenuItem::Scoreboard, KeyCode::Enter, _) => {
            if let Some(game_id) = guard.bracket_select_game() {
                drop(guard);
                let _ = network_requests
                    .send(NetworkRequest::LoadGameDetail { game_id })
                    .await;
                return;
            }
        }

        // Global
        (_, Char('f'), _) => guard.toggle_full_screen(),
        (_, Char('"'), _) => guard.toggle_show_logs(),

        _ => {}
    }

    if refresh_live_feed
        && let Some(game_id) = guard.selected_game_id()
    {
        drop(guard);
        let _ = network_requests
            .send(NetworkRequest::LoadGameDetail { game_id })
            .await;
    }
}
