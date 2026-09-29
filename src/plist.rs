use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::debounce::ButtonSelection;
use crate::event_tap::check_accessibility;

pub const LAUNCHD_LABEL: &str = "com.macdebounce.daemon";
pub const SIGNING_IDENTIFIER: &str = "com.macdebounce.daemon";

pub fn sign_binary(binary_path: &Path) -> Result<(), String> {
    // If the binary already satisfies its designated requirement, do not touch it
    if let Some(status) = check_signature_status(binary_path) {
        if status.contains("[VALID]") {
            return Ok(());
        }
    }

    let req = format!("designated => identifier \"{SIGNING_IDENTIFIER}\"");
    let path_str = binary_path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in binary path".to_string())?;

    // Codesign must only be run directly in place. Never overwrite binary bytes in-place
    // with file writes, as that causes macOS kernel code-signing cache taint (SIGKILL).
    let res = Command::new("codesign")
        .args([
            "--force",
            "-s",
            "-",
            "-i",
            SIGNING_IDENTIFIER,
            &format!("-r={req}"),
            path_str,
        ])
        .output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(format!(
                "codesign failed for {}: {stderr}. Administrator privileges may be required.",
                binary_path.display()
            ))
        }
        Err(e) => Err(format!("Failed to execute codesign: {e}")),
    }
}

pub fn check_signature_status(binary_path: &Path) -> Option<String> {
    let path_str = binary_path.to_str()?;
    let output = Command::new("codesign")
        .args(["-v", "--verbose=4", path_str])
        .output()
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}\n{stderr}");

    if output.status.success() {
        if combined.contains("satisfies its Designated Requirement") {
            Some("[VALID] (stable designated requirement)".to_string())
        } else {
            Some("[VALID]".to_string())
        }
    } else if combined.contains("does not satisfy its designated Requirement") {
        Some("[MISMATCH] (run `macdebounce --install-launchd` to repair)".to_string())
    } else {
        Some("[NOT SIGNED]".to_string())
    }
}

pub fn get_log_dir() -> Result<PathBuf, String> {
    let home =
        env::var_os("HOME").ok_or_else(|| "HOME environment variable is not set".to_string())?;
    Ok(PathBuf::from(home).join("Library/Logs"))
}

pub fn get_launch_agents_dir() -> Result<PathBuf, String> {
    let home =
        env::var_os("HOME").ok_or_else(|| "HOME environment variable is not set".to_string())?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents"))
}

pub fn get_plist_path() -> Result<PathBuf, String> {
    let dir = get_launch_agents_dir()?;
    Ok(dir.join(format!("{LAUNCHD_LABEL}.plist")))
}

pub fn get_service_status() -> Option<u32> {
    let output = Command::new("launchctl")
        .args(["list", LAUNCHD_LABEL])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"PID\" =") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                let pid_str = parts[1].trim().trim_end_matches(';').trim();
                if let Ok(pid) = pid_str.parse::<u32>() {
                    return Some(pid);
                }
            }
        }
    }
    None
}

pub fn generate_plist_string(binary_path: &Path, config: &Config) -> Result<String, String> {
    let bin_str = binary_path
        .to_str()
        .ok_or_else(|| "Invalid UTF-8 in binary path".to_string())?;

    let log_dir = get_log_dir()?;
    let out_log = log_dir
        .join("macdebounce.log")
        .to_str()
        .unwrap()
        .to_string();
    let err_log = log_dir
        .join("macdebounce.err")
        .to_str()
        .unwrap()
        .to_string();

    let mut args_xml = format!("        <string>{bin_str}</string>\n");

    // Pass configuration flags if needed
    if let Some(ref cfg_path) = config.config_file_path {
        args_xml.push_str("        <string>--config</string>\n");
        args_xml.push_str(&format!(
            "        <string>{}</string>\n",
            cfg_path.to_str().unwrap()
        ));
    } else {
        if config.debounce_ms != 50 {
            args_xml.push_str("        <string>--debounce-ms</string>\n");
            args_xml.push_str(&format!(
                "        <string>{}</string>\n",
                config.debounce_ms
            ));
        }

        match config.selection {
            ButtonSelection::All => {}
            ButtonSelection::Specific(mask) => {
                let mut list = Vec::new();
                for i in 0..crate::debounce::MAX_BUTTONS {
                    if (mask & (1 << i)) != 0 {
                        list.push(i.to_string());
                    }
                }
                args_xml.push_str("        <string>--buttons</string>\n");
                args_xml.push_str(&format!("        <string>{}</string>\n", list.join(",")));
            }
        }
    }

    match config.log_level {
        crate::logger::LogLevel::Trace => {
            args_xml.push_str("        <string>--log-all</string>\n");
        }
        crate::logger::LogLevel::Debug => {
            args_xml.push_str("        <string>--verbose</string>\n");
        }
        _ => {}
    }

    if !config.use_syslog {
        args_xml.push_str("        <string>--no-syslog</string>\n");
    }

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args_xml}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
        <key>Crashed</key>
        <true/>
    </dict>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardOutPath</key>
    <string>{out_log}</string>
    <key>StandardErrorPath</key>
    <string>{err_log}</string>
