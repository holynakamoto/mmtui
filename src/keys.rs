use crate::app::{App, MenuItem};
use crate::state::chat::ChatCommand;
use crate::state::messages::NetworkRequest;
use crossterm::event::KeyCode::Char;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub async fn handle_key_bindings(
    key_event: KeyEvent,
    app: &Arc<Mutex<App>>,
    network_requests: &mpsc::Sender<NetworkRequest>,
    chat_commands: &mpsc::Sender<ChatCommand>,
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

    // Custodian wizard intercepts all keys when active
    if guard.state.custodian_wizard.active {
        use crate::state::custodian::WizardStep;
        let wiz = &mut guard.state.custodian_wizard;

        match wiz.step {
            WizardStep::Review => match key_event.code {
                Char('a') => wiz.begin_add(),
                Char('d') => wiz.delete_selected(),
                KeyCode::Down | Char('j') => wiz.cursor_down(),
                KeyCode::Up | Char('k') => wiz.cursor_up(),
                KeyCode::Enter => {
                    let can = guard.state.custodian_wizard.can_finalize();
                    if can {
                        guard.finalize_custodian_wizard();
                    }
                }
                KeyCode::Esc => {
                    if wiz.dirty {
                        wiz.step = WizardStep::ConfirmDiscard;
                    } else {
                        wiz.discard();
                    }
                }
                _ => {}
            },

            WizardStep::EnterLabel => match key_event.code {
                KeyCode::Enter => {
                    let trimmed = wiz.input.trim().to_string();
                    if !trimmed.is_empty() {
                        wiz.advance_to_pubkey();
                    }
                }
                KeyCode::Esc => {
                    wiz.input.clear();
                    wiz.error = None;
                    wiz.step = WizardStep::Review;
                }
                KeyCode::Backspace => {
                    wiz.input.pop();
                }
                Char(ch)
                    if key_event.modifiers == KeyModifiers::NONE
                        || key_event.modifiers == KeyModifiers::SHIFT =>
                {
                    wiz.input.push(ch);
                }
                _ => {}
            },

            WizardStep::EnterPubkey => match key_event.code {
                KeyCode::Enter => {
                    let _ = wiz.commit_pubkey();
                }
                KeyCode::Esc => {
                    wiz.input.clear();
                    wiz.label_buf.clear();
                    wiz.error = None;
                    wiz.step = WizardStep::Review;
                }
                KeyCode::Backspace => {
                    wiz.input.pop();
                    wiz.error = None;
                }
                Char(ch)
                    if (key_event.modifiers == KeyModifiers::NONE
                        || key_event.modifiers == KeyModifiers::SHIFT)
                        && ch.is_ascii_hexdigit() =>
                {
                    wiz.input.push(ch);
                    wiz.error = None;
                }
                _ => {}
            },

            WizardStep::ConfirmDiscard => match key_event.code {
                KeyCode::Esc => wiz.discard(),
                // Any key other than Esc dismisses the confirm dialog and returns to Review
                _ => {
                    wiz.step = WizardStep::Review;
                }
            },
        }
        return;
    }

    if guard.state.active_tab == MenuItem::Chat && guard.state.chat.composing {
        match (key_event.code, key_event.modifiers) {
            (Char('c'), KeyModifiers::CONTROL) => {
                crate::cleanup_terminal();
                std::process::exit(0);
            }
            (KeyCode::Esc, _) => {
                guard.state.chat.composing = false;
                guard.state.chat.input.clear();
            }
            (KeyCode::Enter, _) => {
                let outbound = guard.state.chat.submit_input();
                drop(guard);
                if let Some(outbound) = outbound {
                    let _ = chat_commands
                        .send(ChatCommand::Send {
                            body: outbound.body,
                            message_id: outbound.id,
                        })
                        .await;
                }
            }
            (KeyCode::Backspace, _) => {
                guard.state.chat.input.pop();
            }
            (Char(ch), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                guard.state.chat.input.push(ch);
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
        (tab, Char('1'), _) if tab != MenuItem::PickWizard => guard.update_tab(MenuItem::Bracket),
        (tab, Char('2'), _) if tab != MenuItem::PickWizard => {
            guard.update_tab(MenuItem::Scoreboard)
        }
        (_, Char('3'), _) => guard.update_tab(MenuItem::GameDetail),
        (_, Char('4'), _) => guard.update_tab(MenuItem::Chat),
        (_, Char('5'), _) => guard.update_tab(MenuItem::PickWizard),
        (_, Char('6'), _) => guard.update_tab(MenuItem::Compare),
        (_, Char('7'), _) => {
            guard.update_tab(MenuItem::PrizePool);
            let address = guard.state.prize_pool.address.clone();
            guard.state.prize_pool.loading = true;
            drop(guard);
            let _ = network_requests
                .send(NetworkRequest::RefreshPrizePoolBalance { address })
                .await;
            return;
        }
        (_, Char('?'), _) => guard.update_tab(MenuItem::Help),
        (MenuItem::Help, KeyCode::Esc, _) => guard.exit_help(),

        // Prize Pool
        (MenuItem::PrizePool, Char('r'), _) => {
            let address = guard.state.prize_pool.address.clone();
            guard.state.prize_pool.loading = true;
            drop(guard);
            let _ = network_requests
                .send(NetworkRequest::RefreshPrizePoolBalance { address })
                .await;
            return;
        }
        (MenuItem::PrizePool, Char('e'), _) => guard.open_custodian_wizard(),
        (MenuItem::PrizePool, KeyCode::Esc, _) => guard.update_tab(MenuItem::Bracket),

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
            if let Some((bracket_id, espn_id)) = guard.bracket_select_game() {
                drop(guard);
                let _ = network_requests
                    .send(NetworkRequest::LoadGameDetail { bracket_id, espn_id })
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

        // Chat controls
        (MenuItem::Chat, Char('i'), _) | (MenuItem::Chat, KeyCode::Enter, _) => {
            guard.state.chat.composing = true;
        }
        (MenuItem::Chat, KeyCode::Esc, _) => {
            guard.update_tab(MenuItem::Bracket);
        }
        (MenuItem::Chat, Char('j') | KeyCode::Down, _) => {
            let max_offset = guard.state.chat.messages.len().saturating_sub(1) as u16;
            guard.state.chat.scroll_offset = (guard.state.chat.scroll_offset + 1).min(max_offset);
        }
        (MenuItem::Chat, Char('k') | KeyCode::Up, _) => {
            guard.state.chat.scroll_offset = guard.state.chat.scroll_offset.saturating_sub(1);
        }

        // Pick Wizard
        (MenuItem::PickWizard, Char('1'), _) => guard.pick_wizard_select_top(),
        (MenuItem::PickWizard, Char('2'), _) => guard.pick_wizard_select_bottom(),
        (MenuItem::PickWizard, Char('j') | KeyCode::Down, _) => guard.state.pick_wizard.advance(),
        (MenuItem::PickWizard, Char('k') | KeyCode::Up, _) => guard.pick_wizard_back(),
        (MenuItem::PickWizard, Char('s'), _) => {
            if let Err(e) = guard.save_pick_wizard_file() {
                guard.on_error(e);
            }
        }
        (MenuItem::PickWizard, Char('r'), _) => guard.reset_pick_wizard(),
        (MenuItem::PickWizard, KeyCode::Esc, _) => guard.update_tab(MenuItem::Bracket),

        // Compare
        (MenuItem::Compare, Char('r'), _) => guard.load_compare_sources(),
        (MenuItem::Compare, Char('j') | KeyCode::Down, _) => guard.compare_scroll_down(),
        (MenuItem::Compare, Char('k') | KeyCode::Up, _) => guard.compare_scroll_up(),
        (MenuItem::Compare, KeyCode::Esc, _) => guard.update_tab(MenuItem::Bracket),

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
            if let Some((bracket_id, espn_id)) = guard.bracket_select_game() {
                drop(guard);
                let _ = network_requests
                    .send(NetworkRequest::LoadGameDetail { bracket_id, espn_id })
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
        && let Some((bracket_id, espn_id)) = guard.selected_game_id()
    {
        drop(guard);
        let _ = network_requests
            .send(NetworkRequest::LoadGameDetail { bracket_id, espn_id })
            .await;
    }
}
