use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::{Platform, load_config};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::media::ffmpeg;
use crate::media::live_stream::{self, LiveStreamRunOptions};
use crate::platform::{HlsMediaPlaylist, TwitterClient};

use super::common::{self, LiveCommandArgs};
use super::rescue;

const POST_RECORD_REPLAY_ATTEMPTS: usize = 5;
const POST_RECORD_REPLAY_RETRY_DELAY: Duration = Duration::from_secs(30);
const REPLAY_REPLACE_MIN_EXTRA_SECONDS: f64 = 1.0;

pub fn run(args: &[String]) -> AppResult<()> {
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help") {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "live" => run_live(&args[1..]),
        "download" => run_download(&args[1..]),
        "user" => run_user(&args[1..]),
        "rescue" => rescue::run_twitter(&args[1..]),
        command => Err(AppError::new(format!("未知的 Twitter/X 子命令: {command}"))),
    }
}

fn run_live(args: &[String]) -> AppResult<()> {
    let parsed = parse_live_args(args)?;
    let output_dir = paths::live_dir(Platform::Twitter);
    fs::create_dir_all(&output_dir)?;

    logger::info(format!(
        "正在获取 Twitter/X 直播信息: target={} | 录制: {} | 播放: {}",
        parsed.target,
        if parsed.record { "ON" } else { "OFF" },
        if parsed.play { "ON" } else { "OFF" }
    ));

    let config = load_config()?;
    let twitter_cookie = config
        .twitter
        .ok_or_else(|| AppError::new("未配置 twitter.cookies，无法使用 Twitter/X 命令"))?
        .cookies;
    let client = TwitterClient::new(twitter_cookie.clone());

    let (stream_id, stream_data) = match parse_twitter_live_target(&parsed.target) {
        TwitterLiveTarget::BroadcastId(broadcast_id) => {
            let stream_data = client.fetch_live_room_stream(&broadcast_id)?;
            (broadcast_id, stream_data)
        }
        TwitterLiveTarget::ScreenName(screen_name) => {
            let room_data = client.fetch_live_room_info(&screen_name)?;
            let info = client.extract_live_info(&room_data);
            if !info.is_live {
                let name = info.screen_name.as_deref().unwrap_or(&screen_name);
                return Err(AppError::new(format!("Twitter/X 账号 {name} 当前未开播")));
            }
            let broadcast_id = info.broadcast_id.ok_or_else(|| {
                AppError::new("Twitter/X live-room-info 未返回 broadcast_id，无法解析直播流")
            })?;
            logger::success(format!(
                "检测到 Twitter/X 正在直播: {}",
                info.title.as_deref().unwrap_or(&broadcast_id)
            ));
            let stream_data = client.fetch_live_room_stream(&broadcast_id)?;
            (broadcast_id, stream_data)
        }
    };

    logger::success("获取 Twitter/X 直播流数据成功!");

    if !parsed.record && !parsed.play {
        let file_path = output_dir.join(format!("room_{}.json", stream_id));
        common::write_pretty_json(&file_path, &stream_data)?;
        logger::success_persist(format!(
            "未指定 -r 或 -p 参数。数据已保存至 {}",
            file_path.display()
        ));
        return Ok(());
    }

    let master_url = client
        .extract_hls_url(&stream_data)
        .ok_or_else(|| AppError::new("无法从 Twitter/X 响应中提取 HLS master 地址"))?;
    let stream_url = match client.resolve_best_hls_url(&master_url) {
        Ok(url) => url,
        Err(err) => {
            logger::warn(format!(
                "解析 Twitter/X 最高画质 HLS variant 失败，将回退到 master: {err}"
            ));
            master_url
        }
    };
    logger::success(format!(
        "找到 Twitter/X HLS 最高画质流地址: {}...",
        common::preview(&stream_url, 100)
    ));

    let run_result = live_stream::run(LiveStreamRunOptions {
        record: parsed.record,
        stream_id: &stream_id,
        output_dir: &output_dir,
        cookie: Some(&twitter_cookie),
        stop_file: parsed.stop_file.as_deref(),
        shutdown_message: "检测到退出指令，等待 Twitter/X 录制安全断开并准备写入 MP4 容器，请耐心等待...",
        record_url: parsed.record.then_some(stream_url.as_str()),
        play_url: parsed.play.then_some(stream_url.as_str()),
    })?;

    if let Some(recording) = run_result.recording.as_ref() {
        if recording.end_reason == ffmpeg::RecordEndReason::ShutdownSignal {
            logger::info("录制由本地退出信号停止，跳过 Twitter/X 完整回放补全。");
        } else if let Err(err) = complete_recording_from_replay(
            &client,
            &twitter_cookie,
            &stream_id,
            &recording.output_path,
        ) {
            logger::warn_persist(format!("Twitter/X 完整回放补全失败，保留原录制文件: {err}"));
        }
    }

    Ok(())
}

