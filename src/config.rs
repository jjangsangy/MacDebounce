use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::Parser;

use crate::debounce::{ButtonSelection, MAX_BUTTONS, MouseButton};
use crate::logger::LogLevel;

#[derive(Parser, Debug)]
#[command(
    name = "macdebounce",
    author,
    version,
    about = "Minimal macOS mouse key debouncer with 0.0% idle CPU and launchctl support",
    long_about = "A lightweight background daemon to eliminate mouse chatter and accidental double clicks on macOS with 0.0% idle CPU usage."
)]
pub struct Cli {
    /// Debounce lockout window in milliseconds (default: 50)
    #[arg(short = 'd', long = "debounce-ms", value_name = "MS")]
    pub debounce_ms: Option<u64>,

    /// Buttons to debounce ("all", "left", "right", "middle", "back", "forward", "side", "left,right", "0,1,2")
    #[arg(short = 'b', long = "buttons", value_name = "BUTTONS")]
    pub buttons: Option<String>,

    /// Path to TOML configuration file
    #[arg(short = 'c', long = "config", value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Log debounced clicks to stdout and macOS Unified Logging
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Log EVERY mouse event (passed and debounced) with timestamps for in-depth debugging
    #[arg(long = "log-all")]
    pub log_all: bool,

    /// Custom log file path to write log entries to
    #[arg(long = "log-file", value_name = "PATH")]
    pub log_file: Option<PathBuf>,

    /// Disable logging to macOS Unified Logging System (syslog)
    #[arg(long = "no-syslog")]
    pub no_syslog: bool,

    /// Display recent log entries from the background daemon
    #[arg(long = "show-logs")]
    pub show_logs: bool,

    /// Stream live daemon logs in real time via macOS Unified Logging
    #[arg(long = "stream-logs")]
    pub stream_logs: bool,

    /// Display current accessibility status and launchd service state
    #[arg(long = "status")]
    pub status: bool,

    /// Install and load background LaunchAgent for current user
    #[arg(long = "install-launchd")]
    pub install_launchd: bool,

    /// Unload and remove the background LaunchAgent
    #[arg(long = "uninstall-launchd")]
    pub uninstall_launchd: bool,

    /// Print LaunchAgent plist XML to stdout and exit
    #[arg(long = "generate-plist")]
    pub generate_plist: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub debounce_ms: u64,
    pub selection: ButtonSelection,
    pub log_level: LogLevel,
    pub use_syslog: bool,
    pub log_file: Option<PathBuf>,
    pub config_file_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debounce_ms: 50,
            selection: ButtonSelection::All,
            log_level: LogLevel::Info,
            use_syslog: true,
            log_file: None,
            config_file_path: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliAction {
    Run(Config),
    InstallLaunchd(Config),
    UninstallLaunchd,
    GeneratePlist(Config),
    Status,
    ShowLogs,
    StreamLogs,
}

impl Config {
    /// Parse button string into ButtonSelection.
    /// Supports "all", "left", "right", "middle", numbers like "0,1,2", or "button3".
    pub fn parse_buttons(input: &str) -> Result<ButtonSelection, String> {
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("all") {
            return Ok(ButtonSelection::All);
        }

        let mut mask: u32 = 0;
        for part in trimmed.split(',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }

            match p.to_ascii_lowercase().as_str() {
                "left" | "l" => mask |= MouseButton::LEFT.mask(),
                "right" | "r" => mask |= MouseButton::RIGHT.mask(),
                "middle" | "m" => mask |= MouseButton::MIDDLE.mask(),
                "back" | "backward" | "backwards" => mask |= MouseButton::BACK.mask(),
                "forward" | "foreward" | "front" => mask |= MouseButton::FORWARD.mask(),
                "side" | "sides" | "sidebutton" | "sidebuttons" | "side-button"
                | "side-buttons" | "side_button" | "side_buttons" | "side button"
                | "side buttons" => {
                    mask |= MouseButton::BACK.mask() | MouseButton::FORWARD.mask();
                }
                s if s.starts_with("button") => {
                    let num_str = s["button".len()..].trim();
                    let btn_idx = num_str
                        .parse::<usize>()
                        .map_err(|_| format!("Invalid button format: '{p}'"))?;
                    if btn_idx >= MAX_BUTTONS {
                        return Err(format!(
                            "Button index {btn_idx} out of range (max is {})",
                            MAX_BUTTONS - 1
                        ));
                    }
                    mask |= MouseButton::new(btn_idx).mask();
                }
                s if s.starts_with("btn") => {
                    let num_str = s["btn".len()..].trim();
                    let btn_idx = num_str
                        .parse::<usize>()
                        .map_err(|_| format!("Invalid button format: '{p}'"))?;
                    if btn_idx >= MAX_BUTTONS {
                        return Err(format!(
                            "Button index {btn_idx} out of range (max is {})",
                            MAX_BUTTONS - 1
                        ));
                    }
                    mask |= MouseButton::new(btn_idx).mask();
                }
                s => {
                    let btn_idx = s
                        .parse::<usize>()
                        .map_err(|_| format!("Unknown button identifier: '{p}' (expected 'left', 'right', 'middle', 'back', 'forward', 'side', or button index 0-31)"))?;
                    if btn_idx >= MAX_BUTTONS {
                        return Err(format!(
                            "Button index {btn_idx} out of range (max is {})",
                            MAX_BUTTONS - 1
                        ));
                    }
                    mask |= MouseButton::new(btn_idx).mask();
                }
            }
        }

        if mask == 0 {
            Err("No valid buttons specified".to_string())
        } else {
            Ok(ButtonSelection::Specific(mask))
        }
    }

