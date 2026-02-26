use crate::espn::{ScoreboardResponse, SummaryResponse, TournamentsResponse};
use crate::henrygd::HenrygdResponse;
use crate::{
    BoxScore, Game, GameDetail, GameStatus, Play, PlayerLine, Region, Round, RoundKind, Team,
    TeamSeed, Tournament,
};
use chrono::{DateTime, Datelike, Utc};
use reqwest::Client;
use std::fmt;
use std::time::Duration;

pub type ApiResult<T> = Result<T, ApiError>;

const ESPN_SITE_V2: &str =
    "https://site.api.espn.com/apis/site/v2/sports/basketball/mens-college-basketball";
const ESPN_V2: &str =
    "https://site.api.espn.com/apis/v2/sports/basketball/mens-college-basketball";
const NCAA_HENRYGD: &str = "https://ncaa-api.henrygd.me";
const FALLBACK_BRACKET_YEAR: i32 = 2025;
const FALLBACK_BRACKET_JSON: &str = include_str!("../../2025_bracket.json");

/// NCAA API client backed by ESPN's public endpoints.
#[derive(Debug, Clone)]
pub struct NcaaApi {
    client: Client,
    timeout: Duration,
}

impl Default for NcaaApi {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .user_agent("mmtui/0.1 (terminal bracket viewer)")
                .build()
                .unwrap_or_default(),
            timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    Network(reqwest::Error, String),
    Api(reqwest::Error, String),
    Parsing(reqwest::Error, String),
    NotFound(String),
    Other(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(e, url) => write!(f, "Network error for {url}: {e}"),
            ApiError::Api(e, url) => write!(f, "API error for {url}: {e}"),
            ApiError::Parsing(e, url) => write!(f, "Parse error for {url}: {e}"),
            ApiError::NotFound(msg) => write!(f, "Not found: {msg}"),
            ApiError::Other(msg) => write!(f, "Error: {msg}"),
        }
    }
}

