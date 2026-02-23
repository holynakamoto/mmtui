# mmbt — March Madness Bracket Terminal
## Implementation Plan

**Branch:** feature/march-madness
**Worktree:** .worktrees/march-madness
**Goal:** Fork mlbt into a full NCAA tournament TUI with animated ASCII banner

---

## Batch 1 — Foundation (crates + domain types)

- [ ] 1. Project rename: update root `Cargo.toml` (name, description, keywords), rename `mlb-api` → `ncaa-api` in `api/Cargo.toml`, update workspace member references
- [ ] 2. Create `ncaa-api` domain types in `api/src/lib.rs`: `Tournament`, `Region`, `Round`, `RoundKind`, `Game`, `GameStatus`, `Team`, `TeamSeed`, `GameDetail`
- [ ] 3. Implement ESPN API client in `api/src/client.rs`: `fetch_tournament()`, `fetch_scoreboard()`, `fetch_game_detail()` using reqwest+serde

## Batch 2 — State & Messaging Layer

- [ ] 4. Replace `src/state/messages.rs`: new `NetworkRequest` (LoadBracket, RefreshScores, LoadGameDetail) and `NetworkResponse` (BracketLoaded, BracketUpdated, GameDetailLoaded) variants
- [ ] 5. Replace `src/state/app_state.rs`: `BracketState` (tournament, selected_round, selected_region, selected_game, scroll_offset), `AnimationState` (frame, tick, theme), updated `AppState`
- [ ] 6. Update `src/app.rs`: new `MenuItem` enum (Bracket, Scoreboard, GameDetail), remove MLB-specific state

## Batch 3 — Network & Animation Infrastructure

- [ ] 7. Replace `src/state/network.rs`: wire `LoadBracket` → ESPN tournament endpoint, `RefreshScores` → scoreboard endpoint, `LoadGameDetail` → summary endpoint; implement `BracketUpdated` merge logic
- [ ] 8. Update `src/state/refresher.rs`: periodic `RefreshScores` tick (replace schedule refresh logic)
- [ ] 9. Add `UiEvent::AnimationTick` to messages; add 80ms Tokio interval task in `main.rs`; handle tick in `handle_ui_event` (increment `animation.frame`)

## Batch 4 — Animated Banner

- [ ] 10. Create `src/components/banner_frames.rs`: basketball + bracket header ASCII art, `FRAME_COUNT` const, `FRAMES` const array, `BannerColor` enum, `BannerTheme` detection, `resolve_color()` fn
- [ ] 11. Rewrite `src/components/banner.rs`: `AnimatedBanner` struct implementing `ratatui::widgets::Widget`; triangle-wave basketball offset from `tick % 8`; grouped `Span` rendering to minimize ANSI codes

## Batch 5 — Bracket Rendering Engine

- [ ] 12. Create `src/components/bracket.rs`: `BracketGrid`, `GameCell` structs; `BracketGrid::compute()` pre-calculation using power-of-2 spacing; `draw_outbound_connector` flag; adaptive `col_widths` (full name ≥100 cols, abbrev <100 cols)
- [ ] 13. Create `src/components/final_four.rs`: hardcoded 3-game `FinalFourLayout` centered renderer; `Winner` color role for bracket path

## Batch 6 — View Components

- [ ] 14. Create `src/components/scoreboard.rs`: live games list for current round, status indicators (InProgress/Final/Scheduled), replacing `schedule.rs`
- [ ] 15. Create `src/components/game_detail.rs`: box score + play-by-play for selected game, replacing `gameday/` and `boxscore.rs`
- [ ] 16. Update `src/keys.rs`: bracket navigation (←/→ rounds, ↑/↓ games, r=region, Enter=detail), remove MLB bindings

## Batch 7 — Layout, Draw & Integration

- [ ] 17. Update `src/ui/layout.rs`: 3-panel layout (banner header, tab bar, content); dynamic content area for Bracket/Scoreboard/GameDetail
- [ ] 18. Update `src/draw.rs`: dispatch to bracket/scoreboard/game_detail views; handle `is_final_four()` layout switch
- [ ] 19. Remove MLB-only components: `src/components/game/`, `linescore.rs`, `date_selector.rs`, `stats.rs`, `standings.rs`; remove `src/ui/` MLB views

## Batch 8 — Verification

- [ ] 20. `cargo check` — fix all compile errors
- [ ] 21. `cargo clippy` — address warnings
- [ ] 22. Update/replace existing tests for new domain types; verify ESPN API calls return parseable JSON
- [ ] 23. Manual smoke test: launch app, confirm banner animates, bracket loads, navigation works

---

## Technical Decisions (from design session)
- ESPN public API (no auth): site.api.espn.com
- Power-of-2 row spacing for bracket grid (pre-computed, not recursive)
- Triangle-wave offset (`tick % 8`) for basketball bounce in banner
- `BannerColor` semantic roles → `ratatui::Style` (dark/light theme)
- `BracketUpdated` carries only changed games (merged into tree, not full replace)
- FirstFour: `RoundKind::FirstFour` with y-offset from clean 2^n alignment
- Final Four uses hardcoded `FinalFourLayout`, not `BracketGrid`
