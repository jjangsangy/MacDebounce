use std::env;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::SystemTime;

unsafe extern "C" {
    fn openlog(ident: *const u8, logopt: i32, facility: i32);
    fn syslog(priority: i32, format: *const u8, ...);
}

// syslog constants
const LOG_PID: i32 = 0x01;
const LOG_DAEMON: i32 = 3 << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }
}

pub struct Logger {
    level: LogLevel,
    use_syslog: bool,
    file_writer: Option<Mutex<File>>,
}

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

pub fn init(level: LogLevel, use_syslog: bool, log_file: Option<&Path>) {
    if use_syslog {
        let ident = b"macdebounce\0";
        unsafe {
            openlog(ident.as_ptr(), LOG_PID, LOG_DAEMON);
        }
    }

    let file_writer = log_file.and_then(|path| {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    });

    let mut guard = LOGGER.lock().unwrap();
    *guard = Some(Logger {
        level,
        use_syslog,
        file_writer,
    });
}

pub(crate) fn syslog_priority(level: LogLevel) -> i32 {
    match level {
        LogLevel::Error => 3, // LOG_ERR
        LogLevel::Warn => 4,  // LOG_WARNING
        // Map Info, Debug, and Trace to LOG_NOTICE (5) so that macOS Unified Logging
        // (os_log) treats them as OS_LOG_TYPE_DEFAULT rather than OS_LOG_TYPE_DEBUG.
        // On macOS, priority 7 (LOG_DEBUG) is silently discarded by default and never
        // persisted to the datastore or shown in Console.app / log show.
        // MacDebounce already filters by LogLevel internally, so only events the user
        // wants logged will be sent to syslog.
        LogLevel::Info | LogLevel::Debug | LogLevel::Trace => 5, // LOG_NOTICE
    }
}

pub fn log(level: LogLevel, message: &str) {
    let guard = LOGGER.lock().unwrap();
    let (current_level, use_syslog, has_file) = if let Some(ref l) = *guard {
        (l.level, l.use_syslog, l.file_writer.is_some())
    } else {
        (LogLevel::Info, false, false)
    };

    if level > current_level {
        return;
    }

    // Format timestamp: HH:MM:SS.mmm
    let now = SystemTime::now();
    let formatted_time = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            let millis = dur.subsec_millis();
            let hours = (secs / 3600) % 24;
            let minutes = (secs / 60) % 60;
            let seconds = secs % 60;
            format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
        }
        Err(_) => "00:00:00.000".to_string(),
    };

    let log_line = format!("{formatted_time} [{}] {message}", level.as_str());

    // 1. Output to stderr for console / launchd stdout/err capture
    eprintln!("{log_line}");

    // 2. Output to log file if configured
    if has_file {
        if let Some(ref l) = *guard {
            if let Some(ref file_mutex) = l.file_writer {
                if let Ok(mut f) = file_mutex.lock() {
                    let _ = writeln!(f, "{log_line}");
                }
            }
        }
    }

    // 3. Output to macOS Unified Logging System (syslog)
    if use_syslog {
        let priority = syslog_priority(level);
        let c_fmt = b"%s\0";
        if let Ok(c_msg) = CString::new(format!("[{}] {message}", level.as_str())) {
            unsafe {
                syslog(priority, c_fmt.as_ptr(), c_msg.as_ptr());
            }
        }
    }
}

pub fn log_error(msg: &str) {
    log(LogLevel::Error, msg);
}

pub fn log_warn(msg: &str) {
    log(LogLevel::Warn, msg);
}

pub fn log_info(msg: &str) {
    log(LogLevel::Info, msg);
}

pub fn log_debug(msg: &str) {
    log(LogLevel::Debug, msg);
}

#[allow(dead_code)]
pub fn log_trace(msg: &str) {
    log(LogLevel::Trace, msg);
}

/// Display recent lines from daemon log files
pub fn show_recent_logs() {
    let home = match env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => {
            eprintln!("Error: HOME environment variable is not set.");
            return;
        }
    };

    let log_path = home.join("Library/Logs/macdebounce.log");
    let err_path = home.join("Library/Logs/macdebounce.err");

    println!("=== MacDebounce Log Viewer ===");
    println!("Log file:   {}", log_path.display());
    println!("Error file: {}", err_path.display());
    println!("------------------------------------------------------------");

    let print_tail = |path: &Path, label: &str| {
        if !path.exists() {
            println!("[{label}] (file does not exist yet)");
            return;
        }
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("[{label}] Failed to open file: {e}");
                return;
            }
        };

        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        println!("[{label}] (last {} lines):", lines.len().min(30));
        let start = lines.len().saturating_sub(30);
        for line in &lines[start..] {
            println!("  {line}");
        }
        if lines.is_empty() {
            println!("  (empty)");
        }
    };

    print_tail(&log_path, "Standard Output");
    println!("------------------------------------------------------------");
    print_tail(&err_path, "Standard Error / Debounce Events");
    println!("------------------------------------------------------------");
    println!("Tip: Stream live logs in real time with: macdebounce --stream-logs");
}

/// Stream live logs from macOS Unified Logging System or log file
pub fn stream_logs() {
    println!("Streaming live MacDebounce logs from macOS Unified Logging System...");
    println!("Press Ctrl+C to stop.\n");

    let status = Command::new("log")
        .args([
            "stream",
            "--predicate",
            "process == \"macdebounce\" || sender == \"macdebounce\"",
            "--info",
            "--debug",
            "--style",
            "compact",
        ])
        .status();

    if let Err(e) = status {
        eprintln!("Failed to run 'log stream': {e}");
        println!("Falling back to tailing log file...");
        if let Some(home) = env::var_os("HOME") {
            let log_path = PathBuf::from(home).join("Library/Logs/macdebounce.err");
            let _ = Command::new("tail")
                .args(["-f", "-n", "30", log_path.to_str().unwrap()])
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syslog_priority_mapping() {
        // macOS Unified Logging drops priority 7 (LOG_DEBUG) by default,
        // so Info, Debug, and Trace must map to LOG_NOTICE (5) to be persisted
        // and visible in Console.app / log show.
        assert_eq!(syslog_priority(LogLevel::Error), 3);
        assert_eq!(syslog_priority(LogLevel::Warn), 4);
        assert_eq!(syslog_priority(LogLevel::Info), 5);
        assert_eq!(syslog_priority(LogLevel::Debug), 5);
        assert_eq!(syslog_priority(LogLevel::Trace), 5);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Trace);
    }
}