fn run_download(args: &[String]) -> AppResult<()> {
    let parsed = parse_download_args(args)?;
    let broadcast_id = parse_twitter_broadcast_target(&parsed.target)?;
    let output_dir = paths::download_dir(Platform::Twitter);
    fs::create_dir_all(&output_dir)?;

    logger::info(format!(
        "正在获取 Twitter/X 回放下载信息: broadcast_id={broadcast_id}"
    ));

    let config = load_config()?;
    let twitter_cookie = config
        .twitter
        .ok_or_else(|| AppError::new("未配置 twitter.cookies，无法使用 Twitter/X 命令"))?
        .cookies;
    let client = TwitterClient::new(twitter_cookie.clone());
    let plan = build_twitter_replay_download_plan(&client, &broadcast_id)?;
    let output_path = resolve_download_output_path(
        parsed.output.as_deref(),
        &output_dir,
        &broadcast_id,
        plan.title.as_deref(),
    );
    let jobs = parsed.jobs.unwrap_or_else(default_download_jobs);
    download_twitter_replay_plan(&plan, &output_path, &broadcast_id, jobs, &twitter_cookie)
}

fn build_twitter_replay_download_plan(
    client: &TwitterClient,
    broadcast_id: &str,
) -> AppResult<TwitterReplayDownloadPlan> {
    let stream_data = client.fetch_live_room_stream(broadcast_id)?;
    let replay_info = client.extract_replay_info(&stream_data);

    let state = replay_info.state.as_deref().unwrap_or("UNKNOWN");
    logger::info(format!(
        "Twitter/X broadcast 状态: state={state} | replay={} | available_for_replay={}",
        replay_info.is_replay,
        replay_info
            .available_for_replay
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));

    if state.eq_ignore_ascii_case("RUNNING") && !replay_info.is_replay {
        return Err(AppError::new(
            "这场 Twitter/X 仍在直播中，请使用 dytl twitter live <broadcast> -r 进行直播录制",
        ));
    }

    if !replay_info.is_replay && !state.eq_ignore_ascii_case("ENDED") {
        return Err(AppError::new(format!(
            "当前 broadcast 不是可下载回放: state={state}, is_replay={}",
            replay_info.is_replay
        )));
    }

    if replay_info.available_for_replay == Some(false) {
        return Err(AppError::new(
            "这场 Twitter/X 直播已结束，但平台标记为不可回放",
        ));
    }

    let master_url = client
        .extract_hls_url(&stream_data)
        .ok_or_else(|| AppError::new("无法从 Twitter/X 响应中提取回放 HLS master 地址"))?;
    let stream_url = match client.resolve_best_hls_url(&master_url) {
        Ok(url) => url,
        Err(err) => {
            logger::warn(format!(
                "解析 Twitter/X 最高画质 HLS variant 失败，将回退到 master: {err}"
            ));
            master_url
        }
    };
    logger::success(format!(
        "找到 Twitter/X 回放最高画质 HLS 地址: {}...",
        common::preview(&stream_url, 100)
    ));

    let playlist = client.fetch_hls_playlist(&stream_url)?;
    let media_playlist = client.extract_hls_media_playlist(&playlist, &stream_url);
    if media_playlist.is_encrypted {
        return Err(AppError::new(
            "Twitter/X 回放 HLS 使用了加密分片，当前并发下载模式暂不支持",
        ));
    }
    if media_playlist.segments.is_empty() {
        return Err(AppError::new("Twitter/X 回放 HLS 清单没有返回任何媒体分片"));
    }
    if !media_playlist.has_endlist {
        return Err(AppError::new(
            "Twitter/X 回放 HLS 清单缺少 ENDLIST，暂不按并发分片下载处理",
        ));
    }

    Ok(TwitterReplayDownloadPlan {
        title: replay_info.title,
        media_playlist,
    })
}

