# Custodian Wizard Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an in-TUI wizard that lets users add/remove Bitcoin custodians (label + pubkey) for the multisig prize pool without leaving the app, with persistence to `~/.config/mmtui/custodians.json`.

**Architecture:** New `src/state/custodian.rs` owns all custodian types (entry, config, wizard state machine). The wizard renders as an overlay on the Prize Pool tab, intercepted in `keys.rs` before regular dispatch (same pattern as Chat composing). On finalize, pubkeys are BIP67-sorted, multisig address recomputed, and config written to disk.

**Tech Stack:** Rust, ratatui 0.30, bitcoin 0.32 (PublicKey + Address), serde_json for persistence.

**Decision Log:**
- Linear step wizard (Option A) over inline table editor — matches PickWizard pattern, simplest state machine
- Load existing custodians on open — prevents re-entry of existing keys
- Dirty-state double-Esc — protects against accidental loss without nagging
- Working copy in wizard entries — original untouched until finalize
- BIP67 lexicographic pubkey sort on finalize — deterministic address regardless of add order
- Fallback chain on load: `custodians.json` → `MMTUI_PRIZE_POOL_KEYS` env var → fake keys

---

## Task 1: `src/state/custodian.rs` — Types, Persistence, and Tests

**Files:**
- Create: `src/state/custodian.rs`

### Step 1: Write failing tests

