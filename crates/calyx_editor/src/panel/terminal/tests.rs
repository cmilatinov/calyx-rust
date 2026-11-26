use super::PanelTerminal;
use log::{Level, LevelFilter};

#[test]
fn level_filter_includes_more_severe_entries() {
    assert!(PanelTerminal::passes_level(Level::Error, LevelFilter::Warn));
    assert!(PanelTerminal::passes_level(Level::Warn, LevelFilter::Warn));
    assert!(!PanelTerminal::passes_level(Level::Info, LevelFilter::Warn));
}