fn download_twitter_replay_plan(
    plan: &TwitterReplayDownloadPlan,
    output_path: &Path,
    broadcast_id: &str,
    jobs: usize,
    twitter_cookie: &str,
) -> AppResult<()> {
    logger::info(format!(
        "Twitter/X 回放清单完整: 分片 {} 个，时长约 {:.1} 秒，下载并发 {}",
        plan.media_playlist.segments.len(),
        plan.media_playlist.duration_seconds,
        jobs
    ));
    let segment_urls = plan
        .media_playlist
        .segments
        .iter()
        .map(|segment| segment.url.clone())
        .collect::<Vec<_>>();
    let segment_durations = plan
        .media_playlist
        .segments
        .iter()
        .map(|segment| segment.duration_seconds)
        .collect::<Vec<_>>();

    ffmpeg::download_hls_segments_concurrent(
        &segment_urls,
        &segment_durations,
        output_path,
        broadcast_id,
        jobs,
        Some(twitter_cookie),
    )
}

fn complete_recording_from_replay(
    client: &TwitterClient,
    twitter_cookie: &str,
    broadcast_id: &str,
    recorded_path: &Path,
) -> AppResult<()> {
    logger::info(format!(
        "Twitter/X 直播录制已结束，开始尝试下载完整回放用于补全: {}",
        recorded_path.display()
    ));

    let candidate_path = replay_candidate_path(recorded_path);
    let jobs = default_download_jobs();
    let mut last_error = None;

    for attempt in 1..=POST_RECORD_REPLAY_ATTEMPTS {
        if attempt > 1 {
            logger::info(format!(
                "等待 Twitter/X 回放生成，{} 秒后重试 ({attempt}/{POST_RECORD_REPLAY_ATTEMPTS})...",
                POST_RECORD_REPLAY_RETRY_DELAY.as_secs()
            ));
            thread::sleep(POST_RECORD_REPLAY_RETRY_DELAY);
        }

        let _ = fs::remove_file(&candidate_path);
        let result = build_twitter_replay_download_plan(client, broadcast_id).and_then(|plan| {
            download_twitter_replay_plan(&plan, &candidate_path, broadcast_id, jobs, twitter_cookie)
        });

        match result {
            Ok(()) => {
                let replaced =
                    match replace_recording_with_longer_replay(recorded_path, &candidate_path) {
                        Ok(replaced) => replaced,
                        Err(err) => {
                            let _ = fs::remove_file(&candidate_path);
                            return Err(err);
                        }
                    };

                if replaced {
                    logger::success_persist(format!(
                        "已用 Twitter/X 完整最高画质回放替换原录制文件: {}",
                        recorded_path.display()
                    ));
                } else {
                    let _ = fs::remove_file(&candidate_path);
                    logger::warn_persist("Twitter/X 完整回放未比原录制更长，已保留原录制文件");
                }

                return Ok(());
            }
            Err(err) => {
                let _ = fs::remove_file(&candidate_path);
                logger::warn(format!(
                    "Twitter/X 完整回放补全尝试失败 ({attempt}/{POST_RECORD_REPLAY_ATTEMPTS}): {err}"
                ));
                last_error = Some(err);
            }
        }
    }

    Err(AppError::new(format!(
        "已重试 {POST_RECORD_REPLAY_ATTEMPTS} 次仍无法下载完整回放: {}",
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "未知错误".to_string())
    )))
}

fn replace_recording_with_longer_replay(
    recorded_path: &Path,
    replay_path: &Path,
) -> AppResult<bool> {
    let recorded_duration = ffmpeg::probe_media_duration_seconds(recorded_path)?;
    let replay_duration = ffmpeg::probe_media_duration_seconds(replay_path)?;
    logger::info(format!(
        "Twitter/X 录制时长对比: 原录制 {:.1}s | 完整回放 {:.1}s",
        recorded_duration, replay_duration
    ));

    if replay_duration <= recorded_duration + REPLAY_REPLACE_MIN_EXTRA_SECONDS {
        return Ok(false);
    }

    replace_file(replay_path, recorded_path)?;
    Ok(true)
}