Add at the bottom of the new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_valid_pubkey() {
        let e = CustodianEntry::new(
            "Alice",
            "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
        );
        assert!(e.is_ok());
    }

    #[test]
    fn test_entry_invalid_pubkey_rejected() {
        let e = CustodianEntry::new("Bob", "notahex");
        assert!(e.is_err());
    }

    #[test]
    fn test_entry_short_hex_rejected() {
        let e = CustodianEntry::new("Carol", "02abcd");
        assert!(e.is_err());
    }

    #[test]
    fn test_config_roundtrip() {
        let config = CustodianConfig {
            custodians: vec![
                CustodianEntry {
                    label: "Alice".to_string(),
                    pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: CustodianConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.custodians[0].label, "Alice");
    }

    #[test]
    fn test_bip67_sort() {
        let mut entries = vec![
            CustodianEntry { label: "Bob".to_string(), pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
            CustodianEntry { label: "Alice".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
        ];
        bip67_sort(&mut entries);
        assert_eq!(entries[0].label, "Alice"); // 02... < 03...
    }

    #[test]
    fn test_threshold_calculation() {
        assert_eq!(compute_threshold(2), 2);
        assert_eq!(compute_threshold(3), 2);
        assert_eq!(compute_threshold(4), 3);
        assert_eq!(compute_threshold(5), 3);
    }

    #[test]
    fn test_wizard_open_loads_existing() {
        let existing = vec![CustodianEntry { label: "Alice".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() }];
        let wiz = CustodianWizardState::open(existing.clone());
        assert_eq!(wiz.entries.len(), 1);
        assert_eq!(wiz.entries[0].label, "Alice");
        assert!(!wiz.dirty);
    }

    #[test]
    fn test_wizard_delete_marks_dirty() {
        let existing = vec![
            CustodianEntry { label: "Alice".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
            CustodianEntry { label: "Bob".to_string(), pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
        ];
        let mut wiz = CustodianWizardState::open(existing);
        wiz.selected = 0;
        wiz.delete_selected();
        assert_eq!(wiz.entries.len(), 1);
        assert!(wiz.dirty);
    }

    #[test]
    fn test_wizard_cannot_finalize_with_one_entry() {
        let existing = vec![CustodianEntry { label: "Alice".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() }];
        let wiz = CustodianWizardState::open(existing);
        assert!(!wiz.can_finalize());
    }

    #[test]
    fn test_wizard_can_finalize_with_two_entries() {
        let existing = vec![
            CustodianEntry { label: "Alice".to_string(), pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
            CustodianEntry { label: "Bob".to_string(), pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".to_string() },
        ];
        let wiz = CustodianWizardState::open(existing);
        assert!(wiz.can_finalize());
    }
}
```

### Step 2: Run to verify tests fail

```bash
cargo test custodian 2>&1 | head -20
```

Expected: compile error — `custodian` module doesn't exist yet.

### Step 3: Write the implementation

Create `src/state/custodian.rs` with the full content:

```rust
use bitcoin::key::PublicKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Persisted types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustodianEntry {
    pub label: String,
    pub pubkey: String, // full 66-char compressed hex
}

impl CustodianEntry {
    /// Validate and construct. Returns Err with a human-readable message.
    pub fn new(label: &str, pubkey: &str) -> Result<Self, String> {
        PublicKey::from_str(pubkey)
            .map_err(|_| "Invalid pubkey — must be a 66-char compressed hex (02/03 prefix)".to_string())?;
        Ok(Self {
            label: label.to_string(),
            pubkey: pubkey.to_string(),
        })
    }

    /// Returns the first 10 chars + "..." + last 6 chars for display.
    pub fn display_pubkey(&self) -> String {
        if self.pubkey.len() >= 16 {
            format!("{}...{}", &self.pubkey[..10], &self.pubkey[self.pubkey.len() - 6..])
        } else {
            self.pubkey.clone()
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CustodianConfig {
    pub custodians: Vec<CustodianEntry>,
}

impl CustodianConfig {
    pub fn load_from_path(path: &PathBuf) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("read custodians.json: {e}"))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("parse custodians.json: {e}"))
    }

    pub fn save_to_path(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create config dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize custodians: {e}"))?;
        std::fs::write(path, json)
            .map_err(|e| format!("write custodians.json: {e}"))
    }
}

// ---------------------------------------------------------------------------
// BIP67 lexicographic sort + threshold
// ---------------------------------------------------------------------------

/// Sort entries by pubkey hex string (BIP67) to ensure deterministic address.
pub fn bip67_sort(entries: &mut Vec<CustodianEntry>) {
    entries.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
}

/// floor(n/2) + 1, minimum 2.
pub fn compute_threshold(n: usize) -> usize {
    (n / 2 + 1).max(2)
}

// ---------------------------------------------------------------------------
// Wizard state machine
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Clone)]
pub enum WizardStep {
    #[default]
    Review,
    EnterLabel,
    EnterPubkey,
    ConfirmDiscard, // dirty state + first Esc
}

#[derive(Debug, Default)]
pub struct CustodianWizardState {
    pub active: bool,
    pub step: WizardStep,
    pub entries: Vec<CustodianEntry>, // working copy
    pub selected: usize,
    pub input: String,      // current text field value
    pub label_buf: String,  // label saved while entering pubkey
    pub error: Option<String>,
    pub dirty: bool,
}

impl CustodianWizardState {
    /// Open the wizard with a clone of the currently active custodians.
    pub fn open(existing: Vec<CustodianEntry>) -> Self {
        Self {
            active: true,
            step: WizardStep::Review,
            entries: existing,
            selected: 0,
            input: String::new(),
            label_buf: String::new(),
            error: None,
            dirty: false,
        }
    }

    pub fn can_finalize(&self) -> bool {
        self.entries.len() >= 2
    }

    /// Move cursor down in Review list.
    pub fn cursor_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    /// Move cursor up in Review list.
    pub fn cursor_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Delete the currently selected entry.
    pub fn delete_selected(&mut self) {
        if self.selected < self.entries.len() {
            self.entries.remove(self.selected);
            self.dirty = true;
            if self.selected > 0 && self.selected >= self.entries.len() {
                self.selected = self.entries.len().saturating_sub(1);
            }
        }
    }

    /// Attempt to commit the current EnterPubkey input. Returns Err with display message on invalid.
    pub fn commit_pubkey(&mut self) -> Result<(), String> {
        let result = CustodianEntry::new(&self.label_buf, self.input.trim());
        match result {
            Ok(entry) => {
                self.entries.push(entry);
                self.dirty = true;
                self.input.clear();
                self.label_buf.clear();
                self.error = None;
                self.step = WizardStep::Review;
                Ok(())
            }
            Err(msg) => {
                self.error = Some(msg);
                Err("invalid".to_string())
            }
        }
    }

    /// Close the wizard, discarding all working-copy changes.
    pub fn discard(&mut self) {
        *self = Self::default();
    }

    /// Status line text for the Review summary panel.
    pub fn status_text(&self) -> &'static str {
        if self.entries.len() >= 2 {
            "Ready to finalize"
        } else {
            "Need at least 2"
        }
    }
}

// ---------------------------------------------------------------------------
// Config path helper
// ---------------------------------------------------------------------------

pub fn custodian_config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir).join("mmtui").join("custodians.json");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".config")
            .join("mmtui")
            .join("custodians.json");
    }
    PathBuf::from("custodians.json")
}

