//! Local preview playback via ffplay.

use std::process::{Child, Command, Stdio};

use crate::core::error::{AppError, AppResult};
use crate::core::logger;

use super::util::{build_ffplay_args, is_command_available};

pub fn is_ffplay_available() -> bool {
    is_command_available("ffplay")
}

pub fn spawn_player(url: &str, douyin_cookie: Option<&str>) -> AppResult<Child> {
    if !is_ffplay_available() {
        return Err(AppError::new("当前系统未检测到 ffplay，无法播放。"));
    }

    logger::info("使用 ffplay 进行预览播放...");

    let args = build_ffplay_args(url, douyin_cookie);
    Command::new("ffplay")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(AppError::from)
}
