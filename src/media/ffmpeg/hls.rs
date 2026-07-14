//! HLS media duration probing and concurrent segment download.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::core::error::{AppError, AppResult};
use crate::core::logger;

use super::util::{
    absolutize_path, current_epoch_millis, release_file_cache, sanitize_file_component,
};

pub fn probe_media_duration_seconds(path: &Path) -> AppResult<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::new(format!(
            "读取媒体时长失败: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration = stdout
        .trim()
        .parse::<f64>()
        .map_err(|_| AppError::new(format!("无法解析媒体时长: {}", stdout.trim())))?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err(AppError::new(format!("媒体时长无效: {duration}")));
    }

    Ok(duration)
}

pub fn download_hls_segments_concurrent(
    segment_urls: &[String],
    segment_durations: &[Option<f64>],
    output_path: &Path,
    temp_name: &str,
    jobs: usize,
    cookie: Option<&str>,
) -> AppResult<()> {
    if segment_urls.is_empty() {
        return Err(AppError::new("HLS 回放清单没有可下载分片"));
    }
    if segment_urls.len() != segment_durations.len() {
        return Err(AppError::new("HLS 分片数量和时长数量不一致"));
    }
    if segment_durations.iter().any(Option::is_none) {
        return Err(AppError::new("HLS 回放清单缺少部分分片时长，无法安全封装"));
    }

    let absolute_output_path = absolutize_path(output_path)?;
    let parent = absolute_output_path
        .parent()
        .ok_or_else(|| AppError::new("输出路径缺少父目录"))?;
    fs::create_dir_all(parent)?;

    let temp_dir = parent.join(format!(
        "temp_download_{}_{}",
        current_epoch_millis(),
        sanitize_file_component(temp_name)
    ));
    fs::create_dir_all(&temp_dir)?;

    let jobs = jobs.max(1).min(segment_urls.len());
    logger::info(format!(
        "开始并发下载 HLS 分片: 分片 {} 个，并发 {}，临时目录 {}",
        segment_urls.len(),
        jobs,
        temp_dir.display()
    ));

    let queue = Arc::new(Mutex::new(
        segment_urls
            .iter()
            .enumerate()
            .map(|(index, url)| (index, url.clone()))
            .collect::<Vec<_>>(),
    ));
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let error = Arc::new(Mutex::new(None::<AppError>));
    let cookie = cookie.map(str::to_string);

    let mut handles = Vec::new();
    for _ in 0..jobs {
        let queue = queue.clone();
        let completed = completed.clone();
        let error = error.clone();
        let temp_dir = temp_dir.clone();
        let cookie = cookie.clone();
        let total = segment_urls.len();

        handles.push(thread::spawn(move || {
            loop {
                if error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .is_some()
                {
                    break;
                }

                let next = {
                    let mut queue = queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    queue.pop()
                };

                let Some((index, url)) = next else {
                    break;
                };

                let file_path = temp_dir.join(format!("slice_{index:06}.ts"));
                if let Err(err) = download_segment_with_curl(&url, &file_path, cookie.as_deref()) {
                    let mut error = error
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if error.is_none() {
                        *error = Some(AppError::new(format!(
                            "下载分片失败: index={} ({err})",
                            index + 1
                        )));
                    }
                    break;
                }

                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                if done == total || done.is_multiple_of(25) {
                    logger::info(format!("HLS 分片下载进度: {done}/{total}"));
                }
            }
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            let _ = release_file_cache(&temp_dir);
            return Err(AppError::new("分片下载线程异常退出"));
        }
    }

    if let Some(err) = error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        let _ = release_file_cache(&temp_dir);
        return Err(err);
    }

    logger::info(format!(
        "正在封装并发下载结果为 MP4: {}",
        absolute_output_path.display()
    ));
    if let Err(err) =
        merge_downloaded_hls_segments(&temp_dir, &absolute_output_path, segment_durations)
    {
        let _ = release_file_cache(&temp_dir);
        return Err(err);
    }

    release_file_cache(&temp_dir)?;
    logger::success_persist(format!(
        "Twitter/X 回放下载完成: {}",
        absolute_output_path.display()
    ));
    Ok(())
}

fn merge_downloaded_hls_segments(
    dir_path: &Path,
    mp4_path: &Path,
    segment_durations: &[Option<f64>],
) -> AppResult<()> {
    let absolute_mp4_path = absolutize_path(mp4_path)?;
    if let Some(parent) = absolute_mp4_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:VOD\n");
    let target_duration = segment_durations
        .iter()
        .filter_map(|value| *value)
        .fold(1.0_f64, f64::max)
        .ceil() as u64;
    playlist.push_str(&format!("#EXT-X-TARGETDURATION:{target_duration}\n"));
    playlist.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");

    for (index, duration) in segment_durations.iter().enumerate() {
        let file_name = format!("slice_{index:06}.ts");
        let file_path = dir_path.join(&file_name);
        if !file_path.exists() {
            return Err(AppError::new(format!("缺少已下载分片: {file_name}")));
        }
        let duration = duration.ok_or_else(|| AppError::new("HLS 分片时长缺失"))?;
        playlist.push_str(&format!("#EXTINF:{duration:.6},\n{file_name}\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");

    let playlist_path = dir_path.join("download.m3u8");
    fs::write(&playlist_path, playlist)?;

    let status = Command::new("ffmpeg")
        .current_dir(dir_path)
        .args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-allowed_extensions",
            "ALL",
            "-protocol_whitelist",
            "file,crypto,data",
            "-i",
            "download.m3u8",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-y",
            absolute_mp4_path.to_string_lossy().as_ref(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(AppError::new(format!(
            "封装 MP4 失败，FFmpeg 退出码 {:?}",
            status.code()
        )));
    }

    Ok(())
}

fn download_segment_with_curl(
    url: &str,
    output_path: &Path,
    cookie: Option<&str>,
) -> AppResult<()> {
    let mut args = vec![
        "-fL".to_string(),
        "--retry".to_string(),
        "3".to_string(),
        "--retry-delay".to_string(),
        "1".to_string(),
        "--connect-timeout".to_string(),
        "15".to_string(),
        "--max-time".to_string(),
        "120".to_string(),
    ];

    if let Some(cookie) = cookie.map(str::trim).filter(|value| !value.is_empty()) {
        args.extend(["-H".to_string(), format!("Cookie: {cookie}")]);
    }

    args.extend([
        "-o".to_string(),
        output_path.to_string_lossy().to_string(),
        url.to_string(),
    ]);

    let output = Command::new("curl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::new(format!(
            "curl 退出码 {:?}: {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    Ok(())
}
