use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::core::error::AppResult;
use crate::core::logger;
use crate::core::signals::install_shutdown_flag;
use crate::media::ffmpeg::{self, RecordResult};

pub struct LiveStreamRunOptions<'a> {
    pub record: bool,
    pub stream_id: &'a str,
    pub output_dir: &'a Path,
    pub cookie: Option<&'a str>,
    pub stop_file: Option<&'a Path>,
    pub shutdown_message: &'static str,
    pub record_url: Option<&'a str>,
    pub play_url: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveStreamRunResult {
    pub recording: Option<RecordResult>,
}

pub fn run(options: LiveStreamRunOptions<'_>) -> AppResult<LiveStreamRunResult> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    if options.record {
        install_shutdown_flag(shutdown_requested.clone(), options.shutdown_message)?;
    }

    let mut player = if let Some(url) = options.play_url {
        Some(ffmpeg::spawn_player(url, options.cookie)?)
    } else {
        None
    };

    let record_result = if let Some(url) = options.record_url {
        ffmpeg::record(
            url,
            options.stream_id,
            options.output_dir,
            options.cookie,
            options.stop_file,
            shutdown_requested,
        )
        .map(Some)
    } else {
        Ok(None)
    };

    if let Some(child) = player.as_mut() {
        if options.record {
            let _ = child.kill();
            let _ = child.wait();
        } else {
            let status = child.wait()?;
            logger::info(format!("播放结束，退出码: {:?}", status.code()));
        }
    }

    record_result.map(|recording| LiveStreamRunResult { recording })
}