// Tests at bottom — see Step 1 above
```

Paste the test block from Step 1 at the bottom.

### Step 4: Register the module in `src/state/mod.rs`

Add one line:

```rust
pub mod custodian;
```

### Step 5: Run tests

```bash
cargo test custodian 2>&1
```

Expected: all 10 tests pass.

### Step 6: Commit

```bash
git add src/state/custodian.rs src/state/mod.rs
git commit -m "feat: add CustodianEntry, CustodianConfig, CustodianWizardState types"
```

---

## Task 2: Wire Types into `AppState` and `PrizePoolState`

**Files:**
- Modify: `src/state/app_state.rs`

### Step 1: Update the imports at top of `app_state.rs`

Add after line 1 (`use crate::app::MenuItem;`):

```rust
use crate::state::custodian::{CustodianEntry, CustodianWizardState};
```

### Step 2: Change `PrizePoolState.custodians` type

Find:
```rust
pub custodians: Vec<String>,
```

Replace with:
```rust
pub custodians: Vec<CustodianEntry>,
```

### Step 3: Add `custodian_wizard` field to `AppState`

Find the `pub struct AppState {` block. Add after `pub prize_pool: PrizePoolState,`:

```rust
pub custodian_wizard: CustodianWizardState,
```

### Step 4: Verify compilation

```bash
cargo check 2>&1 | grep "^error" | head -20
```

Expected: errors only in `app.rs` and `draw.rs` where `custodians: Vec<String>` was used — we fix those in the next tasks.

### Step 5: Commit

```bash
git add src/state/app_state.rs
git commit -m "feat: wire CustodianEntry and CustodianWizardState into AppState"
```

---

## Task 3: Update `app.rs` — Prize Pool Setup and Wizard Methods

**Files:**
- Modify: `src/app.rs`

### Step 1: Update imports at top of `app.rs`

Add to existing use block:

```rust
use crate::state::custodian::{
    CustodianConfig, CustodianEntry, CustodianWizardState,
    bip67_sort, compute_threshold, custodian_config_path,
};
```

### Step 2: Replace `setup_prize_pool`

Find the entire `pub fn setup_prize_pool(&mut self)` function and replace it:

```rust
pub fn setup_prize_pool(&mut self) {
    let entries = self.load_custodian_entries();
    self.apply_custodian_entries(entries);
}

/// Load custodian entries: file → env var → fake placeholders.
fn load_custodian_entries(&self) -> Vec<CustodianEntry> {
    // 1. Try custodians.json
    let path = custodian_config_path();
    if let Ok(config) = CustodianConfig::load_from_path(&path) {
        if config.custodians.len() >= 2 {
            return config.custodians;
        }
    }

    // 2. Try env var
    if let Ok(keys_raw) = std::env::var("MMTUI_PRIZE_POOL_KEYS") {
        let entries: Vec<CustodianEntry> = keys_raw
            .split(',')
            .enumerate()
            .filter_map(|(i, s)| {
                CustodianEntry::new(&format!("Custodian {}", i + 1), s.trim()).ok()
            })
            .collect();
        if entries.len() >= 2 {
            return entries;
        }
    }

    // 3. Fake placeholders (clearly fake keys — won't parse as real PublicKey)
    // We use known-valid test vectors from secp256k1 so the address generates
    vec![
        CustodianEntry { label: "Custodian A (placeholder)".to_string(), pubkey: "022222222222222222222222222222222222222222222222222222222222222222".to_string() },
        CustodianEntry { label: "Custodian B (placeholder)".to_string(), pubkey: "033333333333333333333333333333333333333333333333333333333333333333".to_string() },
        CustodianEntry { label: "Custodian C (placeholder)".to_string(), pubkey: "024444444444444444444444444444444444444444444444444444444444444444".to_string() },
    ]
}

/// Build multisig script and address from entries, update prize_pool state.
pub fn apply_custodian_entries(&mut self, mut entries: Vec<CustodianEntry>) {
    bip67_sort(&mut entries);

    let keys: Vec<_> = entries
        .iter()
        .filter_map(|e| PublicKey::from_str(&e.pubkey).ok())
        .collect();

    if keys.len() < 2 {
        self.state.last_error = Some("Prize Pool: need at least 2 valid keys".to_string());
        return;
    }

    let threshold = compute_threshold(keys.len());
    let mut builder = Builder::new().push_int(threshold as i64);
    for key in &keys {
        builder = builder.push_key(key);
    }
    builder = builder
        .push_int(keys.len() as i64)
        .push_opcode(opcodes::all::OP_CHECKMULTISIG);

    let script = builder.into_script();
    let address = Address::p2wsh(&script, Network::Bitcoin);

    self.state.prize_pool.address = address.to_string();
    self.state.prize_pool.custodians = entries;
    self.state.prize_pool.threshold = threshold;
}
```

### Step 3: Add wizard open/finalize methods

After `apply_custodian_entries`, add:

```rust
pub fn open_custodian_wizard(&mut self) {
    let existing = self.state.prize_pool.custodians.clone();
    self.state.custodian_wizard = CustodianWizardState::open(existing);
}

pub fn finalize_custodian_wizard(&mut self) {
    let mut entries = self.state.custodian_wizard.entries.clone();
    bip67_sort(&mut entries);

    let config = CustodianConfig { custodians: entries.clone() };
    let path = custodian_config_path();
    if let Err(e) = config.save_to_path(&path) {
        self.state.last_error = Some(format!("Save failed: {e}"));
        return;
    }

    self.apply_custodian_entries(entries);
    self.state.custodian_wizard.discard();
}
```

### Step 4: Verify compilation

```bash
cargo check 2>&1 | grep "^error" | head -20
```

Expected: errors only in `draw.rs` (still uses old `Vec<String>`).

### Step 5: Commit

```bash
git add src/app.rs
git commit -m "feat: update prize pool setup to load from file, add wizard open/finalize"
```

---

## Task 4: Key Handling in `keys.rs`

**Files:**
- Modify: `src/keys.rs`

### Step 1: Add wizard-active guard block

The wizard intercepts all keys when active, just like the Chat composing block. Insert this block **after** the `show_intro` block (after line ~28) and **before** the `chat.composing` block:

```rust
// Custodian wizard intercepts all keys when active
if guard.state.custodian_wizard.active {
    use crate::state::custodian::WizardStep;
    let wiz = &mut guard.state.custodian_wizard;

    match &wiz.step.clone() {
        WizardStep::Review => match key_event.code {
            Char('a') => {
                wiz.input.clear();
                wiz.error = None;
                wiz.step = WizardStep::EnterLabel;
            }
            Char('d') => wiz.delete_selected(),
            KeyCode::Down | Char('j') => wiz.cursor_down(),
            KeyCode::Up | Char('k') => wiz.cursor_up(),
            KeyCode::Enter => {
                if wiz.can_finalize() {
                    drop(wiz);
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
                    wiz.label_buf = trimmed;
                    wiz.input.clear();
                    wiz.error = None;
                    wiz.step = WizardStep::EnterPubkey;
                }
            }
            KeyCode::Esc => {
                wiz.input.clear();
                wiz.error = None;
                wiz.step = WizardStep::Review;
            }
            KeyCode::Backspace => { wiz.input.pop(); }
            Char(ch) if key_event.modifiers == KeyModifiers::NONE
                     || key_event.modifiers == KeyModifiers::SHIFT => {
                wiz.input.push(ch);
            }
            _ => {}
        },

        WizardStep::EnterPubkey => match key_event.code {
            KeyCode::Enter => {
                let _ = wiz.commit_pubkey(); // sets error on failure, stays in step
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
            Char(ch) if key_event.modifiers == KeyModifiers::NONE
                     || key_event.modifiers == KeyModifiers::SHIFT => {
                wiz.input.push(ch);
                wiz.error = None;
            }
            _ => {}
        },

        WizardStep::ConfirmDiscard => match key_event.code {
            KeyCode::Esc => wiz.discard(),
            _ => {
                wiz.step = WizardStep::Review;
            }
        },
    }
    return;
}
```

### Step 2: Add `e` key handler for Prize Pool tab

In the main `match (guard.state.active_tab, key_event.code, key_event.modifiers)` block, find the `// Prize Pool` section and add after the existing `r` handler:

```rust
(MenuItem::PrizePool, Char('e'), _) => guard.open_custodian_wizard(),
```

### Step 3: Add missing import for `WizardStep` (if needed)

The `WizardStep` is accessed via the `state::custodian` path — the `use` inside the block handles it. Verify `cargo check` passes.

### Step 4: Verify compilation

```bash
cargo check 2>&1 | grep "^error" | head -20
```

Expected: only `draw.rs` errors remain.

### Step 5: Commit

```bash
git add src/keys.rs
git commit -m "feat: add custodian wizard key handler, e=open wizard on prize pool tab"
```

---

## Task 5: Update `draw.rs` — Prize Pool and Wizard Overlay

**Files:**
- Modify: `src/draw.rs`

### Step 1: Update `draw_prize_pool` to use `CustodianEntry`

Find the `draw_prize_pool` function (~line 964). Replace the custodians loop and the surrounding threshold label:

Old:
```rust
lines.push(Line::from(Span::styled("Custodians (2-of-3 Multisig):", Style::default().fg(Color::Gray))));
for custodian in &state.custodians {
    lines.push(Line::from(format!(" • {}", custodian)));
}

lines.push(Line::from(""));
lines.push(Line::from(Span::styled("Keys: r=refresh balance", Style::default().fg(Color::DarkGray))));
```

New:
```rust
let n = state.custodians.len();
let threshold = state.threshold;
lines.push(Line::from(Span::styled(
    format!("Custodians ({}-of-{} Multisig):", threshold, n),
    Style::default().fg(Color::Gray),
)));
for entry in &state.custodians {
    lines.push(Line::from(format!(" • {}  {}", entry.label, entry.display_pubkey())));
}

lines.push(Line::from(""));
lines.push(Line::from(Span::styled(
    "Keys: r=refresh balance  e=edit custodians",
    Style::default().fg(Color::DarkGray),
)));
```

### Step 2: Add wizard overlay draw function

Add `draw_custodian_wizard` after `draw_prize_pool`. Full implementation:

```rust
pub fn draw_custodian_wizard(f: &mut Frame, app: &App) {
    use crate::state::custodian::{WizardStep, compute_threshold};

    let wiz = &app.state.custodian_wizard;
    if !wiz.active {
        return;
    }

    // Center a 70×22 overlay
    let area = f.area();
    let w = 72u16.min(area.width.saturating_sub(4));
    let h = 24u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect { x, y, width: w, height: h };

    f.render_widget(Clear, popup);

    let title = match &wiz.step {
        WizardStep::ConfirmDiscard => " Prize Pool — Unsaved Changes ",
        _ => " Prize Pool — Custodian Setup ",
    };
    let block = default_border(Color::Yellow).title(title);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Split inner into left list and right context panel
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(inner);

    // --- Left: Custodian list ---
    let mut list_lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!("Custodians ({} added)", wiz.entries.len()),
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled("─".repeat(cols[0].width as usize), Style::default().fg(Color::DarkGray))),
    ];

    if wiz.entries.is_empty() {
        list_lines.push(Line::from(Span::styled(
            " (no custodians yet)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, entry) in wiz.entries.iter().enumerate() {
            let cursor = if i == wiz.selected && wiz.step == WizardStep::Review { "▶ " } else { "  " };
            let style = if i == wiz.selected && wiz.step == WizardStep::Review {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            list_lines.push(Line::from(Span::styled(
                format!("{}{}.  {}   {}", cursor, i + 1, entry.label, entry.display_pubkey()),
                style,
            )));
        }
    }

    // Footer hints
    list_lines.push(Line::from(""));
    let hint = match &wiz.step {
        WizardStep::Review => {
            if wiz.can_finalize() {
                "a=add  d=del  ↑↓=nav  Enter=save  Esc=cancel"
            } else {
                "a=add  d=del  ↑↓=nav  (need ≥2 to save)  Esc=cancel"
            }
        }
        WizardStep::EnterLabel | WizardStep::EnterPubkey => "Enter=confirm  Esc=back",
        WizardStep::ConfirmDiscard => "Esc=discard changes  any key=keep editing",
    };
    list_lines.push(Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))));

    f.render_widget(Paragraph::new(list_lines), cols[0]);

    // --- Right: Context panel ---
    let mut ctx_lines: Vec<Line> = Vec::new();

    match &wiz.step {
        WizardStep::Review => {
            let n = wiz.entries.len();
            let threshold = if n >= 2 { compute_threshold(n) } else { 0 };
            ctx_lines.push(Line::from(Span::styled("Summary", Style::default().fg(Color::Gray))));
            ctx_lines.push(Line::from(Span::styled("─".repeat(cols[1].width as usize), Style::default().fg(Color::DarkGray))));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(vec![
                Span::styled("Threshold:  ", Style::default().fg(Color::Gray)),
                Span::styled(
                    if n >= 2 { format!("{}-of-{}", threshold, n) } else { "—".to_string() },
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));
            ctx_lines.push(Line::from(vec![
                Span::styled("Min needed: ", Style::default().fg(Color::Gray)),
                Span::styled("2", Style::default().fg(Color::White)),
                Span::raw("  "),
                if n >= 2 {
                    Span::styled("✓", Style::default().fg(Color::Green))
                } else {
                    Span::styled("✗", Style::default().fg(Color::Red))
                },
            ]));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(vec![
                Span::styled("Status:     ", Style::default().fg(Color::Gray)),
                Span::styled(
                    wiz.status_text(),
                    if wiz.can_finalize() {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
            ]));
        }

        WizardStep::EnterLabel => {
            ctx_lines.push(Line::from(Span::styled("Enter Label", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            ctx_lines.push(Line::from(Span::styled("─".repeat(cols[1].width as usize), Style::default().fg(Color::DarkGray))));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(Span::styled("Name for this custodian:", Style::default().fg(Color::Gray))));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Yellow)),
                Span::styled(format!("{}_", wiz.input), Style::default().fg(Color::White)),
            ]));
        }

        WizardStep::EnterPubkey => {
            ctx_lines.push(Line::from(Span::styled("Enter Pubkey", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            ctx_lines.push(Line::from(Span::styled("─".repeat(cols[1].width as usize), Style::default().fg(Color::DarkGray))));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(Span::styled(
                format!("For: {}", wiz.label_buf),
                Style::default().fg(Color::Gray),
            )));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(Span::styled(
                "66-char compressed hex (02/03...):",
                Style::default().fg(Color::Gray),
            )));
            ctx_lines.push(Line::from(""));
            // Show input broken into two lines if long
            let inp = &wiz.input;
            if inp.len() > 33 {
                ctx_lines.push(Line::from(Span::styled(&inp[..33], Style::default().fg(Color::White))));
                ctx_lines.push(Line::from(vec![
                    Span::styled(&inp[33..], Style::default().fg(Color::White)),
                    Span::styled("_", Style::default().fg(Color::Yellow)),
                ]));
            } else {
                ctx_lines.push(Line::from(vec![
                    Span::styled(inp.as_str(), Style::default().fg(Color::White)),
                    Span::styled("_", Style::default().fg(Color::Yellow)),
                ]));
            }
            if let Some(err) = &wiz.error {
                ctx_lines.push(Line::from(""));
                ctx_lines.push(Line::from(Span::styled(
                    format!("⚠ {}", err),
                    Style::default().fg(Color::Red),
                )));
            }
        }

        WizardStep::ConfirmDiscard => {
            ctx_lines.push(Line::from(Span::styled("Unsaved Changes", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))));
            ctx_lines.push(Line::from(""));
            ctx_lines.push(Line::from(Span::styled(
                "Press Esc again to discard,",
                Style::default().fg(Color::Yellow),
            )));
            ctx_lines.push(Line::from(Span::styled(
                "or any other key to keep editing.",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    f.render_widget(Paragraph::new(ctx_lines), cols[1]);
}
```

### Step 3: Wire the overlay into the main `draw` function

In the `pub fn draw<B>` function, after the `terminal.draw(|f| {` block, after the match that calls `draw_prize_pool`, add the wizard overlay call. It should render **on top of everything** — add it just before the `draw_loading_spinner` call:

```rust
// Custodian wizard overlay (renders on top of active tab)
if app.state.custodian_wizard.active {
    draw_custodian_wizard(f, app);
}
```

Also add the import for `CustodianEntry` at the top of `draw.rs`:

```rust
use crate::state::custodian::CustodianEntry;
```

(Needed because `PrizePoolState.custodians` is now `Vec<CustodianEntry>`.)

### Step 4: Build and run

```bash
cargo build 2>&1 | grep "^error" | head -20
```

Expected: clean build.

```bash
cargo run
```

Navigate to tab 7 (Prize Pool), press `e` to open the wizard. Verify the overlay appears. Add a custodian with a real pubkey:

```
Label: Alice
Pubkey: 02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5
```

Add a second. Press Enter to finalize. Verify the Prize Pool tab updates and `~/.config/mmtui/custodians.json` is created.

### Step 5: Run all tests

```bash
cargo test 2>&1
```

Expected: all tests pass.

### Step 6: Commit

```bash
git add src/draw.rs
git commit -m "feat: add custodian wizard overlay with split list/context layout"
```

---

## Task 6: Update Memory

**Files:**
- Create/Update: `memory/MEMORY.md`

After implementation, save the following to project memory:

```
## Custodian Wizard (Prize Pool Tab)
- Wizard state machine lives in `src/state/custodian.rs`
- Activated by `e` on Prize Pool tab (MenuItem::PrizePool)
- Key intercept guard in `keys.rs` before chat composing block
- Draw overlay in `draw.rs::draw_custodian_wizard()`, called after tab match in `draw()`
- Persists to `~/.config/mmtui/custodians.json` via `CustodianConfig`
- Fallback chain: JSON file → MMTUI_PRIZE_POOL_KEYS env → fake placeholders
- BIP67 sort on finalize (`bip67_sort()` in custodian.rs)
- Threshold: `compute_threshold(n) = floor(n/2)+1, min 2`
```

---

## Acceptance Criteria Checklist

- [ ] `cargo test` passes (all custodian unit tests green)
- [ ] `cargo build --release` succeeds with no warnings
- [ ] Press `7` → Prize Pool tab shows label + truncated pubkey per custodian
- [ ] Press `e` → wizard overlay appears over Prize Pool tab
- [ ] Can add custodian (label + valid pubkey) → appears in list with threshold updated
- [ ] Invalid pubkey shows inline error, stays in EnterPubkey step
- [ ] `d` on selected entry removes it, marks dirty
- [ ] `Esc` on clean wizard closes immediately (no prompt)
- [ ] `Esc` on dirty wizard → ConfirmDiscard → second `Esc` discards, other key resumes
- [ ] `Enter` with < 2 custodians is blocked (hint shown)
- [ ] `Enter` with ≥ 2 → writes `~/.config/mmtui/custodians.json`, updates address, closes wizard
- [ ] Relaunch app → custodians load from JSON file automatically
- [ ] `e` opens wizard pre-populated with existing custodians
