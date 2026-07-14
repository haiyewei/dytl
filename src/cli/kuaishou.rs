use std::fs;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::config::{Platform, load_config};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::media::live_stream::{self, LiveStreamRunOptions};
use crate::platform::KuaishouClient;

use super::common::{self, LiveCommandArgs, UserCommandArgs};
use super::rescue;

pub fn run(args: &[String]) -> AppResult<()> {
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help") {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "live" => run_live(&args[1..]),
        "user" => run_user(&args[1..]),
        "rescue" => rescue::run_kuaishou(&args[1..]),
        command => Err(AppError::new(format!("未知的快手子命令: {command}"))),
    }
}

fn run_live(args: &[String]) -> AppResult<()> {
    let parsed = parse_live_args(args)?;
    let output_dir = paths::live_dir(Platform::Kuaishou);
    fs::create_dir_all(&output_dir)?;

    logger::info(format!(
        "正在获取快手直播间信息: principal_id={} | 录制: {} | 播放: {}",
        parsed.target,
        if parsed.record { "ON" } else { "OFF" },
        if parsed.play { "ON" } else { "OFF" }
    ));

    let config = load_config()?;
    let kuaishou_cookie = config
        .kuaishou
        .ok_or_else(|| AppError::new("未配置 kuaishou.cookies，无法使用快手命令"))?
        .cookies;
    let client = KuaishouClient::new(kuaishou_cookie.clone());
    let mut room_data = client.fetch_live_room_info(&parsed.target)?;
    logger::success("获取快手直播间数据成功!");
    if client.is_anti_crawl_blocked(&room_data) {
        return Err(AppError::new(
            "快手接口触发风控或验证，当前响应无法作为直播状态使用",
        ));
    }

    if !parsed.record && !parsed.play {
        let file_path = output_dir.join(format!("room_{}.json", parsed.target));
        common::write_pretty_json(&file_path, &room_data)?;
        logger::success_persist(format!(
            "未指定 -r 或 -p 参数。数据已保存至 {}",
            file_path.display()
        ));
        return Ok(());
    }

    if !client.has_active_live_room_for_principal(&room_data, &parsed.target) {
        return Err(AppError::new(
            "快手当前未开播，或 live-room-info 未返回 current.liveStream / current.config 中的直播资源",
        ));
    }

    room_data = wait_for_kuaishou_stream_urls(
        &client,
        &parsed.target,
        room_data,
        parsed.record,
        parsed.play,
    )?;

    let record_url = if parsed.record {
        let url = client
            .extract_best_flv_url_for_principal(&room_data, &parsed.target)
            .or_else(|| client.extract_best_hls_url_for_principal(&room_data, &parsed.target))
            .ok_or_else(|| AppError::new("无法从快手直播间数据中提取可用的录制流地址"))?;
        logger::success(format!(
            "找到快手录制流地址: {}...",
            common::preview(&url, 100)
        ));
        Some(url)
    } else {
        None
    };

    let play_url = if parsed.play {
        let url = client
            .extract_best_play_url_for_principal(&room_data, &parsed.target)
            .ok_or_else(|| AppError::new("无法从快手直播间数据中提取可用的播放流地址"))?;
        logger::success(format!(
            "找到快手播放流地址: {}...",
            common::preview(&url, 100)
        ));
        Some(url)
    } else {
        None
    };

    live_stream::run(LiveStreamRunOptions {
        record: parsed.record,
        stream_id: &parsed.target,
        output_dir: &output_dir,
        cookie: Some(&kuaishou_cookie),
        stop_file: parsed.stop_file.as_deref(),
        shutdown_message: "检测到退出指令，等待快手录制安全断开并准备写入 MP4 容器，请耐心等待...",
        record_url: record_url.as_deref(),
        play_url: play_url.as_deref(),
    })
    .map(|_| ())
}

fn wait_for_kuaishou_stream_urls(
    client: &KuaishouClient,
    principal_id: &str,
    mut room_data: Value,
    wants_record: bool,
    wants_play: bool,
) -> AppResult<Value> {
    if has_requested_stream_urls(client, principal_id, &room_data, wants_record, wants_play) {
        return Ok(room_data);
    }

    if !client.indicates_principal_living(&room_data, principal_id) {
        return Ok(room_data);
    }

    for attempt in 1..=3 {
        logger::warn(format!(
            "快手已显示开播，但直播流地址尚未就绪，{} 秒后重试 ({attempt}/3)...",
            3
        ));
        thread::sleep(Duration::from_secs(3));

        room_data = client.fetch_live_room_info(principal_id)?;
        if client.is_anti_crawl_blocked(&room_data) {
            return Err(AppError::new(
                "快手接口触发风控或验证，当前响应无法作为直播状态使用",
            ));
        }

        if has_requested_stream_urls(client, principal_id, &room_data, wants_record, wants_play) {
            logger::success("快手直播流地址已就绪。");
            return Ok(room_data);
        }
    }

    if client.has_recommended_stream_urls(&room_data) {
        Err(AppError::new(
            "快手显示目标账号正在直播，但 live-room-info 仅在推荐列表返回直播流；已忽略推荐流，未找到该账号可用直播流",
        ))
    } else {
        Err(AppError::new(
            "快手显示目标账号正在直播，但 live-room-info 未返回该账号可用直播流",
        ))
    }
}