fn replace_file(source: &Path, destination: &Path) -> AppResult<()> {
    if !destination.exists() {
        fs::rename(source, destination)?;
        return Ok(());
    }

    let backup_path = recording_backup_path(destination);
    fs::rename(destination, &backup_path)?;
    match fs::rename(source, destination) {
        Ok(()) => {
            if let Err(err) = fs::remove_file(&backup_path) {
                logger::warn_persist(format!(
                    "删除原录制备份失败: {} ({err})",
                    backup_path.display()
                ));
            }
            Ok(())
        }
        Err(err) => {
            if let Err(restore_err) = fs::rename(&backup_path, destination) {
                return Err(AppError::new(format!(
                    "替换录制文件失败: {err}; 恢复原录制文件也失败: {restore_err}"
                )));
            }
            Err(AppError::from(err))
        }
    }
}

fn replay_candidate_path(recorded_path: &Path) -> PathBuf {
    let parent = recorded_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = recorded_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "recording.mp4".into());
    parent.join(format!(".{file_name}.replay_download.tmp.mp4"))
}

fn recording_backup_path(recorded_path: &Path) -> PathBuf {
    let parent = recorded_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = recorded_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "recording.mp4".into());
    parent.join(format!(
        ".{file_name}.recording_backup_{}",
        current_epoch_millis()
    ))
}

fn run_user(args: &[String]) -> AppResult<()> {
    let screen_name = parse_user_target(args)?;
    let output_dir = paths::user_dir(Platform::Twitter);
    fs::create_dir_all(&output_dir)?;

    let config = load_config()?;
    let twitter_cookie = config
        .twitter
        .ok_or_else(|| AppError::new("未配置 twitter.cookies，无法使用 Twitter/X 命令"))?
        .cookies;
    let client = TwitterClient::new(twitter_cookie);

    logger::info(format!(
        "正在获取 Twitter/X 用户主页数据: screen_name={screen_name}"
    ));
    let profile = client.fetch_user_profile(&screen_name)?;
    logger::success("获取 Twitter/X 用户主页数据成功!");

    let profile_file = output_dir.join(format!("profile_{}.json", screen_name));
    common::write_pretty_json(&profile_file, &profile)?;
    logger::success_persist(format!(
        "Twitter/X 用户主页数据已保存至 {}",
        profile_file.display()
    ));

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TwitterLiveTarget {
    BroadcastId(String),
    ScreenName(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadCommandArgs {
    target: String,
    output: Option<PathBuf>,
    jobs: Option<usize>,
}

#[derive(Debug, Clone)]
struct TwitterReplayDownloadPlan {
    title: Option<String>,
    media_playlist: HlsMediaPlaylist,
}

fn parse_live_args(args: &[String]) -> AppResult<LiveCommandArgs> {
    common::parse_live_args(
        args,
        "参数错误: 必须提供 Twitter/X 直播链接、broadcast_id、screen name 或用户主页链接",
        false,
        print_live_help,
        normalize_twitter_target,
    )
}

fn parse_user_target(args: &[String]) -> AppResult<String> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        print_user_help();
        std::process::exit(0);
    }

    let Some(target) = args.first() else {
        print_user_help();
        return Err(AppError::new(
            "参数错误: 必须提供 Twitter/X screen name 或用户主页链接",
        ));
    };

    Ok(extract_screen_name(target))
}

fn parse_download_args(args: &[String]) -> AppResult<DownloadCommandArgs> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        print_download_help();
        std::process::exit(0);
    }

    let mut target = None;
    let mut output = None;
    let mut jobs = None;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "-o" | "--output") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| AppError::new("--output 参数需要指定输出文件或目录"))?;
            output = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--output=") {
            if value.trim().is_empty() {
                return Err(AppError::new("--output 参数需要指定输出文件或目录"));
            }
            output = Some(PathBuf::from(value));
        } else if matches!(arg.as_str(), "-j" | "--jobs") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| AppError::new("--jobs 参数需要指定正整数"))?;
            jobs = Some(parse_jobs(value)?);
        } else if let Some(value) = arg.strip_prefix("--jobs=") {
            jobs = Some(parse_jobs(value)?);
        } else if arg.starts_with('-') {
            return Err(AppError::new(format!("未知参数: {arg}")));
        } else if target.replace(arg.clone()).is_some() {
            return Err(AppError::new(
                "参数错误: Twitter/X download 只能指定一个 broadcast",
            ));
        }
        index += 1;
    }

    let target = target.ok_or_else(|| {
        print_download_help();
        AppError::new("参数错误: 必须提供 Twitter/X broadcast 链接或 broadcast_id")
    })?;

    Ok(DownloadCommandArgs {
        target,
        output,
        jobs,
    })
}

