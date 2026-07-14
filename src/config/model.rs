//! Validated runtime configuration types.

use std::path::PathBuf;

use crate::core::logger::FileLogConfig;

use super::platform::Platform;

/// One account entry under `monitor.targets`.
#[derive(Debug, Clone)]
pub struct MonitorTarget {
    pub platform: Platform,
    pub account: String,
    pub alias: Option<String>,
    pub enabled: bool,
}

impl MonitorTarget {
    pub fn key(&self) -> String {
        format!("{}:{}", self.platform.as_str(), self.account)
    }

    pub fn short_account(&self) -> String {
        self.account.chars().take(12).collect()
    }

    pub fn display_name(&self, nickname: Option<&str>) -> String {
        self.alias
            .clone()
            .or_else(|| nickname.map(str::to_string))
            .unwrap_or_else(|| self.short_account())
    }

    pub fn log_name(&self, nickname: Option<&str>) -> String {
        format!(
            "{} / {}",
            self.platform.label(),
            self.display_name(nickname)
        )
    }
}

/// Top-level monitor settings.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub poll_interval_sec: u64,
    pub targets: Vec<MonitorTarget>,
    pub restart_interval_hours: Option<u64>,
    pub auto_rescue: AutoRescueConfig,
}

impl MonitorConfig {
    pub fn enabled_target_count(&self) -> usize {
        self.targets.iter().filter(|target| target.enabled).count()
    }

    pub fn enabled_targets(&self) -> impl Iterator<Item = &MonitorTarget> {
        self.targets.iter().filter(|target| target.enabled)
    }
}

/// Automatic repair of interrupted `temp_record_*` directories.
#[derive(Debug, Clone)]
pub struct AutoRescueConfig {
    pub enabled: bool,
    pub on_startup: bool,
    pub interval_minutes: u64,
    pub min_age_minutes: u64,
}

#[derive(Debug, Clone)]
pub struct DouyinConfig {
    pub cookies: String,
}

#[derive(Debug, Clone)]
pub struct KuaishouConfig {
    pub cookies: String,
}

#[derive(Debug, Clone)]
pub struct TwitterConfig {
    pub cookies: String,
}

#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub success_log_path: Option<PathBuf>,
    pub failure_log_path: Option<PathBuf>,
}

impl LoggingConfig {
    pub(crate) fn to_file_log_config(&self) -> FileLogConfig {
        FileLogConfig {
            success_log_path: self.success_log_path.clone(),
            failure_log_path: self.failure_log_path.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimeConfig {
    pub utc_offset_hours: i8,
}

/// Fully validated application configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub douyin: Option<DouyinConfig>,
    pub kuaishou: Option<KuaishouConfig>,
    pub twitter: Option<TwitterConfig>,
    pub monitor: Option<MonitorConfig>,
    pub logging: Option<LoggingConfig>,
    pub time: TimeConfig,
}
