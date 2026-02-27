use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use bitcoin::key::PublicKey;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CustodianEntry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustodianEntry {
    pub label: String,
    pub pubkey: String,
}

impl CustodianEntry {
    pub fn new(label: &str, pubkey: &str) -> Result<Self, String> {
        PublicKey::from_str(pubkey).map_err(|_| {
            "Invalid pubkey — must be a 66-char compressed hex (02/03 prefix)".to_string()
        })?;
        Ok(Self {
            label: label.to_string(),
            pubkey: pubkey.to_string(),
        })
    }

    pub fn display_pubkey(&self) -> String {
        if self.pubkey.len() < 16 {
            return self.pubkey.clone();
        }
        let prefix = &self.pubkey[..10];
        let suffix = &self.pubkey[self.pubkey.len() - 6..];
        format!("{}...{}", prefix, suffix)
    }
}

// ---------------------------------------------------------------------------
// CustodianConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CustodianConfig {
    pub custodians: Vec<CustodianEntry>,
}

impl CustodianConfig {
    pub fn load_from_path(path: &PathBuf) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse custodian config: {}", e))
    }

    pub fn save_to_path(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, json)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }
}

// ---------------------------------------------------------------------------
// BIP67 sort helper
// ---------------------------------------------------------------------------

pub fn bip67_sort(entries: &mut Vec<CustodianEntry>) {
    entries.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
}

// ---------------------------------------------------------------------------
// Threshold helper
// ---------------------------------------------------------------------------

pub fn compute_threshold(n: usize) -> usize {
    (n / 2 + 1).max(2)
}

// ---------------------------------------------------------------------------
// WizardStep
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Clone)]
pub enum WizardStep {
    #[default]
    Review,
    EnterLabel,
    EnterPubkey,
    ConfirmDiscard,
}

// ---------------------------------------------------------------------------
// CustodianWizardState
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct CustodianWizardState {
    pub active: bool,
    pub step: WizardStep,
    pub entries: Vec<CustodianEntry>,
    pub selected: usize,
    pub input: String,
    pub label_buf: String,
    pub error: Option<String>,
    pub dirty: bool,
}

impl CustodianWizardState {
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

    pub fn cursor_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    pub fn cursor_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn delete_selected(&mut self) {
        if self.selected < self.entries.len() {
            self.entries.remove(self.selected);
            self.dirty = true;
            if self.selected > 0 && self.selected >= self.entries.len() {
                self.selected = self.entries.len().saturating_sub(1);
            }
        }
    }

    pub fn commit_pubkey(&mut self) -> Result<(), String> {
        match CustodianEntry::new(&self.label_buf, self.input.trim()) {
            Ok(entry) => {
                self.entries.push(entry);
                self.dirty = true;
                self.input.clear();
                self.label_buf.clear();
                self.error = None;
                self.step = WizardStep::Review;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    pub fn discard(&mut self) {
        *self = Self::default();
    }

    pub fn status_text(&self) -> &'static str {
        if self.can_finalize() {
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
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".config");
                p
            })
        })
        .unwrap_or_else(|| PathBuf::from("custodians.json").parent().unwrap_or(std::path::Path::new(".")).to_path_buf());

    base.join("mmtui").join("custodians.json")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            custodians: vec![CustodianEntry {
                label: "Alice".to_string(),
                pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            }],
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: CustodianConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.custodians[0].label, "Alice");
    }

    #[test]
    fn test_bip67_sort() {
        let mut entries = vec![
            CustodianEntry {
                label: "Bob".to_string(),
                pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
            CustodianEntry {
                label: "Alice".to_string(),
                pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
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
        let existing = vec![CustodianEntry {
            label: "Alice".to_string(),
            pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                .to_string(),
        }];
        let wiz = CustodianWizardState::open(existing.clone());
        assert_eq!(wiz.entries.len(), 1);
        assert_eq!(wiz.entries[0].label, "Alice");
        assert!(!wiz.dirty);
    }

    #[test]
    fn test_wizard_delete_marks_dirty() {
        let existing = vec![
            CustodianEntry {
                label: "Alice".to_string(),
                pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
            CustodianEntry {
                label: "Bob".to_string(),
                pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
        ];
        let mut wiz = CustodianWizardState::open(existing);
        wiz.selected = 0;
        wiz.delete_selected();
        assert_eq!(wiz.entries.len(), 1);
        assert!(wiz.dirty);
    }

    #[test]
    fn test_wizard_cannot_finalize_with_one_entry() {
        let existing = vec![CustodianEntry {
            label: "Alice".to_string(),
            pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                .to_string(),
        }];
        let wiz = CustodianWizardState::open(existing);
        assert!(!wiz.can_finalize());
    }

    #[test]
    fn test_wizard_can_finalize_with_two_entries() {
        let existing = vec![
            CustodianEntry {
                label: "Alice".to_string(),
                pubkey: "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
            CustodianEntry {
                label: "Bob".to_string(),
                pubkey: "03c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
                    .to_string(),
            },
        ];
        let wiz = CustodianWizardState::open(existing);
        assert!(wiz.can_finalize());
    }
}
