//! Unified live monitoring: poll targets, spawn recorders, auto-rescue.

mod processes;
mod providers;
mod types;

use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{AppConfig, MonitorConfig};
use crate::core::error::{AppError, AppResult};
use crate::core::logger;
use crate::core::paths;
use crate::core::signals::install_shutdown_flag;
use crate::media::rescue::{self, RescueOptions};

use self::processes::{
    handle_exit, reap_active_processes, reap_stopping_processes, request_stop_all, start_recording,
    stop_active_recording,
};
use self::providers::MonitorClients;
use self::types::{MonitorState, PollSummary, TargetStatus};

pub fn run(config: AppConfig) -> AppResult<()> {
    let monitor = config
        .monitor
        .clone()
        .ok_or_else(|| AppError::new("配置文件中未配置 monitor 功能，或 targets 列表为空"))?;
    let enabled_target_count = monitor.enabled_target_count();
    if monitor.targets.is_empty() {
        return Err(AppError::new(
            "配置文件中未配置 monitor 功能，或 targets 列表为空",
        ));
    }
    if enabled_target_count == 0 {
        return Err(AppError::new(
            "monitor.targets 中没有启用的目标，请至少将一个 enabled 设为 true",
        ));
    }

    let stop_dir = paths::monitor_dir();
    fs::create_dir_all(&stop_dir)?;

    logger::info(format!(
        "启动统一监控模式：配置目标 {}，启用监控 {}，轮询间隔 {} 秒",
        monitor.targets.len(),
        enabled_target_count,
        monitor.poll_interval_sec
    ));

    let clients = MonitorClients::from_config(&config);
    let current_exe = env::current_exe()?;
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    install_shutdown_flag(
        shutdown_requested.clone(),
        "监控进程收到退出指令，正在通知所有录制任务安全收尾...",
    )?;

    let mut state = MonitorState::default();
    let restart_at = monitor
        .restart_interval_hours
        .map(|hours| Instant::now() + Duration::from_secs(hours * 60 * 60));
    if let Some(hours) = monitor.restart_interval_hours {
        logger::info(format!(
            "已配置定时重启：将在 {hours} 小时后自动退出并重新登录"
        ));
    }
    if monitor.auto_rescue.enabled {
        logger::info(format!(
            "已启用录制巡航封装：启动巡航 {} | 间隔 {} 分钟 | 最小年龄 {} 分钟",
            if monitor.auto_rescue.on_startup {
                "ON"
            } else {
                "OFF"
            },
            monitor.auto_rescue.interval_minutes,
            monitor.auto_rescue.min_age_minutes
        ));
        if monitor.auto_rescue.on_startup {
            run_auto_rescue(&monitor, "启动巡航封装")?;
        }
    }

    check_all_targets(&clients, &monitor, &current_exe, &mut state)?;
    let poll_interval = Duration::from_secs(monitor.poll_interval_sec);
    let mut next_poll = Instant::now() + poll_interval;
    let mut next_rescue = monitor
        .auto_rescue
        .enabled
        .then(|| Instant::now() + Duration::from_secs(monitor.auto_rescue.interval_minutes * 60));

    loop {
        reap_active_processes(&mut state)?;
        reap_stopping_processes(&mut state)?;

        if shutdown_requested.load(Ordering::SeqCst) {
            return handle_exit(&mut state);
        }

        if let Some(restart_at) = restart_at
            && !state.pending_restart
            && Instant::now() >= restart_at
        {
            state.pending_restart = true;
            let recording_count = state.active.len() + state.stopping.len();
            if recording_count == 0 {
                logger::warn("达到定时重新登录时间，当前无录制任务，准备退出...");
                return Ok(());
            }

            logger::warn(format!(
                "达到定时重新登录时间，当前有 {recording_count} 个录制任务。将停止检测新开播，并在所有录制结束后重启..."
            ));
            request_stop_all(&mut state)?;
        }

        if state.pending_restart && state.active.is_empty() && state.stopping.is_empty() {
            logger::success("所有录制任务已安全退出，监控程序关闭。");
            return Ok(());
        }

        if !state.pending_restart && Instant::now() >= next_poll {
            check_all_targets(&clients, &monitor, &current_exe, &mut state)?;
            next_poll = Instant::now() + poll_interval;
        }

        if !state.pending_restart
            && let Some(rescue_at) = next_rescue
            && Instant::now() >= rescue_at
        {
            run_auto_rescue(&monitor, "定时巡航封装")?;
            next_rescue = Some(
                Instant::now() + Duration::from_secs(monitor.auto_rescue.interval_minutes * 60),
            );
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn run_auto_rescue(monitor: &MonitorConfig, label: &'static str) -> AppResult<()> {
    let summary = rescue::run(RescueOptions {
        platforms: rescue::all_platforms(),
        min_age: Duration::from_secs(monitor.auto_rescue.min_age_minutes * 60),
        label,
    })?;

    if summary.handled_anything() {
        logger::info(format!(
            "{label}处理结果: 发现 {} | 成功 {} | 清理空目录 {} | 跳过活跃/过新 {} | 失败 {}",
            summary.found,
            summary.merged,
            summary.removed_empty,
            summary.skipped_recent,
            summary.failed
        ));
    }

    Ok(())
}

fn check_all_targets(
    clients: &MonitorClients,
    monitor: &MonitorConfig,
    current_exe: &Path,
    state: &mut MonitorState,
) -> AppResult<()> {
    let mut summary = PollSummary::default();
    logger::info(format!(
        "开始新一轮监听状态检查，共 {} 个启用目标",
        monitor.enabled_target_count()
    ));

    for target in monitor.enabled_targets() {
        let target_key = target.key();
        match clients.check_target(target) {
            Ok(TargetStatus::Online(session)) => {
                summary.live_count += 1;
                if let Some(current) = state.active.get(&target_key) {
                    if current.live_marker == session.live_marker {
                        summary.recording_count += 1;
                        logger::info(format!(
                            "[{}] 监听状态: 直播中 | 直播标识: {} | 录制状态: 录制中",
                            session.name, session.live_marker
                        ));
                        continue;
                    }

                    logger::info(format!(
                        "[{}] 检测到直播标识变更 (新 {}，旧 {})，准备重启录制进程",
                        session.name, session.live_marker, current.live_marker
                    ));
                    stop_active_recording(&target_key, state)?;
                    start_recording(target, &session, current_exe, state)?;
                    summary.recording_count += 1;
                    logger::info(format!(
                        "[{}] 监听状态: 直播中 | 直播标识: {} | 录制状态: 已重启录制",
                        session.name, session.live_marker
                    ));
                } else {
                    logger::success(format!(
                        "[{}] 正在直播中! 直播标识: {}",
                        session.name, session.live_marker
                    ));
                    start_recording(target, &session, current_exe, state)?;
                    summary.recording_count += 1;
                    logger::info(format!(
                        "[{}] 监听状态: 直播中 | 直播标识: {} | 录制状态: 已启动录制",
                        session.name, session.live_marker
                    ));
                }
            }
            Ok(TargetStatus::Offline { name }) => {
                summary.offline_count += 1;
                if state.active.contains_key(&target_key) {
                    logger::info(format!("[{name}] 直播已结束，正在停止录制进程..."));
                    stop_active_recording(&target_key, state)?;
                    logger::info(format!(
                        "[{name}] 监听状态: 未开播 | 录制状态: 正在停止录制并等待封装"
                    ));
                } else {
                    logger::info(format!("[{name}] 监听状态: 未开播"));
                }
            }
            Err(err) => {
                summary.failed_count += 1;
                logger::warn(format!(
                    "[{}] 监听状态: 获取失败 | {err}",
                    target.log_name(None)
                ));
            }
        }
    }

    logger::info(format!(
        "本轮监听状态汇总: 直播中 {} | 录制中 {} | 未开播 {} | 获取失败 {}",
        summary.live_count, summary.recording_count, summary.offline_count, summary.failed_count
    ));

    Ok(())
}