    /// Read config file from a given path or try default system locations.
    pub fn load_from_file_or_defaults(explicit_path: Option<&Path>) -> (Self, Option<PathBuf>) {
        if let Some(path) = explicit_path {
            match fs::read_to_string(path) {
                Ok(content) => match Self::parse_toml_str(&content) {
                    Ok(mut cfg) => {
                        cfg.config_file_path = Some(path.to_path_buf());
                        return (cfg, Some(path.to_path_buf()));
                    }
                    Err(err) => {
                        eprintln!(
                            "Warning: Failed to parse configuration file {}: {}",
                            path.display(),
                            err
                        );
                    }
                },
                Err(err) => {
                    eprintln!(
                        "Warning: Failed to read configuration file {}: {}",
                        path.display(),
                        err
                    );
                }
            }
            return (Self::default(), None);
        }

        // Search default locations:
        // 1. ~/.config/macdebounce/config.toml
        // 2. ~/Library/Application Support/MacDebounce/config.toml
        // 3. /Library/Application Support/MacDebounce/config.toml
        let mut candidates = Vec::new();
        if let Some(home) = env::var_os("HOME") {
            let home_path = PathBuf::from(home);
            candidates.push(home_path.join(".config/macdebounce/config.toml"));
            candidates.push(home_path.join("Library/Application Support/MacDebounce/config.toml"));
        }
        candidates.push(PathBuf::from(
            "/Library/Application Support/MacDebounce/config.toml",
        ));

        for candidate in candidates {
            if candidate.is_file() {
                match fs::read_to_string(&candidate) {
                    Ok(content) => match Self::parse_toml_str(&content) {
                        Ok(mut cfg) => {
                            cfg.config_file_path = Some(candidate.clone());
                            return (cfg, Some(candidate));
                        }
                        Err(err) => {
                            eprintln!(
                                "Warning: Failed to parse configuration file {}: {}",
                                candidate.display(),
                                err
                            );
                        }
                    },
                    Err(err) => {
                        eprintln!(
                            "Warning: Failed to read configuration file {}: {}",
                            candidate.display(),
                            err
                        );
                    }
                }
            }
        }

        (Self::default(), None)
    }

