use crate::espn::{ScoreboardResponse, SummaryResponse, TournamentsResponse};
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
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(e, url) => write!(f, "Network error for {url}: {e}"),
            ApiError::Api(e, url) => write!(f, "API error for {url}: {e}"),
            ApiError::Parsing(e, url) => write!(f, "Parse error for {url}: {e}"),
            ApiError::NotFound(msg) => write!(f, "Not found: {msg}"),
        }
    }
}

impl NcaaApi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch the current NCAA Men's Basketball Tournament bracket.
    ///
    /// Fallbacks:
    /// 1) If `MMTUI_BRACKET_JSON` is set, load from local JSON file.
    /// 2) Otherwise query ESPN for current year, then adjacent years.
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


#[cfg(test)]
mod tests {
    use super::*;

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
}
