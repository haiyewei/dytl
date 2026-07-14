use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::MonitorTarget;
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;

use super::types::{LiveSession, MonitorState, RecordingProcess};

pub fn start_recording(
    target: &MonitorTarget,
    session: &LiveSession,
    current_exe: &Path,
    state: &mut MonitorState,
) -> AppResult<()> {
    let stop_file = paths::monitor_dir().join(format!(
        "monitor_stop_{}_{}_{}.flag",
        target.platform.as_str(),
        sanitize_file_component(&target.account),
        sanitize_file_component(&session.live_marker)
    ));
    let _ = fs::remove_file(&stop_file);

    logger::info(format!(
        "[{}] 启动独立录制子进程 -> {} {} live -r --stop-file {} {}",
        session.name,
        current_exe.display(),
        target.platform.as_str(),
        stop_file.display(),
        session.record_arg
    ));

    let child = Command::new(current_exe)
        .args([
            target.platform.as_str(),
            "live",
            "-r",
            "--stop-file",
            &stop_file.to_string_lossy(),
            &session.record_arg,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    state.active.insert(
        target.key(),
        RecordingProcess {
            name: session.name.clone(),
            live_marker: session.live_marker.clone(),
            stop_file,
            child,
        },
    );
    Ok(())
}

pub fn stop_active_recording(target_key: &str, state: &mut MonitorState) -> AppResult<()> {
    if let Some(process) = state.active.remove(target_key) {
        request_stop(&process)?;
        state.stopping.push(process);
    }
    Ok(())
}

pub fn request_stop_all(state: &mut MonitorState) -> AppResult<()> {
    let keys = state.active.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        stop_active_recording(&key, state)?;
    }
    Ok(())
}

pub fn reap_active_processes(state: &mut MonitorState) -> AppResult<()> {
    let keys = state.active.keys().cloned().collect::<Vec<_>>();
    let mut finished = Vec::new();

    for key in keys {
        if let Some(process) = state.active.get_mut(&key)
            && let Some(status) = process.child.try_wait()?
        {
            logger::info(format!(
                "[{}] 独立录制进程已退出 (代码: {:?})",
                process.name,
                status.code()
            ));
            finished.push(key);
        }
    }

    for key in finished {
        if let Some(process) = state.active.remove(&key) {
            let _ = fs::remove_file(process.stop_file);
        }
    }

    Ok(())
}

pub fn reap_stopping_processes(state: &mut MonitorState) -> AppResult<()> {
    let mut index = 0;
    while index < state.stopping.len() {
        let finished = if let Some(status) = state.stopping[index].child.try_wait()? {
            logger::info(format!(
                "[{}] 独立录制进程已退出 (代码: {:?})",
                state.stopping[index].name,
                status.code()
            ));
            true
        } else {
            false
        };

        if finished {
            let process = state.stopping.remove(index);
            let _ = fs::remove_file(process.stop_file);
        } else {
            index += 1;
        }
    }
    Ok(())
}

pub fn handle_exit(state: &mut MonitorState) -> AppResult<()> {
    if state.active.is_empty() && state.stopping.is_empty() {
        return Ok(());
    }

    logger::warn(format!(
        "监控进程收到退出信号，当前有 {} 个任务正在录制...",
        state.active.len() + state.stopping.len()
    ));
    logger::warn("正在向所有录制进程发出停止标记并等待它们安全退出 (进行 MP4 合并)...");

    request_stop_all(state)?;
    let deadline = Instant::now() + Duration::from_secs(25);

    loop {
        reap_stopping_processes(state)?;

        if state.stopping.is_empty() {
            logger::success("所有录制进程已安全退出，监控程序关闭。");
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(AppError::new("等待子进程退出超时，强制结束监控进程"));
        }

        thread::sleep(Duration::from_millis(250));
    }
}

fn request_stop(process: &RecordingProcess) -> AppResult<()> {
    logger::info(format!("[{}] 正在终止录制...", process.name));
    fs::write(&process.stop_file, b"stop")?;
    Ok(())
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
