mod config;
mod debounce;
mod event_tap;
mod logger;
mod plist;

use std::env;
use std::process;

use clap::Parser;
use config::{Cli, CliAction, Config};
use event_tap::{check_accessibility, run_event_tap};
use plist::{LAUNCHD_LABEL, get_plist_path, install_launchd, uninstall_launchd};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_status() {
    println!("MacDebounce v{VERSION} Status:");
    println!("----------------------------------------");

    // Accessibility check
    let trusted = check_accessibility(false);
    if trusted {
        println!("Accessibility Permission: [GRANTED]");
    } else {
        println!("Accessibility Permission: [NOT GRANTED]");
        println!("  To grant: System Settings -> Privacy & Security -> Accessibility");
    }

    // LaunchAgent check
    if let Ok(plist_path) = get_plist_path() {
        if plist_path.is_file() {
            println!(
                "LaunchAgent plist:        [INSTALLED] ({})",
                plist_path.display()
            );
            println!("  Service label:          {LAUNCHD_LABEL}");
        } else {
            println!("LaunchAgent plist:        [NOT INSTALLED]");
        }
    }

    // Default config check
    let (cfg, found_path) = Config::load_from_file_or_defaults(None);
    if let Some(path) = found_path {
        println!("Config file:              [FOUND] ({})", path.display());
    } else {
        println!("Config file:              [NOT FOUND] (using built-in defaults)");
    }
    println!("  Debounce duration:      {} ms", cfg.debounce_ms);
    println!("  Debounced buttons:      {}", cfg.format_buttons_summary());
}

fn main() {
    let cli = Cli::parse();

    let action = match Config::process_cli(cli) {
        Ok(action) => action,
        Err(err) => {
            eprintln!("Error: {err}");
            process::exit(1);
        }
    };

    match action {
        CliAction::Status => {
            print_status();
        }
        CliAction::ShowLogs => {
            logger::show_recent_logs();
        }
        CliAction::StreamLogs => {
            logger::stream_logs();
        }
        CliAction::GeneratePlist(config) => {
            let current_exe = env::current_exe().unwrap_or_else(|_| "macdebounce".into());
            let canonical = std::fs::canonicalize(&current_exe).unwrap_or(current_exe);
            match plist::generate_plist_string(&canonical, &config) {
                Ok(xml) => print!("{xml}"),
                Err(e) => {
                    eprintln!("Error generating plist: {e}");
                    process::exit(1);
                }
            }
        }
        CliAction::InstallLaunchd(config) => {
            if let Err(e) = install_launchd(&config) {
                eprintln!("Error installing launchd service: {e}");
                process::exit(1);
            }
        }
        CliAction::UninstallLaunchd => {
            if let Err(e) = uninstall_launchd() {
                eprintln!("Error uninstalling launchd service: {e}");
                process::exit(1);
            }
        }
        CliAction::Run(config) => {
            println!("Starting MacDebounce v{VERSION}...");
            println!("  Debounce delay:   {} ms", config.debounce_ms);
            println!("  Target buttons:   {}", config.format_buttons_summary());
            println!("  Log level:        {:?}", config.log_level);
            println!(
                "  macOS syslog:     {}",
                if config.use_syslog {
                    "enabled (view in Console.app / log stream)"
                } else {
                    "disabled"
                }
            );

            if let Some(ref path) = config.log_file {
                println!("  Custom log file:  {}", path.display());
            }

            if let Some(ref path) = config.config_file_path {
                println!("  Loaded config:    {}", path.display());
            }

            if let Err(err) = run_event_tap(config) {
                eprintln!("\nError: {err}");
                process::exit(1);
            }
        }
    }
}
