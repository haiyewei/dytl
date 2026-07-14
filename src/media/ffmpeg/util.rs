//! Shared helpers for ffmpeg subprocesses and temp files.

use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};

use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::time::{self, TimeParts};

pub(crate) fn release_file_cache(path: &Path) -> AppResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(AppError::from(err)),
    };

    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }

    Ok(())
}

pub(crate) fn release_recording_file_cache(
    dir_path: &Path,
    stop_file: Option<&Path>,
    remove_temp_dir: bool,
) {
    if remove_temp_dir {
        match release_file_cache(dir_path) {
            Ok(()) => {
                logger::success_persist(format!("已自动清理缓存文件夹: {}", dir_path.display()))
            }
            Err(err) => logger::warn_persist(format!(
                "删除缓存文件夹失败: {} ({err})",
                dir_path.display()
            )),
        }
    }

    if let Some(path) = stop_file
        && let Err(err) = release_file_cache(path)
    {
        logger::warn_persist(format!("删除停止标记失败: {} ({err})", path.display()));
    }
}

pub(crate) fn absolutize_path(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub(crate) fn send_ffmpeg_stop(stdin: Option<&mut ChildStdin>) {
    if let Some(stdin) = stdin {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }
}

pub(crate) fn stop_requested(stop_file: Option<&Path>) -> bool {
    stop_file.is_some_and(Path::exists)
}

pub(crate) fn build_ffplay_args(url: &str, douyin_cookie: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    append_http_cookie_args(&mut args, douyin_cookie);
    args.extend([
        "-i".to_string(),
        url.to_string(),
        "-window_title".to_string(),
        "Live Stream".to_string(),
        "-x".to_string(),
        "720".to_string(),
        "-y".to_string(),
        "1280".to_string(),
        "-autoexit".to_string(),
    ]);
    args
}

pub(crate) fn append_http_cookie_args(args: &mut Vec<String>, douyin_cookie: Option<&str>) {
    let Some(cookie) = douyin_cookie
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    args.extend(["-headers".to_string(), format!("Cookie: {cookie}\r\n")]);
}

pub(crate) fn is_command_available(command: &str) -> bool {
    let checker = if cfg!(windows) { "where" } else { "which" };
    Command::new(checker)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn current_epoch_millis() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn build_record_base_name(room_id: &str) -> String {
    let timestamp_millis = current_epoch_millis();
    let now = time::current_time_parts();
    format_record_base_name(timestamp_millis, now, room_id)
}

pub(crate) fn format_record_base_name(
    timestamp_millis: u128,
    now: TimeParts,
    room_id: &str,
) -> String {
    let room_id = sanitize_file_component(room_id);
    format!(
        "record_{}_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{}",
        timestamp_millis, now.year, now.month, now.day, now.hour, now.minute, now.second, room_id
    )
}

pub(crate) fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        TimeParts, build_record_base_name, format_record_base_name, sanitize_file_component,
    };

    #[test]
    fn sanitizes_file_component() {
        assert_eq!(sanitize_file_component("room:397/abc"), "room_397_abc");
    }

    #[test]
    fn formats_record_base_name_with_epoch_millis_and_system_time() {
        let parts = TimeParts {
            year: 2026,
            month: 5,
            day: 12,
            hour: 16,
            minute: 31,
            second: 45,
        };

        assert_eq!(
            format_record_base_name(1000000000001, parts, "test_room_id_001"),
            "record_1000000000001_2026-05-12_16-31-45_test_room_id_001"
        );
    }

    #[test]
    fn record_base_name_contains_expected_sections() {
        let name = build_record_base_name("test_room_id_001");

        assert!(name.starts_with("record_"));
        assert!(name.ends_with("_test_room_id_001"));
        assert!(!name.contains('.'));
    }
}
