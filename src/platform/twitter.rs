use std::process::Command;

use serde_json::Value;

use crate::core::error::{AppError, AppResult};
use crate::core::json::get_path;

use super::amagi::{is_http_url, is_m3u8_url, run_platform_json, value_to_string};

#[derive(Debug, Clone)]
pub struct TwitterClient {
    cookie: String,
}

impl TwitterClient {
    pub fn new(cookie: String) -> Self {
        Self { cookie }
    }

    pub fn fetch_user_profile(&self, screen_name: &str) -> AppResult<Value> {
        self.run_json(&["user-profile", screen_name])
    }

    pub fn fetch_live_room_info(&self, screen_name: &str) -> AppResult<Value> {
        self.run_json(&["live-room-info", screen_name])
    }

    pub fn fetch_live_room_stream(&self, broadcast_id: &str) -> AppResult<Value> {
        self.run_json(&["live-room-stream", broadcast_id])
    }

    pub fn extract_live_info(&self, room_data: &Value) -> TwitterLiveInfo {
        let live_video = room_data.get("live_video");
        TwitterLiveInfo {
            screen_name: room_data
                .get("screen_name")
                .and_then(value_to_string)
                .or_else(|| {
                    live_video
                        .and_then(|value| value.get("twitter_username"))
                        .and_then(value_to_string)
                }),
            broadcast_id: live_video
                .and_then(|value| value.get("broadcast_id"))
                .and_then(value_to_string),
            media_key: live_video
                .and_then(|value| value.get("media_key"))
                .and_then(value_to_string),
            tweet_id: live_video
                .and_then(|value| value.get("tweet_id"))
                .and_then(value_to_string),
            title: live_video
                .and_then(|value| value.get("title").or_else(|| value.get("status")))
                .and_then(value_to_string),
            state: live_video
                .and_then(|value| value.get("state"))
                .and_then(value_to_string),
            is_live: room_data
                .get("is_live")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    pub fn extract_hls_url(&self, stream_data: &Value) -> Option<String> {
        stream_data
            .get("hls_url")
            .and_then(Value::as_str)
            .filter(|url| is_m3u8_url(url))
            .map(str::to_string)
            .or_else(|| {
                get_path(
                    stream_data,
                    &["upstream_payload", "stream_status", "source", "location"],
                )
                .and_then(Value::as_str)
                .filter(|url| is_m3u8_url(url))
                .map(str::to_string)
            })
    }

    pub fn extract_replay_info(&self, stream_data: &Value) -> TwitterReplayInfo {
        let broadcast = stream_data.get("broadcast");
        TwitterReplayInfo {
            title: broadcast
                .and_then(|value| value.get("status").or_else(|| value.get("title")))
                .and_then(value_to_string)
                .or_else(|| {
                    stream_data
                        .get("title")
                        .or_else(|| stream_data.get("status"))
                        .and_then(value_to_string)
                }),
            state: broadcast
                .and_then(|value| value.get("state"))
                .and_then(value_to_string)
                .or_else(|| stream_data.get("state").and_then(value_to_string)),
            source_status: stream_data.get("source_status").and_then(value_to_string),
            is_replay: stream_data
                .get("is_replay")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            available_for_replay: broadcast
                .and_then(|value| value.get("available_for_replay"))
                .and_then(Value::as_bool),
        }
    }

    pub fn resolve_best_hls_url(&self, master_url: &str) -> AppResult<String> {
        let playlist = self.fetch_hls_playlist(master_url)?;
        Ok(pick_best_hls_variant(&playlist, master_url).unwrap_or_else(|| master_url.to_string()))
    }

    pub fn fetch_hls_playlist(&self, url: &str) -> AppResult<String> {
        fetch_text_with_curl(url)
    }

    pub fn extract_hls_media_playlist(
        &self,
        playlist: &str,
        playlist_url: &str,
    ) -> HlsMediaPlaylist {
        parse_hls_media_playlist(playlist, playlist_url)
    }

    fn run_json(&self, args: &[&str]) -> AppResult<Value> {
        run_platform_json("twitter", "--twitter-cookie", &self.cookie, args)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitterLiveInfo {
    pub screen_name: Option<String>,
    pub broadcast_id: Option<String>,
    pub media_key: Option<String>,
    pub tweet_id: Option<String>,
    pub title: Option<String>,
    pub state: Option<String>,
    pub is_live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitterReplayInfo {
    pub title: Option<String>,
    pub state: Option<String>,
    pub source_status: Option<String>,
    pub is_replay: bool,
    pub available_for_replay: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsMediaPlaylist {
    pub segments: Vec<HlsMediaSegment>,
    pub is_vod: bool,
    pub has_endlist: bool,
    pub is_encrypted: bool,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsMediaSegment {
    pub url: String,
    pub duration_seconds: Option<f64>,
}

fn fetch_text_with_curl(url: &str) -> AppResult<String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "15", url])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::new(format!(
            "读取 HLS master 失败: {}",
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_hls_media_playlist(playlist: &str, playlist_url: &str) -> HlsMediaPlaylist {
    let mut segments = Vec::new();
    let mut is_vod = false;
    let mut has_endlist = false;
    let mut is_encrypted = false;
    let mut duration_seconds = 0.0;
    let mut pending_duration = None;

    for raw_line in playlist.lines() {
        let line = raw_line.trim();
        if line.eq_ignore_ascii_case("#EXT-X-PLAYLIST-TYPE:VOD") {
            is_vod = true;
        } else if line.eq_ignore_ascii_case("#EXT-X-ENDLIST") {
            has_endlist = true;
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let method = parse_attr(attrs, "METHOD").unwrap_or_default();
            if !method.eq_ignore_ascii_case("NONE") {
                is_encrypted = true;
            }
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            pending_duration = value
                .split(',')
                .next()
                .and_then(|value| value.parse::<f64>().ok());
        } else if !line.is_empty() && !line.starts_with('#') {
            let duration = pending_duration.take();
            if let Some(duration) = duration {
                duration_seconds += duration;
            }
            segments.push(HlsMediaSegment {
                url: join_hls_url(playlist_url, line),
                duration_seconds: duration,
            });
        }
    }

    HlsMediaPlaylist {
        segments,
        is_vod,
        has_endlist,
        is_encrypted,
        duration_seconds,
    }
}

fn pick_best_hls_variant(playlist: &str, master_url: &str) -> Option<String> {
    let mut variants = Vec::new();
    let mut pending = None;

    for raw_line in playlist.lines() {
        let line = raw_line.trim();
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(HlsVariantMeta {
                bandwidth: parse_attr_u64(attrs, "BANDWIDTH").unwrap_or_default(),
                width: parse_resolution(attrs)
                    .map(|value| value.0)
                    .unwrap_or_default(),
                height: parse_resolution(attrs)
                    .map(|value| value.1)
                    .unwrap_or_default(),
            });
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some(meta) = pending.take()
        {
            variants.push(HlsVariant {
                meta,
                url: join_hls_url(master_url, line),
            });
        }
    }

    variants
        .into_iter()
        .max_by_key(|variant| {
            (
                variant.meta.width * variant.meta.height,
                variant.meta.bandwidth,
            )
        })
        .map(|variant| variant.url)
}

#[derive(Debug, Clone, Copy)]
struct HlsVariantMeta {
    bandwidth: u64,
    width: u64,
    height: u64,
}

#[derive(Debug, Clone)]
struct HlsVariant {
    meta: HlsVariantMeta,
    url: String,
}

fn parse_attr_u64(attrs: &str, name: &str) -> Option<u64> {
    parse_attr(attrs, name)?.parse().ok()
}

fn parse_resolution(attrs: &str) -> Option<(u64, u64)> {
    let value = parse_attr(attrs, "RESOLUTION")?;
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn parse_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    attrs.split(',').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches('"'))
    })
}

fn join_hls_url(master_url: &str, value: &str) -> String {
    if is_http_url(value).is_some() {
        return value.to_string();
    }

    if value.starts_with('/')
        && let Some((origin, _)) = split_url_origin(master_url)
    {
        return format!("{origin}{value}");
    }

    let base = master_url
        .split_once('?')
        .map(|value| value.0)
        .unwrap_or(master_url);
    let dir = base.rsplit_once('/').map(|value| value.0).unwrap_or(base);
    format!("{dir}/{value}")
}

fn split_url_origin(url: &str) -> Option<(&str, &str)> {
    let scheme_end = url.find("://")? + 3;
    let path_start = url[scheme_end..]
        .find('/')
        .map(|index| scheme_end + index)
        .unwrap_or(url.len());
    Some((&url[..path_start], &url[path_start..]))
}

#[cfg(test)]
mod tests {
    use super::{parse_hls_media_playlist, pick_best_hls_variant};

    #[test]
    fn picks_highest_resolution_hls_variant() {
        let playlist = r#"
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=600000,RESOLUTION=568x320
low/dynamic_delta.m3u8?type=live
#EXT-X-STREAM-INF:BANDWIDTH=2750000,RESOLUTION=1280x720
mid/dynamic_delta.m3u8?type=live
#EXT-X-STREAM-INF:BANDWIDTH=5500000,RESOLUTION=1920x1080
high/dynamic_delta.m3u8?type=live
"#;

        let best = pick_best_hls_variant(
            playlist,
            "https://example.com/root/master_dynamic_delta.m3u8?type=live",
        )
        .expect("best variant should parse");

        assert_eq!(
            best,
            "https://example.com/root/high/dynamic_delta.m3u8?type=live"
        );
    }

    #[test]
    fn breaks_resolution_ties_by_bandwidth() {
        let playlist = r#"
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080
lower.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5500000,RESOLUTION=1920x1080
higher.m3u8
"#;

        let best = pick_best_hls_variant(playlist, "https://example.com/live/master.m3u8")
            .expect("best variant should parse");

        assert_eq!(best, "https://example.com/live/higher.m3u8");
    }

    #[test]
    fn parses_complete_vod_media_playlist_segments() {
        let playlist = r#"
#EXTM3U
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:2.0,
chunk_0.ts
#EXTINF:3.5,
sub/chunk_1.ts
#EXT-X-ENDLIST
"#;

        let media = parse_hls_media_playlist(
            playlist,
            "https://example.com/root/playlist.m3u8?type=replay",
        );

        assert!(media.is_vod);
        assert!(media.has_endlist);
        assert!(!media.is_encrypted);
        assert_eq!(media.duration_seconds, 5.5);
        assert_eq!(
            media
                .segments
                .iter()
                .map(|segment| segment.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/root/chunk_0.ts",
                "https://example.com/root/sub/chunk_1.ts"
            ]
        );
        assert_eq!(
            media
                .segments
                .iter()
                .map(|segment| segment.duration_seconds)
                .collect::<Vec<_>>(),
            vec![Some(2.0), Some(3.5)]
        );
    }

    #[test]
    fn detects_encrypted_media_playlist() {
        let playlist = r#"
#EXTM3U
#EXT-X-KEY:METHOD=AES-128,URI="key.bin"
#EXTINF:2.0,
chunk_0.ts
#EXT-X-ENDLIST
"#;

        let media = parse_hls_media_playlist(playlist, "https://example.com/root/playlist.m3u8");

        assert!(media.is_encrypted);
    }
}
