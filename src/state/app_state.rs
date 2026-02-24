use crate::app::MenuItem;
use chrono::Local;
use ncaa_api::{GameDetail, RoundKind, TeamSeed, Tournament};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Banner animation state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AnimationState {
    /// Current frame index into the banner frames array, wraps at FRAME_COUNT.
    pub frame: usize,
    /// Monotonic tick counter — drives color cycling and the triangle-wave offset.
    pub tick: u64,
}

impl AnimationState {
    pub fn advance(&mut self, frame_count: usize) {
        self.tick = self.tick.wrapping_add(1);
        self.frame = (self.frame + 1) % frame_count;
    }}

// ---------------------------------------------------------------------------
// Bracket / tournament state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct BracketState {
    pub tournament: Option<Tournament>,
    /// The "live" round — auto-detected as the last round with in-progress or
    /// recently finished games. Drives the initial view on load.
    pub current_round: RoundKind,
    /// The round the user has navigated to (may differ from current_round).
    pub view_round: RoundKind,
    /// Selected region index (0–3). Ignored when view_round.is_final_four().
    pub selected_region: usize,
    /// Selected game index within the current region + round.
    pub selected_game: usize,
    /// Vertical scroll offset for when games exceed terminal height.
    pub scroll_offset: u16,
}

impl BracketState {
    /// Store a newly loaded tournament and auto-detect the active round.
    pub fn load(&mut self, tournament: Tournament) {
        self.current_round = detect_active_round(&tournament);
        self.view_round = self.current_round;
        self.selected_game = 0;
        self.selected_region = 0;
        self.scroll_offset = 0;
        self.tournament = Some(tournament);
    }

    /// Merge partial game updates from a scoreboard refresh.
    pub fn merge_updates(&mut self, games: Vec<ncaa_api::Game>) {
        if let Some(t) = &mut self.tournament {
            t.merge_updates(games);
            // Re-detect the active round in case a new round started.
            self.current_round = detect_active_round(t);
        }
    }

    pub fn navigate_round_next(&mut self) {
        if let Some(next) = self.view_round.next() {
            self.view_round = next;
            self.selected_game = 0;
            self.scroll_offset = 0;
        }
    }

    pub fn navigate_round_prev(&mut self) {
        if let Some(prev) = self.view_round.prev() {
            self.view_round = prev;
            self.selected_game = 0;
            self.scroll_offset = 0;
        }
    }

    pub fn navigate_game_down(&mut self) {
        let max = self.games_in_view().saturating_sub(1);
        if self.selected_game < max {
            self.selected_game += 1;
        }
    }

    pub fn navigate_game_up(&mut self) {
        self.selected_game = self.selected_game.saturating_sub(1);
    }

    pub fn cycle_region(&mut self) {
        if !self.view_round.is_final_four() {
            let region_count = self
                .tournament
                .as_ref()
                .map(|t| t.regions.len().saturating_sub(1)) // exclude "National"
                .unwrap_or(4);
            self.selected_region = (self.selected_region + 1) % region_count.max(1);
            self.selected_game = 0;
            self.scroll_offset = 0;
        }
    }

    /// Return the game ID of the currently selected game, if any.
    pub fn selected_game_id(&self) -> Option<String> {
        let tournament = self.tournament.as_ref()?;
        let region = if self.view_round.is_final_four() {
            tournament.regions.iter().find(|r| r.name == "National")?
        } else {
            tournament.regions.get(self.selected_region)?
        };
        let round = region
            .rounds
            .iter()
            .find(|r| r.kind == self.view_round)?;
        round.games.get(self.selected_game).map(|g| g.id.clone())
    }

    fn games_in_view(&self) -> usize {
        let Some(tournament) = &self.tournament else {
            return 0;
        };
        let region = if self.view_round.is_final_four() {
            tournament.regions.iter().find(|r| r.name == "National")
        } else {
            tournament.regions.get(self.selected_region)
        };
        region
            .and_then(|r| r.rounds.iter().find(|rnd| rnd.kind == self.view_round))
            .map(|rnd| rnd.games.len())
            .unwrap_or(0)
    }
}

/// Detect the active tournament round by scanning game statuses.
/// Returns the first round that has any InProgress games, or failing that,
/// the last round that has any Final games.
fn detect_active_round(tournament: &Tournament) -> RoundKind {
    use ncaa_api::GameStatus;

    let round_order = [
        RoundKind::FirstFour,
        RoundKind::First,
        RoundKind::Second,
        RoundKind::Sweet16,
        RoundKind::Elite8,
        RoundKind::FinalFour,
        RoundKind::Championship,
    ];

    let mut last_with_games = RoundKind::First;

    for kind in round_order {
        let has_live = tournament.regions.iter().any(|reg| {
            reg.rounds
                .iter()
                .filter(|r| r.kind == kind)
                .flat_map(|r| r.games.iter())
                .any(|g| g.status == GameStatus::InProgress)
        });

        if has_live {
            return kind;
        }

        let has_final = tournament.regions.iter().any(|reg| {
            reg.rounds
                .iter()
                .filter(|r| r.kind == kind)
                .flat_map(|r| r.games.iter())
                .any(|g| g.status == GameStatus::Final)
        });

        if has_final {
            last_with_games = kind;
        }
    }

    last_with_games
}

