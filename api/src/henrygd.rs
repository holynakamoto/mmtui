/// Wire types for the henrygd NCAA bracket API.
/// Endpoint: https://ncaa-api.henrygd.me/brackets/basketball-men/d1/{year}
use serde::Deserialize;

#[derive(Deserialize, Default, Debug)]
pub struct HenrygdResponse {
    pub championships: Vec<HenrygdChampionship>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdChampionship {
    pub title: String,
    pub year: u16,
    pub games: Vec<HenrygdGame>,
    pub rounds: Vec<HenrygdRound>,
    pub regions: Vec<HenrygdRegion>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdGame {
    pub bracket_position_id: u32,
    pub victor_bracket_position_id: Option<u32>,
    pub contest_id: Option<u64>,
    pub game_state: String,
    /// Empty vec pre-Selection Sunday; populated once bracket is announced.
    #[serde(default)]
    pub teams: Vec<HenrygdTeam>,
    pub section_id: u32,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub start_time: String,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdTeam {
    pub team_id: Option<String>,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub seed: Option<u8>,
    pub winner: Option<bool>,
    pub description: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdRound {
    pub id: String,
    pub round_number: u32,
    pub label: String,
    #[serde(default)]
    pub subtitle: String,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdRegion {
    pub id: String,
    pub section_id: u32,
    /// Empty string pre-Selection Sunday; populated when regions are assigned.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub region_code: String,
}