impl NcaaApi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch the current NCAA Men's Basketball Tournament bracket.
    ///
    /// Fallback chain:
    /// 1) `MMTUI_BRACKET_JSON` env var — load from local ESPN-format JSON file.
    /// 2) NCAA henrygd API — authoritative bracket topology for current year.
    /// 3) ESPN tournaments API — bracket data for current and adjacent years.
    /// 4) Embedded 2025 JSON — last-resort offline fallback.
    pub async fn fetch_tournament(&self) -> ApiResult<Tournament> {
        if let Ok(path) = std::env::var("MMTUI_BRACKET_JSON")
            && !path.trim().is_empty()
        {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| ApiError::NotFound(format!("could not read {path}: {e}")))?;
            let raw: TournamentsResponse = serde_json::from_str(&content)
                .map_err(|e| ApiError::NotFound(format!("invalid tournament json at {path}: {e}")))?;

            let year =
                infer_year_from_path(&path).unwrap_or_else(|| season_tournament_year(Utc::now()) as u16);
            let entry = select_tournament_entry(raw.tournaments.unwrap_or_default(), i32::from(year))?;
            return Ok(map_tournament(entry, year));
        }

        let season_year = season_tournament_year(Utc::now()) as u16;

        // NCAA henrygd: authoritative bracket topology.
        let ncaa_url = format!("{NCAA_HENRYGD}/brackets/basketball-men/d1/{season_year}");
        if let Ok(raw) = self.get::<HenrygdResponse>(&ncaa_url).await {
            if let Some(champ) = raw.championships.into_iter().next() {
                if !champ.games.is_empty() {
                    return Ok(map_ncaa_championship(champ));
                }
            }
        }

        // ESPN fallback: bracket data for current and adjacent years.
        let candidate_years = candidate_tournament_years(Utc::now());
        let mut last_error: Option<ApiError> = None;
        for year in candidate_years {
            let url = format!("{ESPN_V2}/tournaments?limit=25&year={year}");
            match self.get::<TournamentsResponse>(&url).await {
                Ok(raw) => match select_tournament_entry(raw.tournaments.unwrap_or_default(), year) {
                    Ok(entry) => return Ok(map_tournament(entry, year as u16)),
                    Err(e) => last_error = Some(e),
                },
                Err(e) => last_error = Some(e),
            }
        }

        if let Ok(tournament) = load_embedded_fallback_tournament() {
            return Ok(tournament);
        }

        Err(last_error.unwrap_or_else(|| {
            ApiError::NotFound("NCAA Tournament not found in current/adjacent years".into())
        }))
    }

    /// Fetch the bracket skeleton from the NCAA henrygd API for a specific year.
    /// Useful for pre-loading the 2026 bracket structure before Selection Sunday.
    pub async fn fetch_ncaa_bracket(&self, year: u16) -> ApiResult<Tournament> {
        let url = format!("{NCAA_HENRYGD}/brackets/basketball-men/d1/{year}");
        let raw = self.get::<HenrygdResponse>(&url).await?;
        let champ = raw
            .championships
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::NotFound(format!("no championship data for {year}")))?;
        Ok(map_ncaa_championship(champ))
    }

    /// Fetch live scores for games currently in the NCAA tournament.
    /// groups=100 filters to tournament games on ESPN's scoreboard.
    pub async fn fetch_scoreboard(&self) -> ApiResult<Vec<Game>> {
        let url = format!("{ESPN_SITE_V2}/scoreboard?groups=100&limit=50");
        let raw: ScoreboardResponse = self.get(&url).await?;
        let games = raw
            .events
            .unwrap_or_default()
            .iter()
            .map(map_event_to_game)
            .collect();
        Ok(games)
    }

    /// Fetch detailed game data (play-by-play + box score).
    pub async fn fetch_game_detail(&self, game_id: &str) -> ApiResult<GameDetail> {
        let url = format!("{ESPN_SITE_V2}/summary?event={game_id}");
        let raw: SummaryResponse = self.get(&url).await?;
        Ok(map_summary(game_id, raw))
    }

    async fn get<T: Default + serde::de::DeserializeOwned>(&self, url: &str) -> ApiResult<T> {
        let response = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| ApiError::Network(e, url.to_owned()))?;

        match response.error_for_status() {
            Ok(res) => res
                .json::<T>()
                .await
                .map_err(|e| ApiError::Parsing(e, url.to_owned())),
            Err(e) => {
                if e.status().map(|s| s.is_client_error()).unwrap_or(false) {
                    Ok(T::default())
                } else {
                    Err(ApiError::Api(e, url.to_owned()))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mapping: NCAA henrygd wire types → clean domain types
// ---------------------------------------------------------------------------

/// Map a henrygd championship response into the mmtui Tournament domain type.
///
/// ID strategy:
///   - `Game.id` = bracketPositionId string (stable bracket anchor)
///   - `Game.espn_id` = None (populated later by team-matching bridge)
///
/// Region grouping: games are bucketed by sectionId. Region names come from
/// the championship's `regions[]` array; fall back to "Region {n}" pre-Selection Sunday.
fn map_ncaa_championship(champ: crate::henrygd::HenrygdChampionship) -> Tournament {
    use std::collections::HashMap;

    // Build sectionId → region name lookup; fall back to "Region {n}".
    let region_names: HashMap<u32, String> = champ
        .regions
        .iter()
        .map(|r| {
            let name = if r.title.is_empty() {
                format!("Region {}", r.section_id)
            } else {
                r.title.clone()
            };
            (r.section_id, name)
        })
        .collect();

    // sectionId 6 is the National/Final Four section.
    const NATIONAL_SECTION: u32 = 6;

    // Group games by sectionId → RoundKind → Vec<Game>.
    let mut sections: HashMap<u32, HashMap<RoundKind, Vec<Game>>> = HashMap::new();
    for g in &champ.games {
        let round_kind = round_number_to_kind(g.bracket_position_id / 100);
        let game = map_ncaa_game(g);
        sections
            .entry(g.section_id)
            .or_default()
            .entry(round_kind)
            .or_default()
            .push(game);
    }

    // Build Region list. National section always last.
    let region_order = ["East", "West", "South", "Midwest"];
    let mut regions: Vec<Region> = Vec::new();

    // Named regions first (in canonical order when known, otherwise insertion order).
    let mut section_ids: Vec<u32> = sections.keys().copied().filter(|&s| s != NATIONAL_SECTION).collect();
    section_ids.sort_unstable();

    // Try to match canonical region order; fall back to sorted sectionId order.
    let ordered_ids = {
        let named: Vec<u32> = region_order
            .iter()
            .filter_map(|name| {
                section_ids.iter().find(|&&sid| {
                    region_names.get(&sid).map(|n| n.as_str()) == Some(name)
                }).copied()
            })
            .collect();
        if named.len() == section_ids.len() { named } else { section_ids }
    };

    for sid in ordered_ids {
        if let Some(rounds_map) = sections.remove(&sid) {
            let name = region_names.get(&sid).cloned().unwrap_or_else(|| format!("Region {sid}"));
            regions.push(Region {
                id: name.to_lowercase().replace(' ', "-"),
                name,
                rounds: build_rounds(rounds_map),
            });
        }
    }

    // National section (Final Four + Championship).
    if let Some(rounds_map) = sections.remove(&NATIONAL_SECTION) {
        regions.push(Region {
            id: "national".into(),
            name: "National".into(),
            rounds: build_rounds(rounds_map),
        });
    }

    Tournament {
        id: format!("ncaa-{}", champ.year),
        name: champ.title,
        year: champ.year,
        regions,
    }
}

/// Sort a round map into a Vec<Round> ordered by RoundKind.
fn build_rounds(rounds_map: std::collections::HashMap<RoundKind, Vec<Game>>) -> Vec<Round> {
    let mut rounds: Vec<Round> = rounds_map
        .into_iter()
        .map(|(kind, games)| Round { kind, games })
        .collect();
    rounds.sort_by_key(|r| r.kind);
    rounds
}

/// Map a single henrygd game to the mmtui Game domain type.
fn map_ncaa_game(g: &crate::henrygd::HenrygdGame) -> Game {
    let tba = || TeamSeed { seed: 0, team: None, placeholder: Some("TBA".into()) };
    let top = g.teams.first().map(map_ncaa_team).unwrap_or_else(tba);
    let bottom = g.teams.get(1).map(map_ncaa_team).unwrap_or_else(tba);

    let winner_id = g.teams.iter().find(|t| t.winner == Some(true)).and_then(|t| t.team_id.clone());

    let status = match g.game_state.as_str() {
        "L" => GameStatus::InProgress,
        "F" => GameStatus::Final,
        _ => GameStatus::Scheduled,
    };

    Game {
        id: g.bracket_position_id.to_string(),
        espn_id: None, // Populated later by team-matching bridge.
        top,
        bottom,
        status,
        score: None, // henrygd bracket endpoint does not carry live scores.
        winner_id,
        period: None,
        clock: None,
        start_time: None,
        location: None,
    }
}

fn map_ncaa_team(t: &crate::henrygd::HenrygdTeam) -> TeamSeed {
    let team = t.team_id.as_ref().map(|id| Team {
        id: id.clone(),
        name: t.name.clone().unwrap_or_default(),
        short_name: t.short_name.clone().unwrap_or_else(|| t.name.clone().unwrap_or_default()),
        abbrev: String::new(),
        color: None,
    });
    let placeholder = if team.is_none() {
        t.description.clone().or_else(|| Some("TBA".into()))
    } else {
        None
    };
    TeamSeed { seed: t.seed.unwrap_or(0), team, placeholder }
}

// ---------------------------------------------------------------------------
// Mapping: ESPN wire types → clean domain types
// ---------------------------------------------------------------------------

fn infer_year_from_path(path: &str) -> Option<u16> {
    path.split(|c: char| !c.is_ascii_digit())
        .find_map(|token| {
            if token.len() == 4 {
                token.parse::<u16>().ok()
            } else {
                None
            }
        })
        .filter(|y| (2000..=2100).contains(y))
}

fn season_tournament_year(now: DateTime<Utc>) -> i32 {
    // NCAA tournament championship year tracks the season year. In Nov/Dec,
    // queries should target the next calendar year.
    if now.month() >= 11 { now.year() + 1 } else { now.year() }
}

fn candidate_tournament_years(now: DateTime<Utc>) -> Vec<i32> {
    let season_year = season_tournament_year(now);
    let mut years = vec![season_year, season_year - 1, season_year + 1, 2025];
    years.sort_unstable();
    years.dedup();
    years.sort_by(|a, b| {
        (a - season_year)
            .abs()
            .cmp(&(b - season_year).abs())
            .then_with(|| a.cmp(b))
    });
    years
}

fn load_embedded_fallback_tournament() -> ApiResult<Tournament> {
    let raw: TournamentsResponse = serde_json::from_str(FALLBACK_BRACKET_JSON)
        .map_err(|e| ApiError::NotFound(format!("invalid embedded fallback bracket json: {e}")))?;
    let entry = select_tournament_entry(raw.tournaments.unwrap_or_default(), FALLBACK_BRACKET_YEAR)?;
    Ok(map_tournament(entry, FALLBACK_BRACKET_YEAR as u16))
}

fn select_tournament_entry(
    mut entries: Vec<crate::espn::TournamentEntry>,
    year: i32,
) -> ApiResult<crate::espn::TournamentEntry> {
    if entries.is_empty() {
        return Err(ApiError::NotFound(format!(
            "no tournaments returned for year {year}"
        )));
    }

    let has_bracket = |t: &crate::espn::TournamentEntry| {
        t.bracket
            .as_ref()
            .and_then(|b| b.rounds.as_ref())
            .map(|rounds| !rounds.is_empty())
            .unwrap_or(false)
    };

    let name_match = |t: &crate::espn::TournamentEntry| {
        let n = t.name.as_deref().unwrap_or("").to_lowercase();
        (n.contains("ncaa")
            || n.contains("march")
            || n.contains("championship")
            || n.contains("tournament"))
            && !n.contains("nit")
            && !n.contains("invitational")
    };

    if let Some(idx) = entries.iter().position(|t| has_bracket(t) && name_match(t)) {
        return Ok(entries.swap_remove(idx));
    }

    if let Some(idx) = entries.iter().position(has_bracket) {
        return Ok(entries.swap_remove(idx));
    }

    Err(ApiError::NotFound(format!(
        "NCAA tournament bracket not found for year {year}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn season_year_uses_current_year_before_november() {
        let dt = Utc.with_ymd_and_hms(2026, 2, 24, 12, 0, 0).unwrap();
        assert_eq!(season_tournament_year(dt), 2026);
    }

    #[test]
    fn season_year_rolls_forward_in_november_and_december() {
        let nov = Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
        let dec = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();
        assert_eq!(season_tournament_year(nov), 2027);
        assert_eq!(season_tournament_year(dec), 2027);
    }

    #[test]
    fn candidate_years_are_nearest_first() {
        let dt = Utc.with_ymd_and_hms(2026, 2, 24, 12, 0, 0).unwrap();
        assert_eq!(candidate_tournament_years(dt), vec![2026, 2025, 2027]);
    }

    #[test]
    fn candidate_years_always_include_2025_snapshot_fallback() {
        let dt = Utc.with_ymd_and_hms(2027, 2, 24, 12, 0, 0).unwrap();
        assert!(candidate_tournament_years(dt).contains(&2025));
    }

    #[test]
    fn embedded_fallback_tournament_parses() {
        let t = load_embedded_fallback_tournament().expect("fallback bracket should parse");
        assert_eq!(t.year, 2025);
        assert!(!t.regions.is_empty());
    }

    #[test]
    fn test_round_number_mapping() {
        assert_eq!(round_number_to_kind(1), RoundKind::FirstFour);
        assert_eq!(round_number_to_kind(2), RoundKind::First);
        assert_eq!(round_number_to_kind(6), RoundKind::FinalFour);
        assert_eq!(round_number_to_kind(7), RoundKind::Championship);
    }

    #[test]
    fn test_parse_status() {
        assert_eq!(parse_status("STATUS_IN_PROGRESS"), GameStatus::InProgress);
        assert_eq!(parse_status("STATUS_FINAL"), GameStatus::Final);
        assert_eq!(parse_status("STATUS_SCHEDULED"), GameStatus::Scheduled);
        assert_eq!(parse_status("STATUS_POSTPONED"), GameStatus::Postponed);
    }

    #[test]
    fn test_round_kind_navigation() {
        assert_eq!(RoundKind::First.next(), Some(RoundKind::Second));
        assert_eq!(RoundKind::Championship.next(), None);
        assert_eq!(RoundKind::FirstFour.prev(), None);
        assert_eq!(RoundKind::FinalFour.is_final_four(), true);
        assert_eq!(RoundKind::Elite8.is_final_four(), false);
    }

    // -----------------------------------------------------------------------
    // NCAA henrygd adapter tests
    // -----------------------------------------------------------------------

    #[test]
    fn ncaa_position_to_round_covers_all_rounds() {
        assert_eq!(round_number_to_kind(101 / 100), RoundKind::FirstFour);
        assert_eq!(round_number_to_kind(201 / 100), RoundKind::First);
        assert_eq!(round_number_to_kind(301 / 100), RoundKind::Second);
        assert_eq!(round_number_to_kind(401 / 100), RoundKind::Sweet16);
        assert_eq!(round_number_to_kind(501 / 100), RoundKind::Elite8);
        assert_eq!(round_number_to_kind(601 / 100), RoundKind::FinalFour);
        assert_eq!(round_number_to_kind(701 / 100), RoundKind::Championship);
    }

    #[test]
    fn ncaa_game_with_empty_teams_produces_tba_slots() {
        let raw = crate::henrygd::HenrygdGame {
            bracket_position_id: 101,
            game_state: "P".into(),
            teams: vec![],
            section_id: 1,
            ..Default::default()
        };
        let game = map_ncaa_game(&raw);
        assert_eq!(game.id, "101");
        assert!(game.espn_id.is_none(), "espn_id must be None pre-bridge");
        assert!(game.top.team.is_none(), "top team should be None when teams is empty");
        assert!(game.bottom.team.is_none(), "bottom team should be None when teams is empty");
        assert_eq!(game.top.placeholder.as_deref(), Some("TBA"));
        assert_eq!(game.status, GameStatus::Scheduled);
    }

    #[test]
    fn ncaa_game_with_teams_maps_correctly() {
        let raw = crate::henrygd::HenrygdGame {
            bracket_position_id: 201,
            game_state: "F".into(),
            teams: vec![
                crate::henrygd::HenrygdTeam {
                    team_id: Some("uconn".into()),
                    name: Some("Connecticut".into()),
                    short_name: Some("UConn".into()),
                    seed: Some(1),
                    winner: Some(true),
                    description: None,
                },
                crate::henrygd::HenrygdTeam {
                    team_id: Some("stetson".into()),
                    name: Some("Stetson".into()),
                    short_name: None,
                    seed: Some(16),
                    winner: Some(false),
                    description: None,
                },
            ],
            section_id: 2,
            ..Default::default()
        };
        let game = map_ncaa_game(&raw);
        assert_eq!(game.id, "201");
        assert_eq!(game.winner_id.as_deref(), Some("uconn"));
        assert_eq!(game.top.seed, 1);
        assert_eq!(game.bottom.seed, 16);
        assert_eq!(game.status, GameStatus::Final);
    }

    #[test]
    fn ncaa_championship_empty_region_titles_fall_back_to_region_n() {
        use crate::henrygd::{HenrygdChampionship, HenrygdGame, HenrygdRegion};
        let champ = HenrygdChampionship {
            title: "2026 DI Men's Basketball Championship".into(),
            year: 2026,
            games: vec![HenrygdGame {
                bracket_position_id: 201,
                game_state: "P".into(),
                section_id: 1,
                ..Default::default()
            }],
            rounds: vec![],
            regions: vec![HenrygdRegion {
                id: "4031".into(),
                section_id: 1,
                title: String::new(), // empty pre-Selection Sunday
                region_code: "TL".into(),
            }],
        };
        let tournament = map_ncaa_championship(champ);
        assert_eq!(tournament.year, 2026);
        let region = tournament.regions.iter().find(|r| r.id != "national");
        assert!(region.is_some());
        assert!(
            region.unwrap().name.starts_with("Region "),
            "empty title should fall back to 'Region N', got: {}",
            region.unwrap().name
        );
    }

    #[test]
    fn ncaa_championship_national_section_maps_to_national_region() {
        use crate::henrygd::{HenrygdChampionship, HenrygdGame};
        let champ = HenrygdChampionship {
            title: "2026 Championship".into(),
            year: 2026,
            games: vec![HenrygdGame {
                bracket_position_id: 701,
                game_state: "P".into(),
                section_id: 6,
                ..Default::default()
            }],
            rounds: vec![],
            regions: vec![],
        };
        let tournament = map_ncaa_championship(champ);
        let national = tournament.regions.iter().find(|r| r.id == "national");
        assert!(national.is_some(), "sectionId 6 must produce the National region");
        let rounds = &national.unwrap().rounds;
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].kind, RoundKind::Championship);
    }
}

fn to_title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

fn map_tournament(entry: crate::espn::TournamentEntry, year: u16) -> Tournament {
    let name = entry.name.unwrap_or_else(|| "NCAA Tournament".into());
    let id = entry.id.clone();

    let bracket = entry.bracket.unwrap_or_default();
    let espn_rounds = bracket.rounds.unwrap_or_default();

    // Group rounds by region. Each game carries a `note` field naming its region
    // (e.g. "SOUTH", "EAST"). We normalise to title case so the names match the
    // canonical region_order list used below.
    let mut regions: std::collections::HashMap<String, Vec<Round>> =
        std::collections::HashMap::new();

    for espn_round in espn_rounds {
        let round_kind = round_number_to_kind(espn_round.number.unwrap_or(2));

        if round_kind.is_final_four() {
            // Final Four / Championship: all matchups belong to "National".
            let games: Vec<Game> = espn_round.games_iter().map(map_matchup).collect();
            if !games.is_empty() {
                regions
                    .entry("National".to_owned())
                    .or_default()
                    .push(Round { kind: round_kind, games });
            }
        } else {
            // Regular rounds: split matchups by their individual region note so
            // each of the four regional brackets gets only its own games.
            let mut by_region: std::collections::HashMap<String, Vec<Game>> =
                std::collections::HashMap::new();
            for matchup in espn_round.games_iter() {
                let region_name = matchup
                    .note
                    .as_deref()
                    .map(to_title_case)
                    .unwrap_or_else(|| "Region".to_owned());
                by_region.entry(region_name).or_default().push(map_matchup(matchup));
            }
            for (region_name, games) in by_region {
                regions
                    .entry(region_name)
                    .or_default()
                    .push(Round { kind: round_kind, games });
            }
        }
    }

    // Build ordered regions: East, West, South, Midwest, National
    let region_order = ["East", "West", "South", "Midwest", "National", "Region"];
    let mut built_regions: Vec<Region> = region_order
        .iter()
        .filter_map(|name| {
            regions.remove(*name).map(|rounds| Region {
                id: name.to_lowercase(),
                name: name.to_string(),
                rounds,
            })
        })
        .collect();

    // Append any remaining regions not in the canonical order
    for (name, rounds) in regions {
        built_regions.push(Region {
            id: name.to_lowercase(),
            name,
            rounds,
        });
    }

    Tournament { id, name, year, regions: built_regions }
}

fn round_number_to_kind(number: u32) -> RoundKind {
    match number {
        1 => RoundKind::FirstFour,
        2 => RoundKind::First,
        3 => RoundKind::Second,
        4 => RoundKind::Sweet16,
        5 => RoundKind::Elite8,
        6 => RoundKind::FinalFour,
        7 => RoundKind::Championship,
        _ => RoundKind::First,
    }
}

fn map_matchup(m: &crate::espn::EspnMatchup) -> Game {
    let id = m.id.clone().unwrap_or_default();

    // Matchups can embed a full event, or just have competitors directly.
    if let Some(event) = &m.event {
        return map_event_to_game(event);
    }

    let competitors = m.competitors.as_deref().unwrap_or_default();
    let (top, bottom) = split_competitors(competitors);

    let score = {
        let ts = competitors
            .iter()
            .find(|c| c.home_away.as_deref() == Some("home"))
            .or_else(|| competitors.first())
            .and_then(|c| c.score.as_ref())
            .and_then(|s| s.parse::<u16>().ok());
        let bs = competitors
            .iter()
            .find(|c| c.home_away.as_deref() == Some("away"))
            .or_else(|| competitors.get(1))
            .and_then(|c| c.score.as_ref())
            .and_then(|s| s.parse::<u16>().ok());
        ts.zip(bs)
    };

    let winner_id = competitors
        .iter()
        .find(|c| c.winner == Some(true))
        .and_then(|c| c.id.clone());

    let status = if score.is_some() {
        GameStatus::Final
    } else {
        GameStatus::Scheduled
    };

    Game {
        espn_id: Some(id.clone()),
        id,
        top,
        bottom,
        status,
        score,
        winner_id,
        period: None,
        clock: None,
        start_time: None,
        location: None,
    }
}

fn map_event_to_game(event: &crate::espn::EspnEvent) -> Game {
    let id = event.id.clone().unwrap_or_default();

    let status = event
        .status
        .as_ref()
        .and_then(|s| s.status_type.as_ref())
        .and_then(|t| t.name.as_deref())
        .map(parse_status)
        .unwrap_or_default();

    let period = event.status.as_ref().and_then(|s| s.period);
    let clock = event
        .status
        .as_ref()
        .and_then(|s| s.display_clock.clone());

    let location = event.venue.as_ref().and_then(|v| {
        match (&v.full_name, &v.city, &v.state) {
            (Some(name), _, _) => Some(name.clone()),
            (None, Some(city), Some(state)) => Some(format!("{city}, {state}")),
            _ => None,
        }
    });

    let start_time = event
        .date
        .as_deref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Flatten competitions → competitors
    let competitors: Vec<&crate::espn::EspnCompetitor> = event
        .competitions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .flat_map(|c| c.competitors.iter().flatten())
        .collect();

    let (top, bottom) = split_competitor_refs(&competitors);

    // Derive score from competitors
    let score = {
        let ts = top.team.as_ref().and_then(|_| top.score.as_ref());
        let bs = bottom.team.as_ref().and_then(|_| bottom.score.as_ref());
        if let (Some(t), Some(b)) = (ts, bs) {
            t.parse::<u16>().ok().zip(b.parse::<u16>().ok())
        } else {
            None
        }
    };

    let winner_id = competitors
        .iter()
        .find(|c| c.winner == Some(true))
        .and_then(|c| c.id.clone());

    Game {
        espn_id: Some(id.clone()),
        id,
        top: map_competitor(&top),
        bottom: map_competitor(&bottom),
        status,
        score,
        winner_id,
        period,
        clock,
        start_time,
        location,
    }
}

fn split_competitors(
    competitors: &[crate::espn::EspnCompetitor],
) -> (TeamSeed, TeamSeed) {
    // Use "home" as top, "away" as bottom; fall back to index order
    let top = competitors
        .iter()
        .find(|c| c.home_away.as_deref() == Some("home"))
        .or_else(|| competitors.first());
    let bottom = competitors
        .iter()
        .find(|c| c.home_away.as_deref() == Some("away"))
        .or_else(|| competitors.get(1));
    (
        top.map(map_competitor).unwrap_or_default(),
        bottom.map(map_competitor).unwrap_or_default(),
    )
}

fn split_competitor_refs<'a>(
    competitors: &[&'a crate::espn::EspnCompetitor],
) -> (&'a crate::espn::EspnCompetitor, &'a crate::espn::EspnCompetitor) {
    static DEFAULT: crate::espn::EspnCompetitor = crate::espn::EspnCompetitor {
        id: None,
        home_away: None,
        team: None,
        score: None,
        winner: None,
        curated_rank: None,
        records: None,
        placeholder: None,
    };

    let top = competitors
        .iter()
        .find(|c| c.home_away.as_deref() == Some("home"))
        .copied()
        .or_else(|| competitors.first().copied())
        .unwrap_or(&DEFAULT);
    let bottom = competitors
        .iter()
        .find(|c| c.home_away.as_deref() == Some("away"))
        .copied()
        .or_else(|| competitors.get(1).copied())
        .unwrap_or(&DEFAULT);
    (top, bottom)
}

fn map_competitor(c: &crate::espn::EspnCompetitor) -> TeamSeed {
    let seed = c
        .curated_rank
        .as_ref()
        .and_then(|r| r.current)
        .unwrap_or(0);

    let team = c.team.as_ref().map(|t| Team {
        id: t.id.clone().unwrap_or_default(),
        name: t.display_name.clone().unwrap_or_default(),
        short_name: t
            .short_display_name
            .clone()
            .unwrap_or_else(|| t.display_name.clone().unwrap_or_default()),
        abbrev: t.abbreviation.clone().unwrap_or_default(),
        color: t.color.clone(),
    });

    TeamSeed {
        seed,
        team,
        placeholder: c.placeholder.clone(),
    }
}

fn parse_status(s: &str) -> GameStatus {
    match s {
        "STATUS_IN_PROGRESS" | "STATUS_HALFTIME" => GameStatus::InProgress,
        "STATUS_FINAL" | "STATUS_FINAL_OT" => GameStatus::Final,
        "STATUS_POSTPONED" | "STATUS_CANCELLED" | "STATUS_SUSPENDED" => GameStatus::Postponed,
        _ => GameStatus::Scheduled,
    }
}

fn map_summary(game_id: &str, raw: SummaryResponse) -> GameDetail {
    let plays = raw
        .plays
        .unwrap_or_default()
        .into_iter()
        .map(|p| Play {
            period: p.period.and_then(|x| x.number).unwrap_or_default(),
            clock: p
                .clock
                .and_then(|c| c.display_value)
                .unwrap_or_default(),
            description: p.text.unwrap_or_default(),
            home_score: p.home_score.unwrap_or_default(),
            away_score: p.away_score.unwrap_or_default(),
        })
        .collect();

    let mut home_box = BoxScore::default();
    let mut away_box = BoxScore::default();

    if let Some(boxscore) = raw.boxscore {
        let team_players = boxscore.players.unwrap_or_default();
        for (i, team_data) in team_players.into_iter().enumerate() {
            let box_score = build_box_score(team_data);
            if i == 0 {
                home_box = box_score;
            } else {
                away_box = box_score;
            }
        }
    }

    GameDetail {
        game_id: game_id.to_owned(),
        plays,
        home_box,
        away_box,
    }
}

fn build_box_score(team_data: crate::espn::EspnTeamPlayers) -> BoxScore {
    let team = team_data.team.as_ref().map(|t| Team {
        id: t.id.clone().unwrap_or_default(),
        name: t.display_name.clone().unwrap_or_default(),
        short_name: t.short_display_name.clone().unwrap_or_default(),
        abbrev: t.abbreviation.clone().unwrap_or_default(),
        color: t.color.clone(),
    });

    let stats_cat = team_data
        .statistics
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name.as_deref() == Some("athletes"));

    let (players, totals) = stats_cat
        .map(|cat| {
            let keys = cat.keys.unwrap_or_default();
            let athletes = cat.athletes.unwrap_or_default();
            let raw_totals = cat.totals.unwrap_or_default();

            let player_lines = athletes
                .into_iter()
                .map(|a| {
                    let name = a
                        .athlete
                        .and_then(|ath| ath.display_name)
                        .unwrap_or_default();
                    parse_player_stats(name, &a.stats.unwrap_or_default(), &keys)
                })
                .collect();

            let totals_line = parse_player_stats("TOTALS".into(), &raw_totals, &keys);
            (player_lines, totals_line)
        })
        .unwrap_or_default();

    BoxScore { team, players, totals }
}

fn parse_player_stats(name: String, stats: &[String], keys: &[String]) -> PlayerLine {
    let get = |key: &str| -> String {
        keys.iter()
            .position(|k| k == key)
            .and_then(|i| stats.get(i))
            .cloned()
            .unwrap_or_default()
    };

    let parse_u16 = |key: &str| get(key).parse::<u16>().unwrap_or_default();

    PlayerLine {
        name,
        points: parse_u16("PTS"),
        rebounds: parse_u16("REB"),
        assists: parse_u16("AST"),
        minutes: get("MIN"),
        fg: get("FG"),
        fg3: get("3PT"),
    }
}
