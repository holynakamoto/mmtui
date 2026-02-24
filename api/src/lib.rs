pub mod client;
pub mod espn;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Domain types — clean model, independent of ESPN wire format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Tournament {
    pub id: String,
    pub name: String,
    pub year: u16,
    pub regions: Vec<Region>,
}

impl Tournament {
    /// Find a game by ID across all regions and rounds.
    pub fn find_game_mut(&mut self, game_id: &str) -> Option<&mut Game> {
        for region in &mut self.regions {
            for round in &mut region.rounds {
                for game in &mut round.games {
                    if game.id == game_id {
                        return Some(game);
                    }
                }
            }
        }
        None
    }

    /// Merge partial game updates (from scoreboard refresh) into the tree.
    pub fn merge_updates(&mut self, updates: Vec<Game>) {
        for update in updates {
            if let Some(game) = self.find_game_mut(&update.id) {
                *game = update;
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Region {
    pub id: String,
    pub name: String, // "East", "West", "South", "Midwest", "National" (Final Four)
    pub rounds: Vec<Round>,
}

#[derive(Debug, Clone, Default)]
pub struct Round {
    pub kind: RoundKind,
    pub games: Vec<Game>,
}

/// Navigation axis replacing NaiveDate. Ordered from earliest to latest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RoundKind {
    #[default]
    FirstFour,    // Play-in games (Tuesday/Wednesday before the tournament)
    First,        // Round of 64
    Second,       // Round of 32
    Sweet16,
    Elite8,
    FinalFour,    // National semifinals
    Championship,
}

impl RoundKind {
    pub fn label(&self) -> &'static str {
        match self {
            RoundKind::FirstFour => "First Four",
            RoundKind::First => "1st Round",
            RoundKind::Second => "2nd Round",
            RoundKind::Sweet16 => "Sweet 16",
            RoundKind::Elite8 => "Elite Eight",
            RoundKind::FinalFour => "Final Four",
            RoundKind::Championship => "Championship",
        }
    }

    pub fn is_final_four(&self) -> bool {
        matches!(self, RoundKind::FinalFour | RoundKind::Championship)
    }

    pub fn prev(self) -> Option<Self> {
        match self {
            RoundKind::FirstFour => None,
            RoundKind::First => Some(RoundKind::FirstFour),
            RoundKind::Second => Some(RoundKind::First),
            RoundKind::Sweet16 => Some(RoundKind::Second),
            RoundKind::Elite8 => Some(RoundKind::Sweet16),
            RoundKind::FinalFour => Some(RoundKind::Elite8),
            RoundKind::Championship => Some(RoundKind::FinalFour),
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            RoundKind::FirstFour => Some(RoundKind::First),
            RoundKind::First => Some(RoundKind::Second),
            RoundKind::Second => Some(RoundKind::Sweet16),
            RoundKind::Sweet16 => Some(RoundKind::Elite8),
            RoundKind::Elite8 => Some(RoundKind::FinalFour),
            RoundKind::FinalFour => Some(RoundKind::Championship),
            RoundKind::Championship => None,
        }
    }

    /// FirstFour games don't fit the clean 2^n vertical alignment.
    /// This y-offset nudges them into position relative to the main bracket.
    pub fn vertical_offset(&self) -> u16 {
        match self {
            RoundKind::FirstFour => 1,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Game {
    pub id: String,
    pub top: TeamSeed,    // higher seed (or "top" of the bracket slot)
    pub bottom: TeamSeed, // lower seed (or "bottom" of the bracket slot)
    pub status: GameStatus,
    pub score: Option<(u16, u16)>, // (top_score, bottom_score)
    pub winner_id: Option<String>, // from API winner flag, avoids edge-case score logic
    pub period: Option<u8>,
    pub clock: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub location: Option<String>,
}

impl Game {
    pub fn is_live(&self) -> bool {
        self.status == GameStatus::InProgress
    }

    pub fn winner(&self) -> Option<&Team> {
        let winner_id = self.winner_id.as_deref()?;
        if self.top.team.as_ref().map(|t| t.id.as_str()) == Some(winner_id) {
            self.top.team.as_ref()
        } else {
            self.bottom.team.as_ref()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TeamSeed {
    pub seed: u8,
    pub team: Option<Team>, // None = TBD / "Winner of Game X"
    pub placeholder: Option<String>, // "Winner of #42" etc.
}

#[derive(Debug, Clone, Default)]
pub struct Team {
    pub id: String,
    pub name: String,        // "Duke Blue Devils"
    pub short_name: String,  // "Duke"
    pub abbrev: String,      // "DUKE"
    pub color: Option<String>, // hex color from ESPN
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GameStatus {
    #[default]
    Scheduled,
    InProgress,
    Final,
    Postponed,
}

/// Detailed game data (play-by-play, box score) fetched on demand.
#[derive(Debug, Clone, Default)]
pub struct GameDetail {
    pub game_id: String,
    pub plays: Vec<Play>,
    pub home_box: BoxScore,
    pub away_box: BoxScore,
}

#[derive(Debug, Clone, Default)]
pub struct Play {
    pub period: u8,
    pub clock: String,
    pub description: String,
    pub home_score: u16,
    pub away_score: u16,
}

#[derive(Debug, Clone, Default)]
pub struct BoxScore {
    pub team: Option<Team>,
    pub players: Vec<PlayerLine>,
    pub totals: PlayerLine,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerLine {
    pub name: String,
    pub points: u16,
    pub rebounds: u16,
    pub assists: u16,
    pub minutes: String,
    pub fg: String,  // "7-12"
    pub fg3: String, // "2-5"
}