// ---------------------------------------------------------------------------
// Game detail state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct GameDetailState {
    pub detail: Option<GameDetail>,
    pub scroll_offset: u16,
}

// ---------------------------------------------------------------------------
// Chat state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    pub id: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub is_system: bool,
}

#[derive(Debug, Clone)]
pub struct OutboundChatMessage {
    pub id: String,
    pub body: String,
}

#[derive(Debug)]
pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub composing: bool,
    pub scroll_offset: u16,
    pub username: String,
    pub room: String,
    pub connected: bool,
    pub endpoint: String,
    seen_ids: HashSet<String>,
}

impl Default for ChatState {
    fn default() -> Self {
        let username = std::env::var("USER")
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| "fan".to_string());
        Self {
            messages: vec![ChatMessage {
                id: "system-init".to_string(),
                author: "system".to_string(),
                body: "Chat starting... connecting to relay.".to_string(),
                timestamp: Local::now().format("%H:%M").to_string(),
                is_system: true,
            }],
            input: String::new(),
            composing: false,
            scroll_offset: 0,
            username,
            room: std::env::var("MMTUI_CHAT_ROOM").unwrap_or_else(|_| "march-madness".to_string()),
            connected: false,
            endpoint: std::env::var("MMTUI_CHAT_WS")
                .unwrap_or_else(|_| "ws://127.0.0.1:8787".to_string()),
            seen_ids: HashSet::new(),
        }
    }
}

impl ChatState {
    pub fn submit_input(&mut self) -> Option<OutboundChatMessage> {
        let msg = self.input.trim();
        if msg.is_empty() {
            self.composing = false;
            self.input.clear();
            return None;
        }
        let message = OutboundChatMessage {
            id: format!(
                "{}-{}",
                self.username,
                Local::now()
                    .timestamp_nanos_opt()
                    .unwrap_or_else(|| Local::now().timestamp_micros() * 1000)
            ),
            body: msg.to_string(),
        };
        self.ingest_message(ChatMessage {
            id: message.id.clone(),
            author: self.username.clone(),
            body: message.body.clone(),
            timestamp: Local::now().format("%H:%M").to_string(),
            is_system: false,
        });
        self.scroll_offset = 0;
        self.composing = false;
        self.input.clear();
        Some(message)
    }

    pub fn ingest_message(&mut self, msg: ChatMessage) {
        if !msg.id.is_empty() && self.seen_ids.contains(&msg.id) {
            return;
        }
        if !msg.id.is_empty() {
            self.seen_ids.insert(msg.id.clone());
        }
        self.messages.push(msg);
        if self.messages.len() > 200 {
            let remove_count = self.messages.len() - 200;
            self.messages.drain(0..remove_count);
        }
    }

    pub fn push_system(&mut self, body: impl Into<String>) {
        let body = body.into();
        if let Some(last) = self.messages.last()
            && last.is_system
            && last.body == body
        {
            return;
        }
        self.messages.push(ChatMessage {
            id: format!("system-{}", Local::now().timestamp_millis()),
            author: "system".to_string(),
            body,
            timestamp: Local::now().format("%H:%M").to_string(),
            is_system: true,
        });
    }
}

// ---------------------------------------------------------------------------
// Live feed state (lightweight recent play-by-play view)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct LivePlay {
    pub period: u8,
    pub clock: String,
    pub description: String,
    pub away_score: u16,
    pub home_score: u16,
    pub is_new: bool,
}

#[derive(Debug, Default)]
pub struct LiveFeedState {
    pub game_id: Option<String>,
    pub plays: Vec<LivePlay>,
}

impl LiveFeedState {
    pub fn update_from_detail(&mut self, detail: &GameDetail) {
        let prev_keys: HashSet<String> = self
            .plays
            .iter()
            .map(|p| play_key(p.period, &p.clock, &p.description, p.away_score, p.home_score))
            .collect();

        self.game_id = Some(detail.game_id.clone());
        self.plays = detail
            .plays
            .iter()
            .map(|p| {
                let key = play_key(p.period, &p.clock, &p.description, p.away_score, p.home_score);
                LivePlay {
                    period: p.period,
                    clock: p.clock.clone(),
                    description: p.description.clone(),
                    away_score: p.away_score,
                    home_score: p.home_score,
                    is_new: !prev_keys.contains(&key),
                }
            })
            .collect();
    }
}

fn play_key(period: u8, clock: &str, desc: &str, away: u16, home: u16) -> String {
    format!("{period}|{clock}|{away}|{home}|{desc}")
}