    /// Parse a simple TOML configuration file
    pub fn parse_toml_str(content: &str) -> Result<Self, String> {
        let mut cfg = Self::default();

        let mut lines_iter = content.lines().enumerate().peekable();
        while let Some((line_no, line)) = lines_iter.next() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            if let Some((raw_key, raw_val)) = trimmed.split_once('=') {
                let key = raw_key.trim().to_ascii_lowercase();
                let mut val = raw_val.trim().to_string();

                // Strip trailing comment if present
                if let Some((v, _)) = val.split_once('#') {
                    val = v.trim().to_string();
                }

                // If value starts with '[' but doesn't end with ']', read subsequent lines until ']'
                if val.starts_with('[') && !val.ends_with(']') {
                    while let Some((_, next_line)) = lines_iter.next() {
                        let next_trimmed = next_line.trim();
                        let clean_next = if let Some((v, _)) = next_trimmed.split_once('#') {
                            v.trim()
                        } else {
                            next_trimmed
                        };
                        val.push(' ');
                        val.push_str(clean_next);
                        if clean_next.contains(']') {
                            break;
                        }
                    }
                }

                match key.as_str() {
                    "debounce_ms" => {
                        let ms = val.parse::<u64>().map_err(|_| {
                            format!("Line {}: invalid debounce_ms value", line_no + 1)
                        })?;
                        cfg.debounce_ms = ms;
                    }
                    "buttons" => {
                        let val_str = val.trim();
                        if val_str.starts_with('[') && val_str.ends_with(']') {
                            let inner = &val_str[1..val_str.len() - 1];
                            let mut combined = String::new();
                            for item in inner.split(',') {
                                let item_clean = item.trim().trim_matches('"').trim_matches('\'');
                                if item_clean.is_empty() {
                                    continue;
                                }
                                if !combined.is_empty() {
                                    combined.push(',');
                                }
                                combined.push_str(item_clean);
                            }
                            cfg.selection = Self::parse_buttons(&combined)?;
                        } else {
                            let clean = val_str.trim_matches('"').trim_matches('\'');
                            cfg.selection = Self::parse_buttons(clean)?;
                        }
                    }
                    "verbose" => {
                        if val.parse::<bool>().unwrap_or(false) {
                            cfg.log_level = LogLevel::Debug;
                        }
                    }
                    "log_level" => {
                        let clean = val
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_ascii_lowercase();
                        cfg.log_level = match clean.as_str() {
                            "trace" | "all" => LogLevel::Trace,
                            "debug" | "verbose" => LogLevel::Debug,
                            "warn" | "warning" => LogLevel::Warn,
                            "error" => LogLevel::Error,
                            _ => LogLevel::Info,
                        };
                    }
                    "syslog" => {
                        cfg.use_syslog = val.parse::<bool>().unwrap_or(true);
                    }
                    "log_file" => {
                        let clean = val.trim_matches('"').trim_matches('\'');
                        if !clean.is_empty() {
                            cfg.log_file = Some(PathBuf::from(clean));
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(cfg)
    }

    /// Convert parsed CLI arguments into the appropriate CliAction
    pub fn process_cli(cli: Cli) -> Result<CliAction, String> {
        if cli.show_logs {
            return Ok(CliAction::ShowLogs);
        }

        if cli.stream_logs {
            return Ok(CliAction::StreamLogs);
        }

        if cli.uninstall_launchd {
            return Ok(CliAction::UninstallLaunchd);
        }

        if cli.status {
            return Ok(CliAction::Status);
        }

        // Load config from file or defaults
        let (mut config, found_path) = Self::load_from_file_or_defaults(cli.config.as_deref());
        if cli.config.is_some() {
            config.config_file_path = found_path;
        }

        let is_interactive = std::io::stdout().is_terminal();

        // When running as a background daemon (launchd non-interactive) and a config file
        // was loaded, settings from config.toml are the source of truth unless --config was explicitly given.
        // This prevents legacy hardcoded CLI flags in LaunchAgent plists (e.g. `--buttons 0,1`)
        // from permanently overriding the user's config file.
        let is_daemon_with_config = !is_interactive
            && config.config_file_path.is_some()
            && cli.config.is_none()
            && !cli.install_launchd
            && !cli.generate_plist;

        if is_daemon_with_config {
            if cli.buttons.is_some() || cli.debounce_ms.is_some() {
                crate::logger::log_info(&format!(
                    "Running as background daemon with config file ({}). Using config file settings and ignoring legacy launchd plist arguments.",
                    config.config_file_path.as_ref().unwrap().display()
                ));
            }
        } else {
            // Apply CLI overrides
            if let Some(ms) = cli.debounce_ms {
                config.debounce_ms = ms;
            }
            if let Some(ref btns) = cli.buttons {
                config.selection = Self::parse_buttons(btns)?;
            }
        }
        if cli.log_all {
            config.log_level = LogLevel::Trace;
        } else if cli.verbose {
            config.log_level = LogLevel::Debug;
        }
        if cli.no_syslog {
            config.use_syslog = false;
        }
        if let Some(path) = cli.log_file {
            config.log_file = Some(path);
        }

        if cli.generate_plist {
            return Ok(CliAction::GeneratePlist(config));
        }

        if cli.install_launchd {
            return Ok(CliAction::InstallLaunchd(config));
        }

        Ok(CliAction::Run(config))
    }

    /// Formats the selected buttons for display
    pub fn format_buttons_summary(&self) -> String {
        match self.selection {
            ButtonSelection::All => "all (Left, Right, Middle, Extra buttons)".to_string(),
            ButtonSelection::Specific(mask) => {
                let mut names = Vec::new();
                for i in 0..MAX_BUTTONS {
                    let btn = MouseButton::new(i);
                    if (mask & btn.mask()) != 0 {
                        let n = btn.name();
                        if n == "Other" {
                            names.push(format!("Button {i}"));
                        } else {
                            names.push(n.to_string());
                        }
                    }
                }
                if names.is_empty() {
                    "none".to_string()
                } else {
                    names.join(", ")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_buttons() {
        assert_eq!(Config::parse_buttons("all").unwrap(), ButtonSelection::All);
        assert_eq!(Config::parse_buttons("ALL").unwrap(), ButtonSelection::All);

        assert_eq!(
            Config::parse_buttons("left").unwrap(),
            ButtonSelection::Specific(1 << 0)
        );
        assert_eq!(
            Config::parse_buttons("right").unwrap(),
            ButtonSelection::Specific(1 << 1)
        );
        assert_eq!(
            Config::parse_buttons("middle").unwrap(),
            ButtonSelection::Specific(1 << 2)
        );
        assert_eq!(
            Config::parse_buttons("back").unwrap(),
            ButtonSelection::Specific(1 << 3)
        );
        assert_eq!(
            Config::parse_buttons("forward").unwrap(),
            ButtonSelection::Specific(1 << 4)
        );
        assert_eq!(
            Config::parse_buttons("foreward").unwrap(),
            ButtonSelection::Specific(1 << 4)
        );
        assert_eq!(
            Config::parse_buttons("side").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("left,back,forward").unwrap(),
            ButtonSelection::Specific((1 << 0) | (1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("left,right").unwrap(),
            ButtonSelection::Specific((1 << 0) | (1 << 1))
        );
        assert_eq!(
            Config::parse_buttons("0,1,2").unwrap(),
            ButtonSelection::Specific((1 << 0) | (1 << 1) | (1 << 2))
        );
        assert_eq!(
            Config::parse_buttons("button3, button4").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("btn3, btn4").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("side-buttons").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("side_button").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
        assert_eq!(
            Config::parse_buttons("side buttons").unwrap(),
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
            # MacDebounce config
            debounce_ms = 45
            buttons = ["left", "right"]
            verbose = true
        "#;
        let cfg = Config::parse_toml_str(toml_str).unwrap();
        assert_eq!(cfg.debounce_ms, 45);
        assert_eq!(
            cfg.selection,
            ButtonSelection::Specific((1 << 0) | (1 << 1))
        );
        assert_eq!(cfg.log_level, LogLevel::Debug);

        let toml_all = r#"
            debounce_ms = 60
            buttons = "all"
            log_level = "trace"
            syslog = false
        "#;
        let cfg_all = Config::parse_toml_str(toml_all).unwrap();
        assert_eq!(cfg_all.debounce_ms, 60);
        assert_eq!(cfg_all.selection, ButtonSelection::All);
        assert_eq!(cfg_all.log_level, LogLevel::Trace);
        assert_eq!(cfg_all.use_syslog, false);

        let toml_multiline = r#"
            debounce_ms = 80
            buttons = [
                "back",
                "forward",
            ]
        "#;
        let cfg_multiline = Config::parse_toml_str(toml_multiline).unwrap();
        assert_eq!(cfg_multiline.debounce_ms, 80);
        assert_eq!(
            cfg_multiline.selection,
            ButtonSelection::Specific((1 << 3) | (1 << 4))
        );
    }

    #[test]
    fn test_clap_parsing() {
        let cli = Cli::parse_from(["macdebounce", "-d", "40", "-b", "left", "-v"]);
        assert_eq!(cli.debounce_ms, Some(40));
        assert_eq!(cli.buttons.as_deref(), Some("left"));
        assert!(cli.verbose);

        let action = Config::process_cli(cli).unwrap();
        match action {
            CliAction::Run(cfg) => {
                assert_eq!(cfg.debounce_ms, 40);
                assert_eq!(cfg.selection, ButtonSelection::Specific(1 << 0));
                assert_eq!(cfg.log_level, LogLevel::Debug);
            }
            _ => panic!("Expected CliAction::Run"),
        }
    }
}
