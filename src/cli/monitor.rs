use crate::config::load_config;
use crate::core::error::AppResult;
use crate::monitor;

pub fn run(args: &[String]) -> AppResult<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        print_help();
        return Ok(());
    }

    monitor::run(load_config()?)
}

fn print_help() {
    println!();
    println!("用法: dytl [--config PATH] monitor");
    println!(
        "功能: 轮询配置文件中的 monitor.targets，统一监控抖音/快手/Twitter 账号并在开播时自动录制"
    );
    println!("别名: dytl douyin monitor");
    println!();
}
