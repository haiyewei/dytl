use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::error::{AppError, AppResult};

#[derive(Debug)]
pub struct LiveCommandArgs {
    pub record: bool,
    pub play: bool,
    pub target: String,
    pub stop_file: Option<PathBuf>,
}

#[derive(Debug)]
pub struct UserCommandArgs {
    pub target: String,
    pub fetch_extra: bool,
}

pub fn parse_live_args(
    args: &[String],
    missing_target_error: &'static str,
    reject_client_flag: bool,
    print_help: fn(),
    normalize_target: impl Fn(&str) -> String,
) -> AppResult<LiveCommandArgs> {
    let mut record = false;
    let mut play = false;
    let mut stop_file = None;
    let mut positional = Vec::new();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-r" | "--record" => record = true,
            "-p" | "--play" => play = true,
            "-c" | "--client" if reject_client_flag => {
                return Err(AppError::new(
                    "播放器已固定为 ffplay，不再支持 -c/--client 参数",
                ));
            }
            "--stop-file" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| AppError::new("--stop-file 参数需要指定路径"))?;
                stop_file = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => positional.push(other.to_string()),
        }
        index += 1;
    }

    if positional.is_empty() {
        print_help();
        return Err(AppError::new(missing_target_error));
    }

    Ok(LiveCommandArgs {
        record,
        play,
        target: normalize_target(&positional[0]),
        stop_file,
    })
}

pub fn parse_user_args(
    args: &[String],
    extra_short_flag: &'static str,
    extra_long_flag: &'static str,
    missing_target_error: &'static str,
    print_help: fn(),
    normalize_target: impl Fn(&str) -> String,
) -> AppResult<UserCommandArgs> {
    let mut fetch_extra = false;
    let mut positional = Vec::new();

    for arg in args {
        if arg == extra_short_flag || arg == extra_long_flag {
            fetch_extra = true;
        } else if matches!(arg.as_str(), "-h" | "--help") {
            print_help();
            std::process::exit(0);
        } else {
            positional.push(arg.to_string());
        }
    }

    if positional.is_empty() {
        print_help();
        return Err(AppError::new(missing_target_error));
    }

    Ok(UserCommandArgs {
        target: normalize_target(&positional[0]),
        fetch_extra,
    })
}

pub fn extract_path_segment(input: &str, markers: &[&str]) -> String {
    for marker in markers {
        if let Some(index) = input.find(marker) {
            let part = &input[index + marker.len()..];
            return part
                .split(['?', '#', '/'])
                .next()
                .unwrap_or(input)
                .to_string();
        }
    }

    input.to_string()
}

pub fn preview(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect::<String>()
}

pub fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{extract_path_segment, parse_live_args, parse_user_args};

    fn print_help() {}

    #[test]
    fn parses_shared_live_args() {
        let parsed = parse_live_args(
            &[
                "-r".to_string(),
                "--stop-file".to_string(),
                "/tmp/stop.flag".to_string(),
                "https://live.douyin.com/100000000001?foo=bar".to_string(),
            ],
            "missing target",
            true,
            print_help,
            |input| extract_path_segment(input, &["live.douyin.com/"]),
        )
        .expect("live args should parse");

        assert!(parsed.record);
        assert!(!parsed.play);
        assert_eq!(parsed.target, "100000000001");
        assert_eq!(parsed.stop_file, Some(PathBuf::from("/tmp/stop.flag")));
    }

    #[test]
    fn rejects_legacy_client_flag_when_requested() {
        let error = parse_live_args(
            &["-c".to_string(), "mpv".to_string(), "room".to_string()],
            "missing target",
            true,
            print_help,
            str::to_string,
        )
        .expect_err("legacy client flag should fail");

        assert!(error.to_string().contains("ffplay"));
    }

    #[test]
    fn parses_shared_user_args() {
        let parsed = parse_user_args(
            &[
                "--videos".to_string(),
                "https://www.douyin.com/user/test_douyin_sec_uid?modal=1".to_string(),
            ],
            "-v",
            "--videos",
            "missing user",
            print_help,
            |input| extract_path_segment(input, &["douyin.com/user/"]),
        )
        .expect("user args should parse");

        assert!(parsed.fetch_extra);
        assert_eq!(parsed.target, "test_douyin_sec_uid");
    }
}
