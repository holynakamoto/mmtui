mod app;
mod components;
mod draw;
mod keys;
mod state;
mod ui;

use crate::app::App;
use crate::state::messages::{NetworkRequest, NetworkResponse, UiEvent};
use crate::state::network::{LoadingState, NetworkWorker};
use crate::state::refresher::PeriodicRefresher;
use crossterm::event::{self as crossterm_event, Event};
use crossterm::{cursor, execute, terminal};
use log::error;
use std::io::Stdout;
use std::sync::Arc;
use std::{io, panic};
use tokio::sync::{Mutex, mpsc};
use tokio::time::Duration;
use tui::{Terminal, backend::CrosstermBackend};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if handle_cli_args() {
        return Ok(());
    }

    better_panic::install();

    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;

    setup_panic_hook();
    setup_terminal();

    tui_logger::init_logger(log::LevelFilter::Error)?;
    tui_logger::set_default_level(log::LevelFilter::Error);

    let app = Arc::new(Mutex::new(App::new()));

    let (ui_event_tx, ui_event_rx) = mpsc::channel::<UiEvent>(100);
    let (network_req_tx, network_req_rx) = mpsc::channel::<NetworkRequest>(100);
    let (network_resp_tx, network_resp_rx) = mpsc::channel::<NetworkResponse>(100);

    // Input handler thread
    let input_handler = tokio::spawn(input_handler_task(ui_event_tx.clone()));

    // Network thread
    let network_worker = NetworkWorker::new(network_req_rx, network_resp_tx);
    let network_task = tokio::spawn(network_worker.run());

    // Periodic score refresh thread (every 30s)
    let periodic_updater = PeriodicRefresher::new(network_req_tx.clone());
    let periodic_task = tokio::spawn(periodic_updater.run());

    // Animation tick thread — 80ms ≈ 12.5 FPS
    let anim_tx = ui_event_tx.clone();
    let animation_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(80));
        loop {
            interval.tick().await;
            if anim_tx.send(UiEvent::AnimationTick).await.is_err() {
                break;
            }
        }
    });

    // Trigger bracket load on startup
    let _ = ui_event_tx.send(UiEvent::AppStarted).await;

    main_ui_loop(terminal, app, ui_event_rx, network_req_tx, network_resp_rx).await;

    input_handler.abort();
    network_task.abort();
    periodic_task.abort();
    animation_task.abort();

    Ok(())
}

fn handle_cli_args() -> bool {
    let mut args = std::env::args().skip(1);
    let Some(arg) = args.next() else {
        return false;
    };

    match arg.as_str() {
        "-h" | "--help" => {
            println!("{}", usage_text());
            true
        }
        "-V" | "--version" => {
            println!("mmtui {}", env!("CARGO_PKG_VERSION"));
            true
        }
        _ => {
            eprintln!("Unknown argument: {arg}\n\n{}", usage_text());
            std::process::exit(2);
        }
    }
}

fn usage_text() -> &'static str {
    "mmtui - NCAA March Madness terminal UI

Usage:
  mmtui
  mmtui --help
  mmtui --version

Environment:
  MMTUI_BRACKET_JSON   Path to local tournament JSON snapshot"
}

async fn main_ui_loop(
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
    app: Arc<Mutex<App>>,
    mut ui_events: mpsc::Receiver<UiEvent>,
    network_requests: mpsc::Sender<NetworkRequest>,
    mut network_responses: mpsc::Receiver<NetworkResponse>,
) {
    let mut loading = LoadingState::default();

    loop {
        tokio::select! {
            Some(ui_event) = ui_events.recv() => {
                let should_redraw = handle_ui_event(ui_event, &app, &network_requests).await;
                if should_redraw && !loading.is_loading {
                    let mut app_guard = app.lock().await;
                    draw::draw(&mut terminal, &mut app_guard, loading);
                }
            }

            Some(response) = network_responses.recv() => {
                let should_redraw =
                    handle_network_response(response, &app, &mut loading).await;
                if should_redraw {
                    let mut app_guard = app.lock().await;
                    draw::draw(&mut terminal, &mut app_guard, loading);
                }
            }
        }
    }
}

async fn handle_ui_event(
    ui_event: UiEvent,
    app: &Arc<Mutex<App>>,
    network_requests: &mpsc::Sender<NetworkRequest>,
) -> bool {
    match ui_event {
        UiEvent::AppStarted => {
            let _ = network_requests.send(NetworkRequest::LoadBracket).await;
            true
        }
        UiEvent::KeyPressed(key_event) => {
            keys::handle_key_bindings(key_event, app, network_requests).await;
            true
        }
        UiEvent::Resize => true,
        UiEvent::AnimationTick => {
            let mut guard = app.lock().await;
            guard.advance_animation(crate::components::banner::FRAME_COUNT);
            true
        }
    }
}

async fn handle_network_response(
    response: NetworkResponse,
    app: &Arc<Mutex<App>>,
    loading: &mut LoadingState,
) -> bool {
    match response {
        NetworkResponse::LoadingStateChanged { loading_state } => {
            *loading = loading_state;
            return true;
        }
        NetworkResponse::BracketLoaded { tournament } => {
            let mut guard = app.lock().await;
            guard.on_bracket_loaded(tournament);
        }
        NetworkResponse::BracketUpdated { games } => {
            let mut guard = app.lock().await;
            guard.on_scores_updated(games);
        }
        NetworkResponse::GameDetailLoaded { detail } => {
            let mut guard = app.lock().await;
            guard.on_game_detail_loaded(detail);
        }
        NetworkResponse::Error { message } => {
            error!("Network error: {message}");
            let mut guard = app.lock().await;
            guard.on_error(message);
        }
    }
    !loading.is_loading
}

async fn input_handler_task(ui_events: mpsc::Sender<UiEvent>) {
    loop {
        if let Ok(event) = crossterm_event::read() {
            let ui_event = match event {
                Event::Key(key_event) => Some(UiEvent::KeyPressed(key_event)),
                Event::Resize(_, _) => Some(UiEvent::Resize),
                _ => None,
            };

            if let Some(ui_event) = ui_event
                && ui_events.send(ui_event).await.is_err()
            {
                break;
            }
        }
    }
}

fn setup_terminal() {
    let mut stdout = io::stdout();
    execute!(stdout, cursor::Hide).unwrap();
    execute!(stdout, terminal::EnterAlternateScreen).unwrap();
    execute!(stdout, terminal::Clear(terminal::ClearType::All)).unwrap();
    terminal::enable_raw_mode().unwrap();
}

pub fn cleanup_terminal() {
    let mut stdout = io::stdout();
    execute!(stdout, cursor::MoveTo(0, 0)).unwrap();
    execute!(stdout, terminal::Clear(terminal::ClearType::All)).unwrap();
    execute!(stdout, terminal::LeaveAlternateScreen).unwrap();
    execute!(stdout, cursor::Show).unwrap();
    terminal::disable_raw_mode().unwrap();
}

fn setup_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        cleanup_terminal();
        better_panic::Settings::auto().create_panic_handler()(panic_info);
    }));
}
