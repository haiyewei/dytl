//! Concatenate TS segment files into a single MP4.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::core::error::{AppError, AppResult};

use super::util::absolutize_path;

pub fn merge_ts_segments(dir_path: &Path, mp4_path: &Path, list_file_name: &str) -> AppResult<()> {
    let files = collect_ts_files(dir_path)?;
    if files.is_empty() {
        return Err(AppError::new("目录为空或无有效 TS 分片"));
    }

    let absolute_mp4_path = absolutize_path(mp4_path)?;
    if let Some(parent) = absolute_mp4_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let concat_list_path = dir_path.join(list_file_name);
    let concat_content = files
        .iter()
        .map(|file| format!("file '{}'", file))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&concat_list_path, concat_content)?;

    let mp4_output_arg = absolute_mp4_path.to_string_lossy().to_string();

    let status = Command::new("ffmpeg")
        .current_dir(dir_path)
        .args([
            "-fflags",
            "+genpts",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_file_name,
            "-c",
            "copy",
            "-avoid_negative_ts",
            "make_zero",
            "-y",
            mp4_output_arg.as_str(),
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

pub(crate) fn collect_ts_files(dir_path: &Path) -> AppResult<Vec<String>> {
    let mut files = fs::read_dir(dir_path)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let is_ts = path.extension().and_then(|ext| ext.to_str()) == Some("ts");
            if is_ts {
                entry.file_name().into_string().ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}
