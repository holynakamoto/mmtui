# NCAA Hybrid Bracket Adapter — Design Document

**Date:** 2026-02-24
**Status:** Approved, ready for implementation

---

## Understanding Summary

- **What:** A hybrid bracket data source — NCAA henrygd API for bracket topology, ESPN for live scoreboard and game detail
- **Why:** ESPN's bracket structure drifts mid-tournament; the NCAA API provides authoritative `bracketPositionId` topology that never changes
- **Who:** mmtui end users who need a reliable bracket surviving ESPN API drift during March Madness
- **Constraints:** ESPN must be kept for `fetch_scoreboard` and `fetch_game_detail`; must work pre-Selection Sunday with empty teams; 2025 embedded fallback must remain unchanged
- **Non-goals:** Replacing ESPN live score feed, replacing `fetch_game_detail` with NCAA equivalent

---

## Assumptions

1. `bracketPositionId / 100` gives the round number (101→1, 201→2, …, 701→7) — stable convention
2. `sectionId` 1–5 = four regions + First Four; `sectionId` 6 = National/Final Four
3. Region names (East/West/South/Midwest) are populated in `regions[].title` post-Selection Sunday; empty strings pre-Selection Sunday
4. The embedded 2025 fallback stays ESPN-format and continues to work unchanged
5. `espn_id` is `None` until the team-matching bridge fires on first scoreboard load
6. ESPN and NCAA team name strings may differ — normalize to lowercase alphanumeric for fuzzy matching
7. `victorBracketPositionId` is the canonical source for bracket advancement logic (not hardcoded ranges)

---

## Decisions

| Decision | Chosen | Alternatives | Reason |
|---|---|---|---|
| Bracket data source | NCAA henrygd API primary | ESPN primary | Authoritative topology, immune to ESPN field drift |
| Live data source | ESPN retained | NCAA scoreboard | ESPN has richer play-by-play; no NCAA live equivalent |
| `Game.id` invariant | Stable within source: bracketPositionId (NCAA path), ESPN event ID (ESPN fallback path) | Universal bracketPositionId | 2025 fallback has no bracketPositionId; within-source consistency sufficient |
| Bridge field | `espn_id: Option<String>` on `Game` | Bridge HashMap in App state | Self-documenting, mechanical change, 2025 path unaffected |
| `LoadGameDetail` message | `{ bracket_id: String, espn_id: Option<String> }` | Pass full `Game`, keep single String | Minimal, carries exactly what network handler needs |
| Pre-Selection Sunday behavior | Skip game detail if `espn_id = None` | Error, stub | Clean UX: feature disabled until bridge fires |
| Round derivation | `position_id / 100` | Match against `rounds[]` array | Deterministic, O(1), no traversal needed |
| Region grouping | `sectionId` key + `regions[].title` label | Manual region code mapping | Automatically hydrates on Selection Sunday |
| Region name fallback | `"Region {sectionId}"` when title empty | Panic / error | Working skeleton pre-Selection Sunday |
| Team name matching | Normalize to lowercase alphanumeric | Exact match | "UConn" vs "Connecticut" and similar ESPN/NCAA discrepancies |
| `fetch_tournament` fallback chain | NCAA henrygd → ESPN → embedded 2025 JSON | ESPN first | NCAA is structural ground truth; ESPN is fallback |

---

## Risks

- **henrygd API SLA**: No uptime guarantee. ESPN bracket fetch must remain as fallback.
- **Team name normalization failures**: If normalization still produces mismatches, bridge won't fire and `espn_id` stays `None`. Game detail silently unavailable — acceptable degradation.
- **First Four edge cases**: 4 play-in games feed into the 32-game First Round. `victorBracketPositionId` handles advancement correctly.
- **ID inconsistency across sources**: 2025 picks (ESPN IDs) and 2026 picks (bracketPositionIds) stored in separate year files — no collision.

---

## Architecture: Four Implementation Hunks

