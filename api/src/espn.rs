/// ESPN API raw wire types — serde shapes for deserializing ESPN responses.
/// These map to our clean domain types via the From/Into impls in client.rs.
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Tournament bracket  (v2 API)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct TournamentsResponse {
    pub tournaments: Option<Vec<TournamentEntry>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TournamentEntry {
    pub id: String,
    pub name: Option<String>,
    pub bracket: Option<Bracket>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Bracket {
    pub rounds: Option<Vec<EspnRound>>,
    pub full: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnRound {
    pub number: Option<u32>,
    pub name: Option<String>,
    #[serde(rename = "matchups")]
    pub matchups: Option<Vec<EspnMatchup>>,
    /// Some ESPN responses nest games as "games" instead of "matchups"
    pub games: Option<Vec<EspnMatchup>>,
}

impl EspnRound {
    pub fn games_iter(&self) -> impl Iterator<Item = &EspnMatchup> {
        self.matchups
            .iter()
            .flatten()
            .chain(self.games.iter().flatten())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnMatchup {
    pub id: Option<String>,
    #[serde(rename = "event")]
    pub event: Option<EspnEvent>,
    pub competitors: Option<Vec<EspnCompetitor>>,
    pub note: Option<String>, // region name on some endpoints
}

// ---------------------------------------------------------------------------
// Scoreboard  (site v2 API)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ScoreboardResponse {
    pub events: Option<Vec<EspnEvent>>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct EspnEvent {
    pub id: Option<String>,
    pub name: Option<String>,
    pub status: Option<EspnStatus>,
    pub competitions: Option<Vec<EspnCompetition>>,
    pub date: Option<String>, // ISO 8601
    pub venue: Option<EspnVenue>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnStatus {
    #[serde(rename = "type")]
    pub status_type: Option<EspnStatusType>,
    pub period: Option<u8>,
    #[serde(rename = "displayClock")]
    pub display_clock: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnStatusType {
    pub name: Option<String>, // "STATUS_SCHEDULED", "STATUS_IN_PROGRESS", "STATUS_FINAL"
    pub completed: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnCompetition {
    pub competitors: Option<Vec<EspnCompetitor>>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EspnCompetitor {
    pub id: Option<String>,
    #[serde(rename = "homeAway")]
    pub home_away: Option<String>, // "home" | "away" — also used as top/bottom
    pub team: Option<EspnTeam>,
    pub score: Option<String>, // ESPN sends scores as strings
    pub winner: Option<bool>,
    #[serde(rename = "curatedRank")]
    pub curated_rank: Option<EspnRank>,
    pub records: Option<Vec<EspnRecord>>,
    pub placeholder: Option<String>, // "Winner of Game #42"
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnTeam {
    pub id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "shortDisplayName")]
    pub short_display_name: Option<String>,
    pub abbreviation: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnRank {
    pub current: Option<u8>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnRecord {
    #[serde(rename = "type")]
    pub record_type: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnVenue {
    #[serde(rename = "fullName")]
    pub full_name: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
}

// ---------------------------------------------------------------------------
// Game summary  (site v2 API)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SummaryResponse {
    pub plays: Option<Vec<EspnPlay>>,
    pub boxscore: Option<EspnBoxscore>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnPlay {
    pub period: Option<EspnPeriod>,
    pub clock: Option<EspnClock>,
    pub text: Option<String>,
    #[serde(rename = "homeScore")]
    pub home_score: Option<u16>,
    #[serde(rename = "awayScore")]
    pub away_score: Option<u16>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnPeriod {
    pub number: Option<u8>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnClock {
    #[serde(rename = "displayValue")]
    pub display_value: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct EspnBoxscore {
    pub players: Option<Vec<EspnTeamPlayers>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnTeamPlayers {
    pub team: Option<EspnTeam>,
    pub statistics: Option<Vec<EspnStatCategory>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnStatCategory {
    pub name: Option<String>,
    pub athletes: Option<Vec<EspnAthleteStats>>,
    pub totals: Option<Vec<String>>,
    pub keys: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnAthleteStats {
    pub athlete: Option<EspnAthlete>,
    pub stats: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EspnAthlete {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}
