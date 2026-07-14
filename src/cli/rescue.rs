use std::time::Duration;

use crate::config::Platform;
use crate::core::error::AppResult;
use crate::core::logger;
use crate::media::rescue::{self, RescueOptions};

pub fn run(args: &[String]) -> AppResult<()> {
    run_for_platforms(args, rescue::all_platforms(), "手动巡航封装")
}

pub fn run_douyin(args: &[String]) -> AppResult<()> {
    run_for_platforms(
        args,
        rescue::platform_only(Platform::Douyin),
        "抖音手动巡航封装",
    )
}

pub fn run_kuaishou(args: &[String]) -> AppResult<()> {
    run_for_platforms(
        args,
        rescue::platform_only(Platform::Kuaishou),
        "快手手动巡航封装",
    )
}

pub fn run_twitter(args: &[String]) -> AppResult<()> {
    run_for_platforms(
        args,
        rescue::platform_only(Platform::Twitter),
        "Twitter/X 手动巡航封装",
    )
}

fn run_for_platforms(
    args: &[String],
    platforms: Vec<Platform>,
    label: &'static str,
) -> AppResult<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        print_help();
        return Ok(());
    }

    let summary = rescue::run(RescueOptions {
        platforms,
        min_age: Duration::ZERO,
        label,
    })?;

    if !summary.handled_anything() {
        logger::info("未发现需要修复的临时文件夹。");
    } else {
        logger::success_persist("所有修复任务已处理完毕。");
    }

    Ok(())
}

fn print_help() {
    println!();
    println!("用法:");
    println!("  dytl rescue");
    println!("  dytl douyin rescue");
    println!("  dytl kuaishou rescue");
    println!("  dytl twitter rescue");
    println!();
    println!("功能: 扫描直播录制目录下的 temp_record_* 目录并执行 FFmpeg 合并");
    println!();
}
