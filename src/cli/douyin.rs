use crate::core::error::{AppError, AppResult};

use super::{live, monitor, rescue, user};

pub fn run(args: &[String]) -> AppResult<()> {
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help") {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "live" => live::run(&args[1..]),
        "user" => user::run(&args[1..]),
        "monitor" => monitor::run(&args[1..]),
        "rescue" => rescue::run_douyin(&args[1..]),
        command => Err(AppError::new(format!("未知的抖音子命令: {command}"))),
    }
}

fn print_help() {
    println!();
    println!("用法: dytl douyin <子命令> [参数...]");
    println!();
    println!("可用子命令:");
    println!("  live         抖音直播间操作（获取信息、录制、播放）");
    println!("  user         获取抖音用户主页数据和视频列表");
    println!("  monitor      统一监控别名，等价于 dytl monitor");
    println!("  rescue       手动修复/合并意外中断留下的临时分片文件夹");
    println!();
}
