//! YAML configuration loading and validation.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::time;

use super::model::{
    AppConfig, AutoRescueConfig, DouyinConfig, KuaishouConfig, LoggingConfig, MonitorConfig,
    MonitorTarget, TimeConfig, TwitterConfig,
};
use super::platform::Platform;

const DEFAULT_CONFIG_PATH: &str = "config.yaml";
const DEFAULT_UTC_OFFSET_HOURS: i64 = 8;
static CONFIG_PATH_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDouyinConfig {
    cookies: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKuaishouConfig {
    cookies: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTwitterConfig {
    cookies: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    douyin: Option<RawDouyinConfig>,
    kuaishou: Option<RawKuaishouConfig>,
    twitter: Option<RawTwitterConfig>,
    monitor: Option<RawMonitorConfig>,
    logging: Option<RawLoggingConfig>,
    time: Option<RawTimeConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RawLoggingOnlyConfig {
    douyin: Option<serde_yaml::Value>,
    kuaishou: Option<serde_yaml::Value>,
    twitter: Option<serde_yaml::Value>,
    monitor: Option<serde_yaml::Value>,
    logging: Option<RawLoggingConfig>,
    time: Option<RawTimeConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMonitorConfig {
    poll_interval_sec: Option<u64>,
    targets: Option<Vec<RawMonitorTarget>>,
    restart_interval_hours: Option<u64>,
    auto_rescue: Option<RawAutoRescueConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAutoRescueConfig {
    enabled: Option<bool>,
    on_startup: Option<bool>,
    interval_minutes: Option<u64>,
    min_age_minutes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLoggingConfig {
    success_log_path: Option<String>,
    failure_log_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTimeConfig {
    utc_offset_hours: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMonitorTarget {
    platform: Option<String>,
    account: Option<String>,
    alias: Option<String>,
    enabled: Option<bool>,
}

pub fn set_config_path(path: PathBuf) -> AppResult<()> {
    CONFIG_PATH_OVERRIDE
        .set(path)
        .map_err(|_| AppError::new("全局配置文件路径只能设置一次"))
}

pub fn current_config_path() -> PathBuf {
    CONFIG_PATH_OVERRIDE
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
}

pub fn load_config() -> AppResult<AppConfig> {
    let config_path = current_config_path();
    load_config_from_path(&config_path)
}

pub fn load_config_from_path(config_path: &Path) -> AppResult<AppConfig> {
    if !config_path.exists() {
        logger::error(format!("未找到配置文件: {}", config_path.to_string_lossy()));
        if config_path == Path::new(DEFAULT_CONFIG_PATH) {
            logger::error(
                "请复制 config.example.yaml 为 config.yaml 并填入实际值，或使用 --config 指定配置文件路径",
            );
            return Err(AppError::new("missing config.yaml"));
        }

        logger::error("请检查 --config 指向的文件路径是否正确");
        return Err(AppError::new(format!(
            "missing config file: {}",
            config_path.display()
        )));
    }

    let content = fs::read_to_string(config_path)?;
    let config = load_config_from_str(&content)?;
    apply_runtime_config(&config)?;
    Ok(config)
}

pub fn try_init_logging_from_current_config() -> AppResult<()> {
    let config_path = current_config_path();
    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_path)?;
    let raw: RawLoggingOnlyConfig = serde_yaml::from_str(&content)?;
    let time = normalize_time_config(raw.time)?;
    let logging = normalize_logging_config(raw.logging);

    time::configure_utc_offset_hours(time.utc_offset_hours);
    logger::configure_file_logs(
        logging
            .as_ref()
            .map(LoggingConfig::to_file_log_config)
            .unwrap_or_default(),
    )
}

fn apply_runtime_config(config: &AppConfig) -> AppResult<()> {
    time::configure_utc_offset_hours(config.time.utc_offset_hours);
    logger::configure_file_logs(
        config
            .logging
            .as_ref()
            .map(LoggingConfig::to_file_log_config)
            .unwrap_or_default(),
    )
}

fn load_config_from_str(content: &str) -> AppResult<AppConfig> {
    let raw: RawConfig = serde_yaml::from_str(content)?;
    let logging = normalize_logging_config(raw.logging);
    let time = normalize_time_config(raw.time)?;
    let douyin = raw.douyin.and_then(|douyin| {
        if douyin.cookies.trim().is_empty() {
            return None;
        }
        Some(DouyinConfig {
            cookies: douyin.cookies,
        })
    });

    let kuaishou = raw.kuaishou.map(|kuaishou| KuaishouConfig {
        cookies: kuaishou.cookies,
    });

    let twitter = raw.twitter.map(|twitter| TwitterConfig {
        cookies: twitter.cookies,
    });

    if douyin.is_none() && kuaishou.is_none() && twitter.is_none() {
        return Err(AppError::new(
            "配置文件格式错误，至少需要配置 douyin.cookies、kuaishou.cookies 或 twitter.cookies",
        ));
    }

    let monitor = normalize_monitor_config(
        raw.monitor,
        douyin.is_some(),
        kuaishou.is_some(),
        twitter.is_some(),
    )?;

    Ok(AppConfig {
        douyin,
        kuaishou,
        twitter,
        monitor,
        logging,
        time,
    })
}

fn normalize_logging_config(raw_logging: Option<RawLoggingConfig>) -> Option<LoggingConfig> {
    let raw_logging = raw_logging?;
    let success_log_path =
        normalize_optional_string(raw_logging.success_log_path).map(PathBuf::from);
    let failure_log_path =
        normalize_optional_string(raw_logging.failure_log_path).map(PathBuf::from);

    if success_log_path.is_none() && failure_log_path.is_none() {
        return None;
    }

    Some(LoggingConfig {
        success_log_path,
        failure_log_path,
    })
}

fn normalize_time_config(raw_time: Option<RawTimeConfig>) -> AppResult<TimeConfig> {
    let utc_offset_hours = raw_time
        .and_then(|time| time.utc_offset_hours)
        .unwrap_or(DEFAULT_UTC_OFFSET_HOURS);

    if !(-23..=23).contains(&utc_offset_hours) {
        return Err(AppError::new(format!(
            "time.utc_offset_hours 超出范围: {utc_offset_hours}，必须在 -23 到 23 之间"
        )));
    }

    Ok(TimeConfig {
        utc_offset_hours: utc_offset_hours as i8,
    })
}

fn normalize_monitor_config(
    raw_monitor: Option<RawMonitorConfig>,
    has_douyin_cookie: bool,
    has_kuaishou_cookie: bool,
    has_twitter_config: bool,
) -> AppResult<Option<MonitorConfig>> {
    let Some(raw_monitor) = raw_monitor else {
        return Ok(None);
    };

    let raw_targets = raw_monitor.targets.unwrap_or_default();
    if raw_targets.is_empty() {
        return Ok(None);
    }

    let mut targets = Vec::with_capacity(raw_targets.len());
    for raw_target in raw_targets {
        targets.push(normalize_monitor_target(raw_target)?);
    }

    if targets
        .iter()
        .any(|target| target.enabled && target.platform == Platform::Douyin)
        && !has_douyin_cookie
    {
        return Err(AppError::new(
            "monitor.targets 中包含抖音账号，但未配置 douyin.cookies",
        ));
    }

    if targets
        .iter()
        .any(|target| target.enabled && target.platform == Platform::Kuaishou)
        && !has_kuaishou_cookie
    {
        return Err(AppError::new(
            "monitor.targets 中包含快手账号，但未配置 kuaishou.cookies",
        ));
    }

    if targets
        .iter()
        .any(|target| target.enabled && target.platform == Platform::Twitter)
        && !has_twitter_config
    {
        return Err(AppError::new(
            "monitor.targets 中包含 Twitter/X 账号，但未配置 twitter.cookies",
        ));
    }

    let mut poll_interval_sec = raw_monitor.poll_interval_sec.unwrap_or(30);
    if poll_interval_sec < 10 {
        logger::warn("poll_interval_sec 未配置或小于 10 秒，自动设置为 30 秒");
        poll_interval_sec = 30;
    }

    let restart_interval_hours = raw_monitor
        .restart_interval_hours
        .filter(|hours| *hours > 0);
    let auto_rescue = normalize_auto_rescue_config(raw_monitor.auto_rescue);

    Ok(Some(MonitorConfig {
        poll_interval_sec,
        targets,
        restart_interval_hours,
        auto_rescue,
    }))
}

fn normalize_auto_rescue_config(raw: Option<RawAutoRescueConfig>) -> AutoRescueConfig {
    let Some(raw) = raw else {
        return AutoRescueConfig {
            enabled: true,
            on_startup: true,
            interval_minutes: 10,
            min_age_minutes: 5,
        };
    };

    AutoRescueConfig {
        enabled: raw.enabled.unwrap_or(true),
        on_startup: raw.on_startup.unwrap_or(true),
        interval_minutes: raw
            .interval_minutes
            .filter(|minutes| *minutes > 0)
            .unwrap_or(10),
        min_age_minutes: raw.min_age_minutes.unwrap_or(5),
    }
}

fn normalize_monitor_target(raw_target: RawMonitorTarget) -> AppResult<MonitorTarget> {
    let alias = normalize_optional_string(raw_target.alias);
    let enabled = raw_target.enabled.unwrap_or(true);

    let account = normalize_optional_string(raw_target.account)
        .ok_or_else(|| AppError::new("monitor.targets[*].account 不能为空"))?;
    let platform_text = normalize_optional_string(raw_target.platform)
        .ok_or_else(|| AppError::new("monitor.targets[*].platform 不能为空"))?;
    let platform = Platform::parse(&platform_text).ok_or_else(|| {
        AppError::new(format!(
            "不支持的 monitor platform: {}，当前仅支持 douyin / kuaishou / twitter",
            platform_text
        ))
    })?;

    Ok(MonitorTarget {
        platform,
        account,
        alias,
        enabled,
    })
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        RawMonitorConfig, RawMonitorTarget, load_config_from_str, normalize_monitor_config,
        normalize_monitor_target,
    };
    use crate::config::Platform;

    #[test]
    fn normalizes_platform_account_target_with_enabled_flag() {
        let target = normalize_monitor_target(RawMonitorTarget {
            platform: Some("kuaishou".to_string()),
            account: Some("test_ks_account".to_string()),
            alias: Some("快手主播".to_string()),
            enabled: Some(false),
        })
        .expect("target should parse");

        assert_eq!(target.platform, Platform::Kuaishou);
        assert_eq!(target.account, "test_ks_account");
        assert_eq!(target.alias.as_deref(), Some("快手主播"));
        assert!(!target.enabled);
    }

    #[test]
    fn disabled_targets_do_not_require_platform_cookies() {
        let monitor = normalize_monitor_config(
            Some(RawMonitorConfig {
                poll_interval_sec: Some(30),
                targets: Some(vec![RawMonitorTarget {
                    platform: Some("kuaishou".to_string()),
                    account: Some("test_ks_account".to_string()),
                    alias: Some("已暂停监控".to_string()),
                    enabled: Some(false),
                }]),
                restart_interval_hours: None,
                auto_rescue: None,
            }),
            false,
            false,
            false,
        )
        .expect("disabled target should not require cookies")
        .expect("monitor should exist");

        assert_eq!(monitor.targets.len(), 1);
        assert_eq!(monitor.enabled_target_count(), 0);
    }

    #[test]
    fn allows_empty_kuaishou_cookies() {
        let yaml = r#"
kuaishou:
  cookies: ""
monitor:
  poll_interval_sec: 30
  targets:
    - platform: kuaishou
      account: "test_ks_account"
      alias: "快手主播"
      enabled: true
"#;

        let config = load_config_from_str(yaml).expect("empty kuaishou cookies should parse");

        assert_eq!(
            config
                .kuaishou
                .expect("kuaishou config should exist")
                .cookies,
            ""
        );
        assert_eq!(
            config
                .monitor
                .expect("monitor should exist")
                .enabled_target_count(),
            1
        );
    }

    #[test]
    fn allows_empty_twitter_cookies() {
        let yaml = r#"
twitter:
  cookies: ""
monitor:
  poll_interval_sec: 30
  targets:
    - platform: twitter
      account: "test_screen_user"
      alias: "test_screen_user"
      enabled: true
"#;

        let config = load_config_from_str(yaml).expect("empty twitter cookies should parse");

        assert_eq!(
            config.twitter.expect("twitter config should exist").cookies,
            ""
        );
        assert_eq!(
            config
                .monitor
                .expect("monitor should exist")
                .enabled_target_count(),
            1
        );
    }

    #[test]
    fn parses_monitor_auto_rescue_config() {
        let yaml = r#"
douyin:
  cookies: "cookie"
monitor:
  poll_interval_sec: 30
  auto_rescue:
    enabled: true
    on_startup: false
    interval_minutes: 15
    min_age_minutes: 8
  targets:
    - platform: douyin
      account: "test_douyin_sec_uid_auto"
"#;

        let config = load_config_from_str(yaml).expect("config should parse");
        let auto_rescue = config.monitor.expect("monitor should exist").auto_rescue;

        assert!(auto_rescue.enabled);
        assert!(!auto_rescue.on_startup);
        assert_eq!(auto_rescue.interval_minutes, 15);
        assert_eq!(auto_rescue.min_age_minutes, 8);
    }

    #[test]
    fn allows_zero_monitor_auto_rescue_min_age() {
        let yaml = r#"
douyin:
  cookies: "cookie"
monitor:
  poll_interval_sec: 30
  auto_rescue:
    min_age_minutes: 0
  targets:
    - platform: douyin
      account: "test_douyin_sec_uid_auto"
"#;

        let config = load_config_from_str(yaml).expect("config should parse");
        let auto_rescue = config.monitor.expect("monitor should exist").auto_rescue;

        assert_eq!(auto_rescue.min_age_minutes, 0);
    }

    #[test]
    fn rejects_legacy_monitor_target_fields() {
        let yaml = r#"
douyin:
  cookies: "cookie"
monitor:
  poll_interval_sec: 30
  targets:
    - platform: douyin
      account_type: sec_uid
      account: "test_douyin_sec_uid_legacy"
      alias: "旧配置"
"#;

        let error = load_config_from_str(yaml).expect_err("legacy field should be rejected");

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_legacy_douyin_monitor_fields() {
        let yaml = r#"
douyin:
  cookies: "cookie"
  poll_interval_sec: 30
monitor:
  poll_interval_sec: 30
  targets:
    - platform: douyin
      account: "test_douyin_sec_uid_legacy"
"#;

        let error = load_config_from_str(yaml)
            .expect_err("legacy douyin monitor fields should be rejected");

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn parses_logging_and_time_config() {
        let yaml = r#"
douyin:
  cookies: "cookie"
logging:
  success_log_path: ./content/logs/success.log
  failure_log_path: ./content/logs/failure.log
time:
  utc_offset_hours: 8
"#;

        let config = load_config_from_str(yaml).expect("config should parse");

        let logging = config.logging.expect("logging config should exist");
        assert_eq!(
            logging.success_log_path.as_deref(),
            Some(std::path::Path::new("./content/logs/success.log"))
        );
        assert_eq!(
            logging.failure_log_path.as_deref(),
            Some(std::path::Path::new("./content/logs/failure.log"))
        );
        assert_eq!(config.time.utc_offset_hours, 8);
    }

    #[test]
    fn rejects_out_of_range_utc_offset() {
        let yaml = r#"
douyin:
  cookies: "cookie"
time:
  utc_offset_hours: 24
"#;

        let error = load_config_from_str(yaml).expect_err("offset should be rejected");

        assert!(error.to_string().contains("time.utc_offset_hours 超出范围"));
    }

    #[test]
    fn rejects_unknown_top_level_config_field() {
        let yaml = r#"
douyin:
  cookies: "cookie"
legacy: true
"#;

        let error = load_config_from_str(yaml).expect_err("unknown top-level field should fail");

        assert!(error.to_string().contains("unknown field"));
    }
}