fn parse_jobs(value: &str) -> AppResult<usize> {
    let jobs = value
        .parse::<usize>()
        .map_err(|_| AppError::new("--jobs 参数需要指定正整数"))?;
    if jobs == 0 {
        return Err(AppError::new("--jobs 参数必须大于 0"));
    }
    Ok(jobs)
}

fn parse_twitter_live_target(input: &str) -> TwitterLiveTarget {
    if let Some(broadcast_id) = extract_broadcast_id(input) {
        return TwitterLiveTarget::BroadcastId(broadcast_id);
    }

    if looks_like_broadcast_id(input) {
        return TwitterLiveTarget::BroadcastId(input.to_string());
    }

    TwitterLiveTarget::ScreenName(extract_screen_name(input))
}

fn parse_twitter_broadcast_target(input: &str) -> AppResult<String> {
    if let Some(broadcast_id) = extract_broadcast_id(input) {
        return Ok(broadcast_id);
    }

    if looks_like_broadcast_id(input) {
        return Ok(input.to_string());
    }

    Err(AppError::new(
        "Twitter/X download 只支持 broadcast 链接或 broadcast_id，不支持用户主页",
    ))
}

fn normalize_twitter_target(input: &str) -> String {
    if let Some(broadcast_id) = extract_broadcast_id(input) {
        return broadcast_id;
    }

    extract_screen_name(input)
}

fn looks_like_broadcast_id(input: &str) -> bool {
    input.len() >= 10
        && input.starts_with('1')
        && input.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn extract_broadcast_id(input: &str) -> Option<String> {
    let value =
        common::extract_path_segment(input, &["x.com/i/broadcasts/", "twitter.com/i/broadcasts/"]);
    (value != input).then_some(value)
}

fn extract_screen_name(input: &str) -> String {
    let value = common::extract_path_segment(
        input,
        &["x.com/", "twitter.com/", "www.x.com/", "www.twitter.com/"],
    );
    value.trim_start_matches('@').to_string()
}

fn resolve_download_output_path(
    output: Option<&Path>,
    default_dir: &Path,
    broadcast_id: &str,
    title: Option<&str>,
) -> PathBuf {
    let default_file_name = build_download_file_name(current_epoch_millis(), broadcast_id, title);
    match output {
        Some(path) if path.is_dir() || path.extension().is_none() => path.join(default_file_name),
        Some(path) => path.to_path_buf(),
        None => default_dir.join(default_file_name),
    }
}

fn build_download_file_name(
    timestamp_millis: u128,
    broadcast_id: &str,
    title: Option<&str>,
) -> String {
    let broadcast_id = sanitize_file_component(broadcast_id, 64);
    let title = title
        .map(|value| sanitize_file_component(value, 80))
        .filter(|value| !value.is_empty());

    match title {
        Some(title) => format!("download_{timestamp_millis}_{broadcast_id}_{title}.mp4"),
        None => format!("download_{timestamp_millis}_{broadcast_id}.mp4"),
    }
}

fn sanitize_file_component(value: &str, max_len: usize) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;

    for ch in value.chars() {
        let sanitized = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            ch
        } else {
            '_'
        };

        if sanitized == '_' {
            if last_was_separator {
                continue;
            }
            last_was_separator = true;
        } else {
            last_was_separator = false;
        }

        output.push(sanitized);
        if output.len() >= max_len {
            break;
        }
    }

    output.trim_matches('_').to_string()
}

fn current_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn default_download_jobs() -> usize {
    let available = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    (available / 2).max(1)
}