fn has_requested_stream_urls(
    client: &KuaishouClient,
    principal_id: &str,
    room_data: &Value,
    wants_record: bool,
    wants_play: bool,
) -> bool {
    let has_record_url = !wants_record
        || client
            .extract_best_flv_url_for_principal(room_data, principal_id)
            .is_some()
        || client
            .extract_best_hls_url_for_principal(room_data, principal_id)
            .is_some();
    let has_play_url = !wants_play
        || client
            .extract_best_play_url_for_principal(room_data, principal_id)
            .is_some();

    has_record_url && has_play_url
}

fn run_user(args: &[String]) -> AppResult<()> {
    let parsed = parse_user_args(args)?;
    let output_dir = paths::user_dir(Platform::Kuaishou);
    fs::create_dir_all(&output_dir)?;

    let config = load_config()?;
    let kuaishou_cookie = config
        .kuaishou
        .ok_or_else(|| AppError::new("未配置 kuaishou.cookies，无法使用快手命令"))?
        .cookies;
    let client = KuaishouClient::new(kuaishou_cookie);

    logger::info(format!(
        "正在获取快手用户主页数据: principal_id={}",
        parsed.target
    ));
    let profile = client.fetch_user_profile(&parsed.target)?;
    logger::success("获取快手用户主页数据成功!");

    let profile_file = output_dir.join(format!("profile_{}.json", parsed.target));
    common::write_pretty_json(&profile_file, &profile)?;
    logger::success_persist(format!(
        "快手用户主页数据已保存至 {}",
        profile_file.display()
    ));

    if parsed.fetch_extra {
        logger::info("正在获取快手用户作品列表...");
        let works = client.fetch_user_work_list(&parsed.target, None, None)?;
        logger::success("获取快手用户作品列表成功!");

        let works_file = output_dir.join(format!("works_{}.json", parsed.target));
        common::write_pretty_json(&works_file, &works)?;
        logger::success_persist(format!("快手用户作品列表已保存至 {}", works_file.display()));
    }

    Ok(())
}

fn parse_live_args(args: &[String]) -> AppResult<LiveCommandArgs> {
    common::parse_live_args(
        args,
        "参数错误: 必须提供快手 principal_id 或链接",
        false,
        print_live_help,
        extract_principal_id,
    )
}

fn parse_user_args(args: &[String]) -> AppResult<UserCommandArgs> {
    common::parse_user_args(
        args,
        "-w",
        "--works",
        "参数错误: 必须提供快手 principal_id 或链接",
        print_user_help,
        extract_principal_id,
    )
}

fn extract_principal_id(input: &str) -> String {
    common::extract_path_segment(
        input,
        &[
            "kuaishou.com/profile/",
            "live.kuaishou.com/u/",
            "kuaishou.com/u/",
        ],
    )
}

fn print_help() {
    println!();
    println!("用法: dytl kuaishou <子命令> [参数...]");
    println!();
    println!("可用子命令:");
    println!("  live         快手直播间操作（获取信息、录制、播放）");
    println!("  user         获取快手用户主页数据和作品列表");
    println!("  rescue       手动修复/合并快手异常中断留下的临时分片文件夹");
    println!("  说明         统一监控入口请使用 dytl douyin monitor");
    println!();
}

fn print_live_help() {
    println!();
    println!("用法: dytl kuaishou live <principal_id 或 url> [-r] [-p]");
    println!();
    println!("参数:");
    println!("  -r, --record         录制快手直播流");
    println!("  -p, --play           使用 ffplay 预览播放快手直播流");
    println!();
}

fn print_user_help() {
    println!();
    println!("用法: dytl kuaishou user <principal_id 或 url> [--works]");
    println!();
    println!("参数:");
    println!("  -w, --works          同时获取快手用户作品列表");
    println!();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{KuaishouClient, has_requested_stream_urls};

    #[test]
    fn requested_stream_url_check_matches_record_and_play_modes() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "current": {
                "author": {
                    "id": "test_ks_user_a"
                },
                "config": {
                    "hlsPlayUrl": "https://example.com/live.m3u8",
                    "multiResolutionPlayUrls": {
                        "h264": {
                            "adaptationSet": {
                                "representation": [{
                                    "url": "https://example.com/live.flv",
                                    "level": 50,
                                    "bitrate": 2000
                                }]
                            }
                        }
                    }
                }
            }
        });

        assert!(has_requested_stream_urls(
            &client,
            "test_ks_user_a",
            &room_data,
            true,
            false
        ));
        assert!(has_requested_stream_urls(
            &client,
            "test_ks_user_a",
            &room_data,
            false,
            true
        ));
        assert!(has_requested_stream_urls(
            &client,
            "test_ks_user_a",
            &room_data,
            true,
            true
        ));
        assert!(!has_requested_stream_urls(
            &client, "other", &room_data, true, true
        ));
    }
}
