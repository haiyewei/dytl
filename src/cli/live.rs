use std::fs;

use crate::config::{Platform, load_config};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::media::ffmpeg;
use crate::media::live_stream::{self, LiveStreamRunOptions};
use crate::platform::DouyinClient;

use super::common::{self, LiveCommandArgs};

pub fn run(args: &[String]) -> AppResult<()> {
    let parsed = parse_args(args)?;
    let output_dir = paths::live_dir(Platform::Douyin);
    fs::create_dir_all(&output_dir)?;

    logger::info(format!(
        "正在获取直播间信息: web_rid={} | 录制: {} | 播放: {}",
        parsed.target,
        if parsed.record { "ON" } else { "OFF" },
        if parsed.play { "ON" } else { "OFF" }
    ));

    let config = load_config()?;
    let douyin_cookie = config
        .douyin
        .ok_or_else(|| AppError::new("未配置 douyin.cookies，无法使用抖音命令"))?
        .cookies;
    let client = DouyinClient::new(douyin_cookie.clone());

    let mut room_data = client.fetch_live_room_info("1", &parsed.target)?;
    if let Some(true_room_id) = client.extract_true_room_id(&room_data)
        && true_room_id != "0"
        && true_room_id != "1"
    {
        logger::info(format!(
            "获取到真实 room_id={true_room_id}，正在获取完整直播流数据..."
        ));
        if let Ok(complete) = client.fetch_live_room_info(&true_room_id, &parsed.target) {
            room_data = complete;
        }
    }

    logger::success("获取直播间数据成功!");

    if !parsed.record && !parsed.play {
        let file_path = output_dir.join(format!("room_{}.json", parsed.target));
        common::write_pretty_json(&file_path, &room_data)?;
        logger::success(format!(
            "未指定 -r 或 -p 参数。数据已保存至 {}",
            file_path.display()
        ));
        return Ok(());
    }

    let record_url = if parsed.record {
        logger::info("正在解析原画 FLV 直播地址...");
        let url = client.extract_best_flv_url(&room_data).ok_or_else(|| {
            AppError::new("无法提取出可用的 FLV 直播流链接，主播可能未开播或者数据结构已更改。")
        })?;
        logger::success(format!(
            "找到 FLV 原画流地址: {}...",
            common::preview(&url, 100)
        ));
        Some(url)
    } else {
        None
    };

    let play_url = if parsed.play {
        logger::info("正在解析原画 HLS 直播地址...");
        let url = client.extract_best_hls_url(&room_data).ok_or_else(|| {
            AppError::new("无法提取出可用的 HLS 直播流链接，主播可能未开播或者数据结构已更改。")
        })?;
        logger::success(format!(
            "找到 HLS 原画流地址: {}...",
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
        cookie: Some(&douyin_cookie),
        stop_file: parsed.stop_file.as_deref(),
        shutdown_message: "检测到退出指令，等待流安全断开并准备写入 MP4 容器，请耐心等待...",
        record_url: record_url.as_deref(),
        play_url: play_url.as_deref(),
    })
    .map(|_| ())
}

fn parse_args(args: &[String]) -> AppResult<LiveCommandArgs> {
    common::parse_live_args(
        args,
        "参数错误: 必须提供直播间链接或 web_rid",
        true,
        print_help,
        extract_web_rid,
    )
}

fn extract_web_rid(input: &str) -> String {
    common::extract_path_segment(input, &["live.douyin.com/"])
}

fn print_help() {
    let player_status = if ffmpeg::is_ffplay_available() {
        "ffplay".to_string()
    } else {
        "(未检测到 ffplay)".to_string()
    };

    println!();
    println!("用法: dytl douyin live <url 或 web_rid> [-r] [-p]");
    println!();
    println!("参数:");
    println!("  -r, --record         录制直播流");
    println!("  -p, --play           使用 ffplay 预览播放直播流");
    println!();
    println!("可用播放器: {player_status}");
    println!("固定播放器: ffplay");
    println!();
}