fn print_help() {
    println!();
    println!("用法: dytl twitter <子命令> [参数...]");
    println!();
    println!("可用子命令:");
    println!("  live         Twitter/X 直播操作（获取信息、录制、播放，默认最高画质）");
    println!("  download     下载已结束且可回放的 Twitter/X broadcast，默认最高画质");
    println!("  user         获取 Twitter/X 用户主页数据");
    println!("  rescue       手动修复/合并 Twitter/X 异常中断留下的临时分片文件夹");
    println!();
}

fn print_live_help() {
    println!();
    println!("用法: dytl twitter live <broadcast_url/broadcast_id/screen_name/url> [-r] [-p]");
    println!();
    println!("参数:");
    println!("  -r, --record         录制 Twitter/X 直播流，默认解析最高画质 HLS variant");
    println!("  -p, --play           使用 ffplay 预览播放最高画质直播流");
    println!();
}

fn print_download_help() {
    println!();
    println!("用法: dytl twitter download <broadcast_url/broadcast_id> [-o PATH] [-j N]");
    println!();
    println!("参数:");
    println!(
        "  -o, --output PATH    输出 MP4 文件路径，或输出目录；默认 content/twitter/download/"
    );
    println!("  -j, --jobs N         并发下载分片数；默认使用机器可用并行度的一半");
    println!();
}

fn print_user_help() {
    println!();
    println!("用法: dytl twitter user <screen_name 或 url>");
    println!();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        DownloadCommandArgs, TwitterLiveTarget, build_download_file_name, parse_download_args,
        parse_twitter_broadcast_target, parse_twitter_live_target,
    };

    #[test]
    fn parses_twitter_broadcast_url() {
        assert_eq!(
            parse_twitter_live_target("https://x.com/i/broadcasts/1TestBroadcastAA"),
            TwitterLiveTarget::BroadcastId("1TestBroadcastAA".to_string())
        );
    }

    #[test]
    fn parses_twitter_profile_url() {
        assert_eq!(
            parse_twitter_live_target("https://x.com/test_screen_user"),
            TwitterLiveTarget::ScreenName("test_screen_user".to_string())
        );
    }

    #[test]
    fn parses_plain_twitter_broadcast_id() {
        assert_eq!(
            parse_twitter_live_target("1TestBroadcastAA"),
            TwitterLiveTarget::BroadcastId("1TestBroadcastAA".to_string())
        );
    }

    #[test]
    fn parses_download_broadcast_url() {
        assert_eq!(
            parse_twitter_broadcast_target("https://x.com/i/broadcasts/1TestBroadcastBB")
                .expect("broadcast should parse"),
            "1TestBroadcastBB"
        );
    }

    #[test]
    fn rejects_profile_url_as_download_target() {
        assert!(parse_twitter_broadcast_target("https://x.com/test_screen_user").is_err());
    }

    #[test]
    fn parses_download_output_arg() {
        assert_eq!(
            parse_download_args(&[
                "1TestBroadcastBB".to_string(),
                "--output".to_string(),
                "out/replay.mp4".to_string(),
            ])
            .expect("args should parse"),
            DownloadCommandArgs {
                target: "1TestBroadcastBB".to_string(),
                output: Some(PathBuf::from("out/replay.mp4")),
                jobs: None,
            }
        );
    }

    #[test]
    fn parses_download_jobs_arg() {
        assert_eq!(
            parse_download_args(&[
                "1TestBroadcastBB".to_string(),
                "--jobs".to_string(),
                "8".to_string(),
            ])
            .expect("args should parse"),
            DownloadCommandArgs {
                target: "1TestBroadcastBB".to_string(),
                output: None,
                jobs: Some(8),
            }
        );
    }

    #[test]
    fn rejects_zero_download_jobs() {
        assert!(
            parse_download_args(&[
                "1TestBroadcastBB".to_string(),
                "-j".to_string(),
                "0".to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn builds_safe_download_file_name() {
        assert_eq!(
            build_download_file_name(1000000000001, "1TestBroadcastBB", Some("Test Title / Demo")),
            "download_1000000000001_1TestBroadcastBB_Test_Title_Demo.mp4"
        );
    }
}
