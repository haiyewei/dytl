use std::fs;

use crate::config::{Platform, load_config};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::platform::DouyinClient;

use super::common::{self, UserCommandArgs};

pub fn run(args: &[String]) -> AppResult<()> {
    let parsed = parse_args(args)?;
    let output_dir = paths::user_dir(Platform::Douyin);
    fs::create_dir_all(&output_dir)?;

    let config = load_config()?;
    let douyin_cookie = config
        .douyin
        .ok_or_else(|| AppError::new("未配置 douyin.cookies，无法使用抖音命令"))?
        .cookies;
    let client = DouyinClient::new(douyin_cookie);

    logger::info(format!("正在获取用户主页数据: sec_uid={}", parsed.target));
    let profile = client.fetch_user_profile(&parsed.target)?;
    logger::success("获取用户主页数据成功!");

    let profile_file = output_dir.join(format!("profile_{}.json", parsed.target));
    common::write_pretty_json(&profile_file, &profile)?;
    logger::success_persist(format!("用户主页数据已保存至 {}", profile_file.display()));

    if parsed.fetch_extra {
        logger::info("正在获取用户视频列表...");
        let videos = client.fetch_user_video_list(&parsed.target, None, None)?;
        logger::success("获取用户视频列表成功!");

        let video_file = output_dir.join(format!("videos_{}.json", parsed.target));
        common::write_pretty_json(&video_file, &videos)?;
        logger::success_persist(format!("用户视频列表已保存至 {}", video_file.display()));
    }

    Ok(())
}

fn parse_args(args: &[String]) -> AppResult<UserCommandArgs> {
    common::parse_user_args(
        args,
        "-v",
        "--videos",
        "参数错误: 必须提供用户 sec_uid 或主页链接",
        print_help,
        extract_sec_uid,
    )
}

fn extract_sec_uid(input: &str) -> String {
    common::extract_path_segment(input, &["douyin.com/user/"])
}

fn print_help() {
    println!();
    println!("用法: dytl douyin user <sec_uid 或 url> [--videos]");
    println!();
    println!("参数:");
    println!("  sec_uid / url         用户的 sec_uid 或主页 URL");
    println!("  -v, --videos          同时获取用户视频列表");
    println!();
}
