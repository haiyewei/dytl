use crate::config::{AppConfig, MonitorTarget, Platform};
use crate::core::error::{AppError, AppResult};
use crate::platform::{DouyinClient, KuaishouClient, TwitterClient};

use super::types::{LiveSession, TargetStatus};

pub struct MonitorClients {
    douyin: Option<DouyinClient>,
    kuaishou: Option<KuaishouClient>,
    twitter: Option<TwitterClient>,
}

impl MonitorClients {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            douyin: config
                .douyin
                .as_ref()
                .map(|value| DouyinClient::new(value.cookies.clone())),
            kuaishou: config
                .kuaishou
                .as_ref()
                .map(|value| KuaishouClient::new(value.cookies.clone())),
            twitter: config
                .twitter
                .as_ref()
                .map(|value| TwitterClient::new(value.cookies.clone())),
        }
    }

    pub fn check_target(&self, target: &MonitorTarget) -> AppResult<TargetStatus> {
        match target.platform {
            Platform::Douyin => check_douyin_target(self.douyin()?, target),
            Platform::Kuaishou => check_kuaishou_target(self.kuaishou()?, target),
            Platform::Twitter => check_twitter_target(self.twitter()?, target),
        }
    }

    fn douyin(&self) -> AppResult<&DouyinClient> {
        self.douyin
            .as_ref()
            .ok_or_else(|| AppError::new("未配置 douyin.cookies，无法监控抖音账号"))
    }

    fn kuaishou(&self) -> AppResult<&KuaishouClient> {
        self.kuaishou
            .as_ref()
            .ok_or_else(|| AppError::new("未配置 kuaishou.cookies，无法监控快手账号"))
    }

    fn twitter(&self) -> AppResult<&TwitterClient> {
        self.twitter
            .as_ref()
            .ok_or_else(|| AppError::new("未配置 twitter.cookies，无法监控 Twitter/X 账号"))
    }
}

fn check_douyin_target(client: &DouyinClient, target: &MonitorTarget) -> AppResult<TargetStatus> {
    let profile = client.fetch_user_profile(&target.account)?;
    let (nickname, web_rid) = client.extract_monitor_live_info(&profile);
    let name = target.log_name(nickname.as_deref());

    match web_rid {
        Some(web_rid) => Ok(TargetStatus::Online(LiveSession {
            name,
            live_marker: web_rid.clone(),
            record_arg: web_rid,
        })),
        None => Ok(TargetStatus::Offline { name }),
    }
}

fn check_kuaishou_target(
    client: &KuaishouClient,
    target: &MonitorTarget,
) -> AppResult<TargetStatus> {
    match client.fetch_live_room_info(&target.account) {
        Ok(room_data) => {
            let (nickname, live_marker) =
                client.extract_monitor_live_info_for_principal(&room_data, &target.account);
            let name = target.log_name(nickname.as_deref());
            if client.is_anti_crawl_blocked(&room_data) {
                return Err(AppError::new(format!(
                    "{name}: 快手接口触发风控或验证，无法确认直播状态"
                )));
            }

            let record_url = client
                .extract_best_flv_url_for_principal(&room_data, &target.account)
                .or_else(|| client.extract_best_hls_url_for_principal(&room_data, &target.account));

            if record_url.is_some() {
                return Ok(TargetStatus::Online(LiveSession {
                    name,
                    live_marker: live_marker.unwrap_or_else(|| target.account.clone()),
                    record_arg: target.account.clone(),
                }));
            }

            if client.indicates_principal_living(&room_data, &target.account) {
                let detail = if client.has_recommended_stream_urls(&room_data) {
                    "快手显示目标账号正在直播，但 live-room-info 仅在推荐列表返回直播流；已忽略推荐流"
                } else {
                    "快手显示目标账号正在直播，但 live-room-info 未返回该账号可用直播流"
                };
                return Err(AppError::new(format!("{name}: {detail}")));
            }

            Ok(TargetStatus::Offline { name })
        }
        Err(live_err) => {
            let nickname = client
                .fetch_user_profile(&target.account)
                .ok()
                .and_then(|profile| client.extract_profile_nickname(&profile));
            let name = target.log_name(nickname.as_deref());

            if looks_like_offline_error(&live_err.to_string()) {
                return Ok(TargetStatus::Offline { name });
            }

            Err(AppError::new(format!("{name}: {live_err}")))
        }
    }
}

fn check_twitter_target(client: &TwitterClient, target: &MonitorTarget) -> AppResult<TargetStatus> {
    let room_data = client.fetch_live_room_info(&target.account)?;
    let info = client.extract_live_info(&room_data);
    let name = target.log_name(info.screen_name.as_deref());

    if info.is_live {
        let broadcast_id = info.broadcast_id.ok_or_else(|| {
            AppError::new(format!("{name}: Twitter/X 已开播但未返回 broadcast_id"))
        })?;
        Ok(TargetStatus::Online(LiveSession {
            name,
            live_marker: broadcast_id.clone(),
            record_arg: broadcast_id,
        }))
    } else {
        Ok(TargetStatus::Offline { name })
    }
}

fn looks_like_offline_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "offline",
        "not live",
        "not found live",
        "room closed",
        "not currently live",
        "未开播",
        "未直播",
        "直播结束",
        "已下播",
    ]
    .iter()
    .any(|keyword| message.contains(keyword))
}
