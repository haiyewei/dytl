use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::config::Platform;
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::media::ffmpeg;

#[derive(Debug, Clone)]
pub struct RescueOptions {
    pub platforms: Vec<Platform>,
    pub min_age: Duration,
    pub label: &'static str,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RescueSummary {
    pub found: usize,
    pub merged: usize,
    pub removed_empty: usize,
    pub skipped_recent: usize,
    pub failed: usize,
}

impl RescueSummary {
    pub fn handled_anything(self) -> bool {
        self.found > 0 || self.removed_empty > 0 || self.skipped_recent > 0 || self.failed > 0
    }
}

pub fn run(options: RescueOptions) -> AppResult<RescueSummary> {
    let mut summary = RescueSummary::default();

    for platform in &options.platforms {
        rescue_platform(*platform, &options, &mut summary)?;
    }

    if summary.handled_anything() {
        logger::info(format!(
            "{}完成: 发现 {} | 成功 {} | 清理空目录 {} | 跳过活跃/过新 {} | 失败 {}",
            options.label,
            summary.found,
            summary.merged,
            summary.removed_empty,
            summary.skipped_recent,
            summary.failed
        ));
    }

    Ok(summary)
}

fn rescue_platform(
    platform: Platform,
    options: &RescueOptions,
    summary: &mut RescueSummary,
) -> AppResult<()> {
    let live_dir = paths::live_dir(platform);
    if !live_dir.exists() {
        return Ok(());
    }

    let mut temp_dirs = fs::read_dir(&live_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("temp_record_")
        })
        .collect::<Vec<_>>();
    temp_dirs.sort_by_key(|entry| entry.file_name());

    for entry in temp_dirs {
        summary.found += 1;
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let dir_path = entry.path();

        if is_too_recent(&dir_path, options.min_age)? {
            summary.skipped_recent += 1;
            logger::info(format!(
                "[{}] 跳过仍可能活跃的临时目录: {dir_name}",
                platform.label()
            ));
            continue;
        }

        if is_empty_dir(&dir_path)? {
            match fs::remove_dir(&dir_path) {
                Ok(()) => {
                    summary.removed_empty += 1;
                    logger::info_persist(format!(
                        "[{}] 巡航发现空临时目录，已删除: {dir_name}",
                        platform.label()
                    ));
                }
                Err(err) => {
                    summary.failed += 1;
                    logger::warn_persist(format!(
                        "[{}] 删除空临时目录失败: {} ({err})",
                        platform.label(),
                        dir_path.display()
                    ));
                }
            }
            continue;
        }

        let mp4_path = live_dir.join(temp_dir_to_mp4_name(&dir_name)?);
        logger::info_persist("--------------------------------------------------");
        logger::info_persist(format!(
            "[{}] 巡航封装临时目录: {dir_name}",
            platform.label()
        ));

        match ffmpeg::merge_ts_segments(&dir_path, &mp4_path, "rescue_concat.txt") {
            Ok(()) => {
                summary.merged += 1;
                logger::success_persist(format!(
                    "[{}] 巡航封装成功: {}",
                    platform.label(),
                    mp4_path.display()
                ));
                if let Err(err) = fs::remove_dir_all(&dir_path) {
                    logger::warn_persist(format!(
                        "[{}] 清理临时目录失败: {} ({err})",
                        platform.label(),
                        dir_path.display()
                    ));
                } else {
                    logger::info(format!("已清理原始文件夹: {dir_name}"));
                }
            }
            Err(err) => {
                summary.failed += 1;
                logger::error_persist(format!(
                    "[{}] 巡航封装失败: {dir_name} ({err})",
                    platform.label()
                ));
            }
        }
    }

    Ok(())
}

fn is_empty_dir(dir_path: &Path) -> AppResult<bool> {
    Ok(fs::read_dir(dir_path)?.next().is_none())
}

fn temp_dir_to_mp4_name(dir_name: &str) -> AppResult<String> {
    let record_name = dir_name
        .strip_prefix("temp_")
        .ok_or_else(|| AppError::new(format!("临时目录命名不合法: {dir_name}")))?;
    Ok(format!("{record_name}.mp4"))
}

fn is_too_recent(dir_path: &Path, min_age: Duration) -> AppResult<bool> {
    if min_age.is_zero() {
        return Ok(false);
    }

    let newest = newest_modified_time(dir_path)?;
    let age = SystemTime::now()
        .duration_since(newest)
        .unwrap_or(Duration::ZERO);

    Ok(age < min_age)
}

fn newest_modified_time(dir_path: &Path) -> AppResult<SystemTime> {
    let mut newest = fs::metadata(dir_path)?.modified()?;

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let modified = entry.metadata()?.modified()?;
        if modified > newest {
            newest = modified;
        }
    }

    Ok(newest)
}

pub fn all_platforms() -> Vec<Platform> {
    vec![Platform::Douyin, Platform::Kuaishou, Platform::Twitter]
}

pub fn platform_only(platform: Platform) -> Vec<Platform> {
    vec![platform]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{is_empty_dir, is_too_recent, temp_dir_to_mp4_name};

    #[test]
    fn converts_temp_record_dir_to_mp4_name() {
        let mp4_name = temp_dir_to_mp4_name("temp_record_1000000000001_room")
            .expect("valid temp dir should convert");

        assert_eq!(mp4_name, "record_1000000000001_room.mp4");
    }

    #[test]
    fn rejects_unexpected_temp_dir_name() {
        let err = temp_dir_to_mp4_name("record_1000000000001_room")
            .expect_err("non-temp name should be rejected");

        assert!(err.to_string().contains("临时目录命名不合法"));
    }

    #[test]
    fn treats_new_temp_dir_as_too_recent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dytl_rescue_recent_{unique}"));
        fs::create_dir(&dir).expect("temp dir should be created");

        let result = is_too_recent(&dir, Duration::from_secs(60));
        fs::remove_dir_all(&dir).expect("temp dir should be removed");

        assert!(result.expect("recent check should work"));
    }

    #[test]
    fn zero_min_age_never_treats_dir_as_recent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dytl_rescue_zero_age_{unique}"));
        fs::create_dir(&dir).expect("temp dir should be created");

        let result = is_too_recent(&dir, Duration::ZERO);
        fs::remove_dir_all(&dir).expect("temp dir should be removed");

        assert!(!result.expect("recent check should work"));
    }

    #[test]
    fn detects_empty_temp_dir() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dytl_rescue_empty_dir_{unique}"));
        fs::create_dir(&dir).expect("temp dir should be created");

        assert!(is_empty_dir(&dir).expect("empty dir check should work"));

        fs::write(dir.join("slice_0000.ts"), b"data").expect("file should be written");
        assert!(!is_empty_dir(&dir).expect("non-empty dir check should work"));

        fs::remove_dir_all(&dir).expect("temp dir should be removed");
    }
}
