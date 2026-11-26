use super::{format_file_record, format_stdout_record, FilterConfig, LogEntry};
use env_filter::Filter;
use log::{Level, LevelFilter, Record};

fn record(target: &'static str, level: Level) -> Record<'static> {
    Record::builder()
        .args(format_args!("test"))
        .level(level)
        .target(target)
        .build()
}

fn filter(rust_log: Option<&str>, calyx_log: Option<&str>) -> Filter {
    FilterConfig::build(&[
        ("RUST_LOG", rust_log.map(String::from)),
        ("CALYX_LOG", calyx_log.map(String::from)),
    ])
    .filter
}

/// Asserts the engine, editor and sandbox targets log `info` and above.
fn assert_calyx_targets_at_info(filter: &Filter) {
    for target in [
        "calyx_engine::runtime::player",
        "calyx_editor::remote",
        "sandbox::hud",
    ] {
        for level in [Level::Error, Level::Warn, Level::Info] {
            assert!(
                filter.matches(&record(target, level)),
                "{target} {level} is filtered out"
            );
        }
        assert!(!filter.matches(&record(target, Level::Debug)));
    }
}

#[test]
fn default_filter_includes_calyx_targets_only() {
    let filter = filter(None, None);

    assert_eq!(filter.filter(), LevelFilter::Info);
    assert_calyx_targets_at_info(&filter);
    assert!(!filter.matches(&record("wgpu_core", Level::Info)));
}

#[test]
fn empty_variables_keep_the_defaults() {
    let filter = filter(Some(""), Some("  "));

    assert_calyx_targets_at_info(&filter);
}

#[test]
fn a_bare_rust_log_level_leaves_calyx_targets_at_their_level() {
    // This machine's user environment, set for other tools.
    let filter = filter(
        Some("off,bitwatch=trace,engine=trace,sandbox=trace,editor=trace"),
        None,
    );

    for target in ["calyx_engine::runtime::player", "calyx_editor::remote"] {
        for level in [Level::Error, Level::Warn, Level::Info] {
            assert!(filter.matches(&record(target, level)));
        }
        assert!(!filter.matches(&record(target, Level::Debug)));
    }
    assert!(filter.matches(&record("sandbox::hud", Level::Trace)));
    assert!(filter.matches(&record("bitwatch", Level::Trace)));
    assert!(!filter.matches(&record("wgpu_core", Level::Error)));
}

#[test]
fn rust_log_target_directives_change_only_the_targets_they_name() {
    let filter = filter(Some("calyx_engine=trace,wgpu_core=warn"), None);

    assert_eq!(filter.filter(), LevelFilter::Trace);
    assert!(filter.matches(&record("calyx_engine::assets", Level::Trace)));
    assert!(filter.matches(&record("calyx_editor", Level::Info)));
    assert!(!filter.matches(&record("calyx_editor", Level::Debug)));
    assert!(filter.matches(&record("wgpu_core", Level::Warn)));
    assert!(!filter.matches(&record("wgpu_core", Level::Info)));
}

#[test]
fn calyx_log_takes_precedence_over_rust_log() {
    let filter = filter(
        Some("calyx_engine=trace,wgpu_core=warn"),
        Some("calyx_engine=warn,calyx_editor=debug"),
    );

    assert!(!filter.matches(&record("calyx_engine::assets", Level::Info)));
    assert!(filter.matches(&record("calyx_engine::assets", Level::Warn)));
    assert!(filter.matches(&record("calyx_editor::remote", Level::Debug)));
    assert!(filter.matches(&record("sandbox::hud", Level::Info)));
    assert!(filter.matches(&record("wgpu_core", Level::Warn)));
}

#[test]
fn naming_a_calyx_target_can_silence_it() {
    let filter = filter(None, Some("calyx_engine=off"));

    assert!(!filter.matches(&record("calyx_engine::assets", Level::Error)));
    assert!(filter.matches(&record("calyx_editor", Level::Info)));
}

#[test]
fn an_invalid_variable_is_ignored_and_reported() {
    let config = FilterConfig::build(&[
        ("RUST_LOG", Some("calyx_engine=loud".to_string())),
        ("CALYX_LOG", Some("calyx_editor=debug".to_string())),
    ]);

    assert_eq!(
        config.rejected,
        [("RUST_LOG", "calyx_engine=loud".to_string())]
    );
    assert_eq!(
        config.applied,
        [("CALYX_LOG", "calyx_editor=debug".to_string())]
    );
    assert!(config
        .filter
        .matches(&record("calyx_engine::assets", Level::Info)));
    assert!(config
        .filter
        .matches(&record("calyx_editor::remote", Level::Debug)));
}

#[test]
fn stdout_record_format_includes_level_color() {
    let line = format_stdout_record(&record("calyx_engine::assets", Level::Warn));

    assert!(line.contains("\x1b[33mWARN "));
    assert!(line.contains("\x1b[0m calyx_engine::assets - test"));
}

#[test]
fn file_record_format_omits_level_color() {
    let line = format_file_record(&record("calyx_engine::assets", Level::Warn));

    assert!(!line.contains("\x1b["));
    assert!(line.contains("WARN  calyx_engine::assets - test"));
}

#[test]
fn log_entry_line_uses_plain_console_format() {
    let entry = LogEntry {
        timestamp: "2026-05-13 00:00:00.000".to_string(),
        level: Level::Info,
        target: "calyx_editor".to_string(),
        message: "ready".to_string(),
    };

    assert_eq!(
        entry.line(),
        "2026-05-13 00:00:00.000 INFO  calyx_editor - ready"
    );
}
