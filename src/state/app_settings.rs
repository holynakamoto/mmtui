use log::LevelFilter;

#[derive(Debug, Default, Clone)]
pub struct AppSettings {
    pub full_screen: bool,
    pub log_level: Option<LevelFilter>,
}

impl AppSettings {
    pub fn load() -> Self {
        // Simple defaults — log level can be overridden via env var RUST_LOG in the future.
        Self { full_screen: false, log_level: None }
    }
}
