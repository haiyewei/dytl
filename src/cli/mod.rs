//! Command-line interface: global flags and subcommand routing.

pub mod common;
pub mod douyin;
pub mod kuaishou;
pub mod live;
pub mod monitor;
pub mod rescue;
pub mod twitter;
pub mod user;

use std::path::PathBuf;

use crate::config::{set_config_path, try_init_logging_from_current_config};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;

pub fn run(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_global_args(argv.into_iter().skip(1).collect())?;
    if let Some(config_path) = parsed.config_path {
        set_config_path(config_path)?;
    }
    try_init_logging_from_current_config()?;

    let args = parsed.command_args;

    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help") {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "monitor" => monitor::run(&args[1..]),
        "rescue" => rescue::run(&args[1..]),
        "douyin" | "dy" => douyin::run(&args[1..]),
        "kuaishou" | "ks" => kuaishou::run(&args[1..]),
        "twitter" | "x" => twitter::run(&args[1..]),
        "live" | "user" => Err(AppError::new(format!(
            "旧的根级用法已移除，请改用 dytl douyin {}",
            args[0]
        ))),
        command => {
            logger::error(format!("未知命令: {command}"));
            print_help();
            Err(AppError::new("unknown command"))
        }
    }
}

pub fn print_help() {
    println!();
    println!("多平台直播工具箱 - DYTL CLI (Cargo)");
    println!();
    println!("用法: dytl [--config PATH] <命令> [参数...]");
    println!();
    println!("全局参数:");
    println!("  --config PATH  指定配置文件路径，默认读取当前目录下的 config.yaml");
    println!();
    println!("根级命令:");
    println!("  monitor      统一监控配置中的抖音/快手/Twitter 账号并自动录制");
    println!("  rescue       手动修复/合并抖音、快手和 Twitter 异常中断留下的临时分片文件夹");
    println!();
    println!("平台命令:");
    println!("  douyin       抖音能力集合（live / user / monitor / rescue）");
    println!("  kuaishou     快手能力集合（live / user）");
    println!("  twitter      Twitter/X 能力集合（live / download / user）");
    println!();
    println!("示例:");
    println!("  dytl monitor");
    println!("  dytl rescue");
    println!("  dytl douyin live https://live.douyin.com/test_web_rid");
    println!("  dytl douyin user test_douyin_sec_uid");
    println!("  dytl douyin monitor");
    println!("  dytl douyin rescue");
    println!("  dytl kuaishou user test_ks_account");
    println!("  dytl kuaishou live -p test_ks_account");
    println!("  dytl twitter live -p https://x.com/test_screen_user");
    println!("  dytl twitter live -r https://x.com/i/broadcasts/1TestBroadcastAA");
    println!("  dytl twitter download https://x.com/i/broadcasts/1TestBroadcastAA");
    println!("  dytl --config /home/app/config.yaml monitor");
    println!();
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedGlobalArgs {
    config_path: Option<PathBuf>,
    command_args: Vec<String>,
}

fn parse_global_args(args: Vec<String>) -> AppResult<ParsedGlobalArgs> {
    let mut config_path = None;
    let mut command_args = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == "--config" {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| AppError::new("--config 参数需要指定文件路径"))?;
            if config_path.is_some() {
                return Err(AppError::new("全局参数 --config 只能指定一次"));
            }
            config_path = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--config=") {
            if value.trim().is_empty() {
                return Err(AppError::new("--config 参数需要指定文件路径"));
            }
            if config_path.is_some() {
                return Err(AppError::new("全局参数 --config 只能指定一次"));
            }
            config_path = Some(PathBuf::from(value));
        } else {
            command_args.push(arg.clone());
        }
        index += 1;
    }

    Ok(ParsedGlobalArgs {
        config_path,
        command_args,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse_global_args;

    #[test]
    fn parses_global_config_before_command() {
        let parsed = parse_global_args(vec![
            "--config".to_string(),
            "/tmp/dytl.yaml".to_string(),
            "monitor".to_string(),
        ])
        .expect("global config should parse");

        assert_eq!(parsed.config_path, Some(PathBuf::from("/tmp/dytl.yaml")));
        assert_eq!(parsed.command_args, vec!["monitor"]);
    }

    #[test]
    fn parses_global_config_after_command() {
        let parsed = parse_global_args(vec![
            "monitor".to_string(),
            "--config=/tmp/dytl.yaml".to_string(),
        ])
        .expect("global config should parse");

        assert_eq!(parsed.config_path, Some(PathBuf::from("/tmp/dytl.yaml")));
        assert_eq!(parsed.command_args, vec!["monitor"]);
    }

    #[test]
    fn rejects_duplicate_global_config() {
        let error = parse_global_args(vec![
            "--config".to_string(),
            "/tmp/a.yaml".to_string(),
            "--config=/tmp/b.yaml".to_string(),
            "monitor".to_string(),
        ])
        .expect_err("duplicate global config should fail");

        assert!(error.to_string().contains("只能指定一次"));
    }
}
