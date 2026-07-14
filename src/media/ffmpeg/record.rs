//! Live stream recording into TS segments and MP4 packaging.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::core::error::{AppError, AppResult};
use crate::core::logger;

use super::merge::{collect_ts_files, merge_ts_segments};
use super::types::{RecordEndReason, RecordResult};
use super::util::{
    append_http_cookie_args, build_record_base_name, release_recording_file_cache,
    send_ffmpeg_stop, stop_requested,
};

pub fn record(
    url: &str,
    room_id: &str,
    output_dir: &Path,
    douyin_cookie: Option<&str>,
    stop_file: Option<&Path>,
    shutdown_requested: Arc<AtomicBool>,
) -> AppResult<RecordResult> {
    let base_name = build_record_base_name(room_id);
    let dir_path = output_dir.join(format!("temp_{base_name}"));
    let mp4_path = output_dir.join(format!("{base_name}.mp4"));

    fs::create_dir_all(&dir_path)?;
    logger::info(format!(
        "开始调用 FFmpeg 进行流缓冲 (TS 切片存储)，临时保存至: {}",
        dir_path.display()
    ));

    let mut ffmpeg_args = Vec::new();
    append_http_cookie_args(&mut ffmpeg_args, douyin_cookie);
    ffmpeg_args.extend([
        "-i".to_string(),
        url.to_string(),
        "-c".to_string(),
        "copy".to_string(),
        "-f".to_string(),
        "segment".to_string(),
        "-segment_time".to_string(),
        "10".to_string(),
        "-reset_timestamps".to_string(),
        "1".to_string(),
        dir_path.join("slice_%04d.ts").to_string_lossy().to_string(),
    ]);

    let mut ffmpeg = match Command::new("ffmpeg")
        .args(ffmpeg_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            release_recording_file_cache(&dir_path, stop_file, false);
            return Err(AppError::from(err));
        }
    };

    let mut stop_reason = None;
    let mut stdin = ffmpeg.stdin.take();

    loop {
        if stop_reason.is_none()
            && (shutdown_requested.load(Ordering::SeqCst) || stop_requested(stop_file))
        {
            let reason = if shutdown_requested.load(Ordering::SeqCst) {
                RecordEndReason::ShutdownSignal
            } else {
                RecordEndReason::StopFile
            };
            stop_reason = Some(reason);
            logger::warn("检测到退出指令，正在通知 FFmpeg 安全收尾并准备封装 MP4，请耐心等待...");
            send_ffmpeg_stop(stdin.as_mut());
        }

        if let Some(status) = ffmpeg.try_wait()? {
            logger::info(format!(
                "录制流传输断开，FFmpeg 退出码: {:?}",
                status.code()
            ));
            break;
        }

        thread::sleep(Duration::from_millis(500));
    }

    let files = match collect_ts_files(&dir_path) {
        Ok(files) => files,
        Err(err) => {
            logger::error_persist(format!("读取录制缓存失败: {err}"));
            release_recording_file_cache(&dir_path, stop_file, false);
            return Err(err);
        }
    };
    if files.is_empty() {
        let err = AppError::new("未找到任何有效的录制切片，可能根本未开始录制");
        logger::error_persist(err.to_string());
        release_recording_file_cache(&dir_path, stop_file, false);
        return Err(err);
    }

    logger::info(format!(
        "正在封装 MP4: 检索到 {} 个本地分片，开始合并为 {} ...",
        files.len(),
        mp4_path.display()
    ));

    if let Err(err) = merge_ts_segments(&dir_path, &mp4_path, "concat.txt") {
        logger::error_persist(format!("封装 MP4 失败: {err}"));
        release_recording_file_cache(&dir_path, stop_file, false);
        return Err(err);
    }

    logger::success_persist(format!("封装完成！新文件为: {}", mp4_path.display()));
    release_recording_file_cache(&dir_path, stop_file, true);

    logger::success_persist("操作已全部结束，可以安全关闭了。");
    Ok(RecordResult {
        output_path: mp4_path,
        end_reason: stop_reason.unwrap_or(RecordEndReason::StreamEnded),
    })
}
