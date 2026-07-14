use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::core::error::{AppError, AppResult};
use crate::core::time;

const RESET: &str = "\x1b[0m";
const GRAY: &str = "\x1b[90m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
static FILE_LOG_STATE: OnceLock<Mutex<FileLogConfig>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct FileLogConfig {
    pub success_log_path: Option<PathBuf>,
    pub failure_log_path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum LogLevel {
    Info,
    Warn,
    Error,
    Success,
}

fn timestamp() -> String {
    time::current_timestamp_hms()
}

fn style(level: LogLevel) -> (&'static str, &'static str) {
    match level {
        LogLevel::Info => (BLUE, "INFO"),
        LogLevel::Warn => (YELLOW, "WARN"),
        LogLevel::Error => (RED, "ERROR"),
        LogLevel::Success => (GREEN, " OK "),
    }
}

fn log(level: LogLevel, message: &str) {
    let (color, label) = style(level);
    let mut line = String::new();
    let _ = write!(
        &mut line,
        "{GRAY}{}{RESET} {color}{BOLD}[{label}]{RESET} {}",
        timestamp(),
        message
    );

    match level {
        LogLevel::Warn | LogLevel::Error => eprintln!("{line}"),
        _ => println!("{line}"),
    }
}

fn persist(level: LogLevel, message: &str) {
    log(level, message);

    let Some(path) = persist_target(level) else {
        return;
    };

    if let Err(err) = append_plain_log(&path, level, message) {
        warn(format!("写入日志文件失败: {} ({err})", path.display()));
    }
}

fn persist_target(level: LogLevel) -> Option<PathBuf> {
    let state = file_log_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match level {
        LogLevel::Info | LogLevel::Success => state.success_log_path.clone(),
        LogLevel::Warn | LogLevel::Error => state.failure_log_path.clone(),
    }
}

fn append_plain_log(path: &Path, level: LogLevel, message: &str) -> AppResult<()> {
    ensure_log_target(path)?;
    let (_, label) = style(level);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{} [{label}] {}", timestamp(), message).map_err(AppError::from)
}

fn file_log_state() -> &'static Mutex<FileLogConfig> {
    FILE_LOG_STATE.get_or_init(|| Mutex::new(FileLogConfig::default()))
}

fn ensure_log_target(path: &Path) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(AppError::from)
}

pub fn configure_file_logs(config: FileLogConfig) -> AppResult<()> {
    if let Some(path) = config.success_log_path.as_deref() {
        ensure_log_target(path)?;
    }
    if let Some(path) = config.failure_log_path.as_deref() {
        ensure_log_target(path)?;
    }

    let mut state = file_log_state()
        .lock()
        .map_err(|_| AppError::new("日志配置状态已损坏"))?;
    *state = config;
    Ok(())
}

pub fn info(message: impl AsRef<str>) {
    log(LogLevel::Info, message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    log(LogLevel::Warn, message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    log(LogLevel::Error, message.as_ref());
}

pub fn success(message: impl AsRef<str>) {
    log(LogLevel::Success, message.as_ref());
}

pub fn info_persist(message: impl AsRef<str>) {
    persist(LogLevel::Info, message.as_ref());
}

pub fn warn_persist(message: impl AsRef<str>) {
    persist(LogLevel::Warn, message.as_ref());
}

pub fn error_persist(message: impl AsRef<str>) {
    persist(LogLevel::Error, message.as_ref());
}

pub fn success_persist(message: impl AsRef<str>) {
    persist(LogLevel::Success, message.as_ref());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{FileLogConfig, configure_file_logs, error_persist, info_persist, success_persist};

    #[test]
    fn persistent_logs_append_plain_lines() {
        let dir = std::env::temp_dir().join(format!("dytl-logger-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let success_path = dir.join("success.log");
        let failure_path = dir.join("nested").join("failure.log");

        configure_file_logs(FileLogConfig {
            success_log_path: Some(success_path.clone()),
            failure_log_path: Some(failure_path.clone()),
        })
        .expect("logger should configure file targets");

        success_persist("saved profile");
        info_persist("processing item");
        error_persist("failed recording");

        let success = fs::read_to_string(&success_path).expect("success log should exist");
        let failure = fs::read_to_string(&failure_path).expect("failure log should exist");

        assert!(success.contains("[ OK ] saved profile"));
        assert!(success.contains("[INFO] processing item"));
        assert!(failure.contains("[ERROR] failed recording"));
        assert!(!success.contains("\x1b["));
        assert!(!failure.contains("\x1b["));

        configure_file_logs(FileLogConfig::default()).expect("logger should reset file targets");
        let _ = fs::remove_dir_all(&dir);
    }
}