// ---------------------------------------------------------------------------
// Pick wizard state (currently pinned to 2025 bracket template)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WizardGame {
    pub game_id: String,
    pub round: RoundKind,
    pub top_label: String,
    pub bottom_label: String,
    pub top_team_id: Option<String>,
    pub bottom_team_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketPicks {
    pub user_id: String,
    pub year: u16,
    pub selections: HashMap<String, String>,
}

#[derive(Debug)]
pub struct PickWizardState {
    pub year: u16,
    pub games: Vec<WizardGame>,
    pub current_index: usize,
    pub selections: HashMap<String, String>,
    pub completed: bool,
}

impl Default for PickWizardState {
    fn default() -> Self {
        Self {
            year: 2025,
            games: Vec::new(),
            current_index: 0,
            selections: HashMap::new(),
            completed: false,
        }
    }
}

impl PickWizardState {
    pub fn load_from_tournament_2025_template(&mut self, tournament: &Tournament) {
        self.year = 2025;
        self.games.clear();
        self.current_index = 0;
        self.selections.clear();
        self.completed = false;

        let round_order = [
            RoundKind::FirstFour,
            RoundKind::First,
            RoundKind::Second,
            RoundKind::Sweet16,
            RoundKind::Elite8,
            RoundKind::FinalFour,
            RoundKind::Championship,
        ];

        for round in round_order {
            if round.is_final_four() {
                if let Some(national) = tournament.regions.iter().find(|r| r.name == "National") {
                    for r in &national.rounds {
                        if r.kind == round {
                            for g in &r.games {
                                self.games.push(WizardGame {
                                    game_id: g.id.clone(),
                                    round,
                                    top_label: format_seed_team(&g.top),
                                    bottom_label: format_seed_team(&g.bottom),
                                    top_team_id: g.top.team.as_ref().map(|t| t.id.clone()),
                                    bottom_team_id: g.bottom.team.as_ref().map(|t| t.id.clone()),
                                });
                            }
                        }
                    }
                }
            } else {
                for region in tournament.regions.iter().filter(|r| r.name != "National") {
                    for r in &region.rounds {
                        if r.kind == round {
                            for g in &r.games {
                                self.games.push(WizardGame {
                                    game_id: g.id.clone(),
                                    round,
                                    top_label: format_seed_team(&g.top),
                                    bottom_label: format_seed_team(&g.bottom),
                                    top_team_id: g.top.team.as_ref().map(|t| t.id.clone()),
                                    bottom_team_id: g.bottom.team.as_ref().map(|t| t.id.clone()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn current_game(&self) -> Option<&WizardGame> {
        self.games.get(self.current_index)
    }

    pub fn select_top(&mut self) {
        if let Some(g) = self.current_game().cloned() {
            let winner = g.top_team_id.unwrap_or_else(|| format!("top:{}", g.game_id));
            self.selections.insert(g.game_id, winner);
            self.advance();
        }
    }

    pub fn select_bottom(&mut self) {
        if let Some(g) = self.current_game().cloned() {
            let winner = g
                .bottom_team_id
                .unwrap_or_else(|| format!("bottom:{}", g.game_id));
            self.selections.insert(g.game_id, winner);
            self.advance();
        }
    }

    pub fn advance(&mut self) {
        if self.current_index + 1 < self.games.len() {
            self.current_index += 1;
        } else {
            self.completed = true;
        }
    }

    pub fn back(&mut self) {
        self.current_index = self.current_index.saturating_sub(1);
        self.completed = false;
    }

    pub fn to_export(&self, user_id: String) -> BracketPicks {
        BracketPicks {
            user_id,
            year: self.year,
            selections: self.selections.clone(),
        }
    }

    pub fn apply_saved_selections(&mut self, selections: HashMap<String, String>) {
        self.selections = selections;
        self.completed = self.games.iter().all(|g| self.selections.contains_key(&g.game_id));
        self.current_index = self
            .games
            .iter()
            .position(|g| !self.selections.contains_key(&g.game_id))
            .unwrap_or_else(|| self.games.len().saturating_sub(1));
    }
}

fn format_seed_team(seed: &TeamSeed) -> String {
    let seed_no = if seed.seed > 0 {
        seed.seed.to_string()
    } else {
        "-".to_string()
    };
    let team = seed
        .team
        .as_ref()
        .map(|t| t.short_name.clone())
        .or_else(|| seed.placeholder.clone())
        .unwrap_or_else(|| "TBD".to_string());
    format!("({seed_no}) {team}")
}

// ---------------------------------------------------------------------------
// Root app state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AppState {
    pub active_tab: MenuItem,
    pub previous_tab: MenuItem,
    pub show_intro: bool,
    pub show_logs: bool,
    pub last_error: Option<String>,
    pub bracket: BracketState,
    pub game_detail: GameDetailState,
    pub live_feed: LiveFeedState,
    pub chat: ChatState,
    pub pick_wizard: PickWizardState,
    pub animation: AnimationState,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            show_intro: true,
            ..Self::default()
        }
    }
}