</dict>
</plist>
"#
    ))
}

pub fn install_launchd(config: &Config) -> Result<(), String> {
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to locate current executable: {e}"))?;
    let canonical_exe = fs::canonicalize(&current_exe).unwrap_or(current_exe);

    let plist_dir = get_launch_agents_dir()?;
    let log_dir = get_log_dir()?;

    fs::create_dir_all(&plist_dir)
        .map_err(|e| format!("Failed to create LaunchAgents directory: {e}"))?;
    fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create Logs directory: {e}"))?;

    let plist_path = get_plist_path()?;
    let plist_content = generate_plist_string(&canonical_exe, config)?;

    fs::write(&plist_path, plist_content)
        .map_err(|e| format!("Failed to write plist file: {e}"))?;

    println!("Wrote launchd service file to: {}", plist_path.display());

    let plist_path_str = plist_path.to_str().unwrap();

    // Try modern launchctl bootstrap first, fallback to load
    let uid_output = Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "501".to_string());

    let target_domain = format!("gui/{uid_output}");

    // 1. Unload existing service first so running binary is stopped cleanly
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("{target_domain}/{LAUNCHD_LABEL}")])
        .output();
    let _ = Command::new("launchctl")
        .args(["unload", plist_path_str])
        .output();

    // 2. Ensure canonical binary has a stable identifier and designated requirement
    // BEFORE starting launchd service, so macOS TCC Accessibility permissions persist
    // across reboots and logins.
    if let Err(err) = sign_binary(&canonical_exe) {
        eprintln!("Notice: {err}");
    }

    // 3. Load new service
    let boot_res = Command::new("launchctl")
        .args(["bootstrap", &target_domain, plist_path_str])
        .output();

    let success = match boot_res {
        Ok(out) if out.status.success() => true,
        _ => {
            // Fallback to legacy load
            let load_res = Command::new("launchctl")
                .args(["load", plist_path_str])
                .output();
            match load_res {
                Ok(out) => out.status.success(),
                Err(_) => false,
            }
        }
    };

    if success {
        println!("Successfully loaded service into launchctl!");
    } else {
        println!(
            "Service registered at {}. You can load it with:",
            plist_path.display()
        );
        println!("  launchctl load {}", plist_path.display());
    }

    // Check accessibility status and advise
    if check_accessibility(false) {
        println!("\n[OK] Accessibility permission is granted.");
    } else {
        println!("\n[ACTION REQUIRED] Accessibility permission needed!");
        println!("MacDebounce requires Accessibility permission to filter mouse clicks.");
        println!("  1. Open System Settings -> Privacy & Security -> Accessibility");
        println!(
            "  2. Enable MacDebounce (or add: {})",
            canonical_exe.display()
        );
        println!("  MacDebounce is running in the background and will automatically activate");
        println!("  once permission is enabled (no restart required).");
        let _ = check_accessibility(true);
    }

    println!("\nService logs are located at:");
    println!("  {}", log_dir.join("macdebounce.log").display());
    println!("  {}", log_dir.join("macdebounce.err").display());

    Ok(())
}

pub fn uninstall_launchd() -> Result<(), String> {
    let plist_path = get_plist_path()?;

    let uid_output = Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "501".to_string());

    let target_domain = format!("gui/{uid_output}");

    if plist_path.exists() {
        let plist_path_str = plist_path.to_str().unwrap();

        // Stop and unload
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{target_domain}/{LAUNCHD_LABEL}")])
            .output();
        let _ = Command::new("launchctl")
            .args(["unload", plist_path_str])
            .output();

        fs::remove_file(&plist_path).map_err(|e| format!("Failed to remove plist file: {e}"))?;

        println!("Successfully unloaded and removed {}", plist_path.display());
    } else {
        println!("No LaunchAgent found at {}", plist_path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_plist_string() {
        let config = Config::default();
        let bin_path = Path::new("/usr/local/bin/macdebounce");
        let plist_xml = generate_plist_string(bin_path, &config).expect("failed to generate plist");

        assert!(
            plist_xml.contains("<key>Label</key>\n    <string>com.macdebounce.daemon</string>")
        );
        assert!(plist_xml.contains("<string>/usr/local/bin/macdebounce</string>"));
        assert!(plist_xml.contains("<key>KeepAlive</key>"));
        assert!(plist_xml.contains("<key>SuccessfulExit</key>\n        <false/>"));
        assert!(plist_xml.contains("<key>Crashed</key>\n        <true/>"));
    }

    #[test]
    fn test_sign_binary_and_check_signature_status() {
        let current_exe = env::current_exe().expect("failed to get current_exe");
        let temp_dir = env::temp_dir();
        let temp_bin = temp_dir.join(format!("test_macdebounce_{}", std::process::id()));

        fs::copy(&current_exe, &temp_bin).expect("failed to copy test binary");

        let sign_res = sign_binary(&temp_bin);
        assert!(sign_res.is_ok(), "signing failed: {:?}", sign_res.err());

        let status = check_signature_status(&temp_bin);
        assert!(
            status.as_deref() == Some("[VALID] (stable designated requirement)"),
            "unexpected status: {:?}",
            status
        );

        let _ = fs::remove_file(&temp_bin);
    }
}