### Hunk 1 — Domain model (`api/src/lib.rs`)

Add `espn_id: Option<String>` to `Game` struct:

```rust
pub struct Game {
    pub id: String,              // bracketPositionId (NCAA path) or ESPN event ID (ESPN path)
    pub espn_id: Option<String>, // ESPN event ID for routing fetch_game_detail; None pre-bridge
    // ... existing fields unchanged
}
```

ESPN mapper: set `id = espn_event.id` AND `espn_id = Some(espn_event.id)` — identical for 2025 fallback.

### Hunk 2 — Wire types (`api/src/henrygd.rs` — new file)

Serde structs for henrygd NCAA JSON:

```rust
#[derive(Deserialize, Default, Debug)]
pub struct HenrygdResponse {
    pub championships: Vec<HenrygdChampionship>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdChampionship {
    pub title: String,
    pub year: u16,
    pub season: u16,
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
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdRound {
    pub id: String,
    pub round_number: u32,
    pub label: String,
    pub subtitle: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HenrygdRegion {
    pub id: String,
    pub section_id: u32,
    pub title: String,
    pub region_code: String,
}
```

### Hunk 3 — NCAA mapper + `fetch_tournament` fallback chain (`api/src/client.rs`)

New `fetch_tournament_from_ncaa()` method on `NcaaApi`:

```
1. GET https://ncaa-api.henrygd.me/brackets/basketball-men/d1/{year}
2. Deserialize into HenrygdResponse
3. map_ncaa_championship() → Tournament
   - Group games by sectionId → Region
   - Derive round from position_id / 100 → RoundKind
   - Region names from regions[].title; fallback "Region {n}"
   - teams[] empty → TeamSeed { team: None, placeholder: Some("TBA") }
   - Game { id: bracketPositionId.to_string(), espn_id: None, ... }
4. sectionId 6 → "National" region
```

Update `fetch_tournament()` fallback chain:
```
1. MMTUI_BRACKET_JSON env var (unchanged)
2. NCAA henrygd API (new primary)
3. ESPN tournaments API (existing fallback)
4. Embedded 2025 JSON (unchanged final fallback)
```

ESPN mapper update: add `espn_id: Some(event.id.clone())` to `map_matchup` and `map_event_to_game`.

### Hunk 4 — Message + network handler (`src/state/messages.rs`, `src/state/network.rs`)

```rust
// messages.rs
LoadGameDetail {
    bracket_id: String,
    espn_id: Option<String>,
}

// network.rs
NetworkRequest::LoadGameDetail { bracket_id, espn_id } => {
    match espn_id {
        Some(eid) => self.handle_load_game_detail(eid).await,
        None => {
            debug!("game detail unavailable for bracket pos {bracket_id}: no ESPN ID yet");
            Ok(NetworkResponse::Error { message: "Game detail not yet available (pre-Selection Sunday)".into() })
        }
    }
}
```

Update callers in `keys.rs` to send `bracket_id: game.id.clone(), espn_id: game.espn_id.clone()`.

---

## Team-Matching Bridge (Post-Selection Sunday)

Separate concern — not needed for the skeleton build. When implemented:

1. On each `RefreshScores` response, iterate ESPN games
2. For each ESPN game, normalize team names: lowercase, strip non-alphanumeric
3. Find matching game in `Tournament` by comparing normalized top+bottom team names
4. If found: `tournament_game.espn_id = Some(espn_game.id)`
5. Bridge is lazy — only fires when both sides are known

---

## Implementation Order

1. `api/src/lib.rs` — add `espn_id` field to `Game`
2. `api/src/henrygd.rs` — new file, wire type structs
3. `api/src/client.rs` — NCAA mapper + fallback chain + ESPN mapper `espn_id` population
4. `src/state/messages.rs` + `src/state/network.rs` — message evolution
5. `src/keys.rs` — update `LoadGameDetail` senders
6. `cargo test` — verify 14 baseline tests pass, add tests for NCAA mapper
