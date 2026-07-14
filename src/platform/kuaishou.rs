use serde_json::Value;

use crate::core::error::AppResult;
use crate::core::json::get_path;

use super::amagi::{is_flv_url, is_m3u8_url, run_platform_json, value_to_i64, value_to_string};

#[derive(Debug, Clone)]
pub struct KuaishouClient {
    cookie: String,
}

impl KuaishouClient {
    pub fn new(cookie: String) -> Self {
        Self { cookie }
    }

    pub fn fetch_user_profile(&self, principal_id: &str) -> AppResult<Value> {
        self.run_json(&["user-profile", principal_id])
    }

    pub fn fetch_user_work_list(
        &self,
        principal_id: &str,
        count: Option<u64>,
        pcursor: Option<&str>,
    ) -> AppResult<Value> {
        let mut args = vec!["user-work-list".to_string(), principal_id.to_string()];
        if let Some(count) = count {
            args.push("--count".to_string());
            args.push(count.to_string());
        }
        if let Some(pcursor) = pcursor.filter(|value| !value.trim().is_empty()) {
            args.push("--pcursor".to_string());
            args.push(pcursor.to_string());
        }
        let ref_args = args.iter().map(String::as_str).collect::<Vec<_>>();
        self.run_json(&ref_args)
    }

    pub fn fetch_live_room_info(&self, principal_id: &str) -> AppResult<Value> {
        self.run_json(&["live-room-info", principal_id])
    }

    pub fn extract_best_hls_url_for_principal(
        &self,
        room_data: &Value,
        principal_id: &str,
    ) -> Option<String> {
        extract_kuaishou_hls_url_for_principal(room_data, principal_id)
    }

    pub fn extract_best_flv_url_for_principal(
        &self,
        room_data: &Value,
        principal_id: &str,
    ) -> Option<String> {
        extract_kuaishou_ranked_flv_url_for_principal(
            room_data,
            principal_id,
            KuaishouFlvStrategy::Record,
        )
    }

    pub fn extract_best_play_url_for_principal(
        &self,
        room_data: &Value,
        principal_id: &str,
    ) -> Option<String> {
        extract_kuaishou_ranked_flv_url_for_principal(
            room_data,
            principal_id,
            KuaishouFlvStrategy::Play,
        )
        .or_else(|| extract_kuaishou_hls_url_for_principal(room_data, principal_id))
    }

    pub fn has_active_live_room_for_principal(
        &self,
        room_data: &Value,
        principal_id: &str,
    ) -> bool {
        has_kuaishou_live_payload_for_principal(room_data, principal_id)
            || self.indicates_principal_living(room_data, principal_id)
    }

    pub fn indicates_principal_living(&self, room_data: &Value, principal_id: &str) -> bool {
        has_kuaishou_live_payload_for_principal(room_data, principal_id)
            || has_principal_living_marker(room_data, principal_id)
    }

    pub fn is_anti_crawl_blocked(&self, value: &Value) -> bool {
        contains_anti_crawl_marker(value)
    }

    pub fn has_recommended_stream_urls(&self, room_data: &Value) -> bool {
        get_path(room_data, &["upstream_payload", "reco", "list"]).is_some_and(contains_stream_url)
    }

    pub fn extract_profile_nickname(&self, profile_data: &Value) -> Option<String> {
        get_path(profile_data, &["author", "userInfo", "name"])
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
    }

    pub fn extract_monitor_live_info_for_principal(
        &self,
        room_data: &Value,
        principal_id: &str,
    ) -> (Option<String>, Option<String>) {
        for node in live_nodes_for_principal(room_data, principal_id) {
            let nickname = get_path(node, &["author", "userInfo", "name"])
                .or_else(|| get_path(node, &["author", "name"]))
                .or_else(|| get_path(node, &["config", "user", "name"]))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string);
            let live_marker = get_path(node, &["config", "liveStreamId"])
                .or_else(|| get_path(node, &["liveStream", "id"]))
                .or_else(|| get_path(node, &["liveStream", "liveStreamId"]))
                .and_then(value_to_string);

            if nickname.is_some() || live_marker.is_some() {
                return (nickname, live_marker);
            }
        }

        (None, None)
    }

    fn run_json(&self, args: &[&str]) -> AppResult<Value> {
        run_platform_json("kuaishou", "--kuaishou-cookie", &self.cookie, args)
    }
}

#[derive(Debug, Clone, Copy)]
enum KuaishouFlvStrategy {
    Record,
    Play,
}

#[derive(Debug)]
struct KuaishouFlvCandidate {
    url: String,
    level: i64,
    bitrate: i64,
    default_select: bool,
    codec: KuaishouCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum KuaishouCodec {
    Unknown,
    H264,
    Hevc,
}

fn collect_kuaishou_flv_candidates(
    value: &Value,
    codec_hint: Option<KuaishouCodec>,
    candidates: &mut Vec<KuaishouFlvCandidate>,
) {
    match value {
        Value::Object(map) => {
            let inherited_codec = map
                .get("codec")
                .and_then(Value::as_str)
                .map(parse_kuaishou_codec)
                .or(codec_hint)
                .unwrap_or(KuaishouCodec::Unknown);

            if let Some(url) = map
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| is_flv_url(url))
            {
                candidates.push(KuaishouFlvCandidate {
                    url: url.to_string(),
                    level: map.get("level").and_then(value_to_i64).unwrap_or_default(),
                    bitrate: map
                        .get("bitrate")
                        .and_then(value_to_i64)
                        .unwrap_or_default(),
                    default_select: map
                        .get("defaultSelect")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    codec: inherited_codec,
                });
            }

            for (key, child) in map {
                let child_codec_hint = infer_kuaishou_codec_hint(key).or(Some(inherited_codec));
                collect_kuaishou_flv_candidates(child, child_codec_hint, candidates);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_kuaishou_flv_candidates(child, codec_hint, candidates);
            }
        }
        _ => {}
    }
}

fn pick_best_kuaishou_flv_candidate_by(
    candidates: Vec<KuaishouFlvCandidate>,
    strategy: KuaishouFlvStrategy,
) -> Option<KuaishouFlvCandidate> {
    candidates.into_iter().max_by(|left, right| {
        kuaishou_candidate_sort_key(left, strategy)
            .cmp(&kuaishou_candidate_sort_key(right, strategy))
    })
}

fn infer_kuaishou_codec_hint(key: &str) -> Option<KuaishouCodec> {
    let key = key.to_ascii_lowercase();
    if key.contains("h264") || key.contains("avc") {
        Some(KuaishouCodec::H264)
    } else if key.contains("hevc") || key.contains("h265") {
        Some(KuaishouCodec::Hevc)
    } else {
        None
    }
}

fn parse_kuaishou_codec(codec: &str) -> KuaishouCodec {
    let codec = codec.to_ascii_lowercase();
    if codec.contains("h264") || codec.contains("avc") {
        KuaishouCodec::H264
    } else if codec.contains("hevc") || codec.contains("h265") {
        KuaishouCodec::Hevc
    } else {
        KuaishouCodec::Unknown
    }
}

fn kuaishou_candidate_sort_key(
    candidate: &KuaishouFlvCandidate,
    strategy: KuaishouFlvStrategy,
) -> (i64, i64, i64, bool) {
    let codec_rank = match strategy {
        KuaishouFlvStrategy::Record => match candidate.codec {
            KuaishouCodec::H264 => 3,
            KuaishouCodec::Unknown => 2,
            KuaishouCodec::Hevc => 1,
        },
        KuaishouFlvStrategy::Play => match candidate.codec {
            KuaishouCodec::Hevc => 3,
            KuaishouCodec::H264 => 2,
            KuaishouCodec::Unknown => 1,
        },
    };

    match strategy {
        KuaishouFlvStrategy::Record => (
            candidate.level,
            candidate.bitrate,
            codec_rank,
            candidate.default_select,
        ),
        KuaishouFlvStrategy::Play => (
            candidate.level,
            candidate.bitrate,
            codec_rank,
            candidate.default_select,
        ),
    }
}

fn extract_kuaishou_ranked_flv_url_for_principal(
    room_data: &Value,
    principal_id: &str,
    strategy: KuaishouFlvStrategy,
) -> Option<String> {
    let mut candidates = Vec::new();

    for node in live_nodes_for_principal(room_data, principal_id) {
        collect_kuaishou_flv_candidates_from_node(node, &mut candidates);
    }

    pick_best_kuaishou_flv_candidate_by(candidates, strategy).map(|candidate| candidate.url)
}

fn collect_kuaishou_flv_candidates_from_node(
    node: &Value,
    candidates: &mut Vec<KuaishouFlvCandidate>,
) {
    for path in [
        ["liveStream", "playUrls"].as_slice(),
        ["config", "multiResolutionPlayUrls"].as_slice(),
    ] {
        if let Some(found) = get_path(node, path) {
            collect_kuaishou_flv_candidates(found, None, candidates);
        }
    }
}

fn extract_kuaishou_hls_url_for_principal(room_data: &Value, principal_id: &str) -> Option<String> {
    for node in live_nodes_for_principal(room_data, principal_id) {
        if let Some(url) = extract_kuaishou_hls_url_from_node(node) {
            return Some(url);
        }
    }

    None
}

fn extract_kuaishou_hls_url_from_node(node: &Value) -> Option<String> {
    for path in [
        ["liveStream", "hlsPlayUrl"].as_slice(),
        ["config", "hlsPlayUrl"].as_slice(),
    ] {
        if let Some(url) = get_path(node, path)
            .and_then(Value::as_str)
            .filter(|url| is_m3u8_url(url))
            .map(str::to_string)
        {
            return Some(url);
        }
    }

    None
}

fn has_kuaishou_live_payload_for_principal(room_data: &Value, principal_id: &str) -> bool {
    live_nodes_for_principal(room_data, principal_id)
        .into_iter()
        .any(node_has_live_payload)
}

fn node_has_live_payload(node: &Value) -> bool {
    get_path(node, &["config", "liveStreamId"]).is_some()
        || get_path(node, &["liveStream", "id"]).is_some()
        || extract_kuaishou_hls_url_from_node(node).is_some()
        || {
            let mut candidates = Vec::new();
            collect_kuaishou_flv_candidates_from_node(node, &mut candidates);
            !candidates.is_empty()
        }
}

fn contains_stream_url(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.values().any(contains_stream_url),
        Value::Array(items) => items.iter().any(contains_stream_url),
        Value::String(text) => is_flv_url(text) || is_m3u8_url(text),
        _ => false,
    }
}

fn live_nodes(room_data: &Value) -> Vec<&Value> {
    [
        ["current"].as_slice(),
        ["upstream_payload", "live_detail"].as_slice(),
    ]
    .into_iter()
    .filter_map(|path| get_path(room_data, path))
    .filter(|node| node.is_object())
    .collect()
}

fn live_nodes_for_principal<'a>(room_data: &'a Value, principal_id: &str) -> Vec<&'a Value> {
    let root_matches = root_principal_matches(room_data, principal_id);

    live_nodes(room_data)
        .into_iter()
        .filter(|node| root_matches || node_author_matches_principal(node, principal_id))
        .collect()
}

fn node_author_matches_principal(node: &Value, principal_id: &str) -> bool {
    for path in [
        ["id"].as_slice(),
        ["principalId"].as_slice(),
        ["author", "id"].as_slice(),
        ["author", "userInfo", "id"].as_slice(),
        ["author", "userInfo", "principalId"].as_slice(),
        ["author", "principalId"].as_slice(),
        ["userInfo", "id"].as_slice(),
        ["userInfo", "principalId"].as_slice(),
        ["config", "user", "id"].as_slice(),
        ["config", "user", "principalId"].as_slice(),
    ] {
        if value_matches_principal(get_path(node, path), principal_id) {
            return true;
        }
    }

    false
}

fn principal_state_nodes<'a>(room_data: &'a Value, principal_id: &str) -> Vec<&'a Value> {
    let root_matches = root_principal_matches(room_data, principal_id);

    [
        ["author", "userInfo"].as_slice(),
        ["upstream_payload", "user_info", "userInfo"].as_slice(),
        ["upstream_payload", "sensitive", "sensitiveUserInfo"].as_slice(),
    ]
    .into_iter()
    .filter_map(|path| get_path(room_data, path))
    .filter(|node| {
        node.is_object() && (root_matches || node_author_matches_principal(node, principal_id))
    })
    .collect()
}

fn has_principal_living_marker(room_data: &Value, principal_id: &str) -> bool {
    principal_state_nodes(room_data, principal_id)
        .into_iter()
        .any(contains_living_marker)
        || live_nodes_for_principal(room_data, principal_id)
            .into_iter()
            .any(contains_living_marker)
}

fn root_principal_matches(room_data: &Value, principal_id: &str) -> bool {
    value_matches_principal(room_data.get("principalId"), principal_id)
}

fn value_matches_principal(value: Option<&Value>, principal_id: &str) -> bool {
    value
        .and_then(value_to_string)
        .is_some_and(|value| value == principal_id)
}

fn contains_living_marker(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, child)| key_indicates_living(key, child) || contains_living_marker(child)),
        Value::Array(items) => items.iter().any(contains_living_marker),
        _ => false,
    }
}

fn key_indicates_living(key: &str, value: &Value) -> bool {
    let key = key.to_ascii_lowercase();
    let key_is_live_state = key.contains("living")
        || key.contains("livestatus")
        || key.contains("is_live")
        || key.contains("islive");

    if !key_is_live_state {
        return false;
    }

    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().is_some_and(|value| value > 0),
        Value::String(text) => text_indicates_living(text),
        _ => false,
    }
}

fn text_indicates_living(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    matches!(text.as_str(), "live" | "living" | "online" | "onlive")
        || text.contains("正在直播")
        || text.contains("直播中")
}

fn contains_anti_crawl_marker(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, child)| {
            key_value_indicates_anti_crawl(key, child) || contains_anti_crawl_marker(child)
        }),
        Value::Array(items) => items.iter().any(contains_anti_crawl_marker),
        _ => false,
    }
}

fn key_value_indicates_anti_crawl(key: &str, value: &Value) -> bool {
    if key_indicates_anti_crawl(key) && value_is_meaningful(value) {
        return true;
    }

    key_can_contain_anti_crawl_message(key) && value.as_str().is_some_and(text_indicates_anti_crawl)
}

fn key_indicates_anti_crawl(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("captcha")
        || key == "verify"
        || key.contains("verifycode")
        || key.contains("verify_code")
        || key.contains("needverify")
        || key.contains("need_verify")
        || key.contains("verification")
        || key.contains("security")
        || key.contains("risk")
        || key.contains("anticrawl")
        || key.contains("anti_crawl")
        || key.contains("anti-crawl")
        || key.contains("crawl")
}

fn key_can_contain_anti_crawl_message(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("message")
        || key.contains("msg")
        || key.contains("error")
        || key.contains("reason")
        || key.contains("title")
        || key.contains("description")
        || key.contains("desc")
}

fn text_indicates_anti_crawl(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    [
        "captcha",
        "verify",
        "verification",
        "anti-crawl",
        "anti crawl",
        "blocked",
        "risk",
        "风控",
        "反爬",
        "安全验证",
        "滑块",
        "访问过于频繁",
        "操作频繁",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

fn value_is_meaningful(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::Number(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::KuaishouClient;

    fn kuaishou_codec_block(prefix: &str, include_4k: bool) -> Value {
        let mut representations = vec![
            json!({
                "bitrate": 1000,
                "defaultSelect": false,
                "level": 30,
                "url": format!("https://example.com/Sport{prefix}SdL1Promax.flv")
            }),
            json!({
                "bitrate": 2000,
                "defaultSelect": true,
                "level": 50,
                "url": format!("https://example.com/Sport{prefix}HdL1Promax.flv")
            }),
            json!({
                "bitrate": 4000,
                "defaultSelect": false,
                "level": 70,
                "url": format!("https://example.com/Sport{prefix}FhdL1Promax.flv")
            }),
            json!({
                "bitrate": 8000,
                "defaultSelect": false,
                "level": 130,
                "url": format!("https://example.com/Sport{prefix}FhdL3Promax.flv")
            }),
        ];

        if include_4k {
            representations.push(json!({
                "bitrate": 32000,
                "defaultSelect": false,
                "level": 490,
                "url": format!("https://example.com/Sport{prefix}Ultra4kL2Promax.flv")
            }));
        }

        json!({
            "adaptationSet": {
                "representation": representations
            }
        })
    }

    fn kuaishou_live_room_sample() -> Value {
        let play_urls = json!({
            "h264": kuaishou_codec_block("Avc", false),
            "hevc": kuaishou_codec_block("Hevc", true)
        });

        json!({
            "principalId": "test_ks_user_a",
            "activeIndex": 0,
            "current": {
                "config": {
                    "liveStreamId": "test_stream_marker",
                    "hlsPlayUrl": "https://example.com/SportAvcSdL1Promax.m3u8",
                    "multiResolutionPlayUrls": play_urls.clone()
                },
                "liveStream": {
                    "id": "test_stream_marker",
                    "hlsPlayUrl": "https://example.com/SportAvcSdL1Promax.m3u8",
                    "playUrls": play_urls
                }
            },
            "playList": []
        })
    }

    fn kuaishou_offline_room_sample() -> Value {
        json!({
            "activeIndex": 0,
            "current": null,
            "playList": [],
            "principalId": "test_ks_user_a"
        })
    }

    #[test]
    fn extracts_kuaishou_urls_from_live_room_sample() {
        let client = KuaishouClient::new(String::new());
        let room_data = kuaishou_live_room_sample();

        let record_flv = client
            .extract_best_flv_url_for_principal(&room_data, "test_ks_user_a")
            .expect("kuaishou flv url should exist");
        let play_url = client
            .extract_best_play_url_for_principal(&room_data, "test_ks_user_a")
            .expect("kuaishou play url should exist");
        let hls = client
            .extract_best_hls_url_for_principal(&room_data, "test_ks_user_a")
            .expect("kuaishou hls url should exist");
        let (_, marker) =
            client.extract_monitor_live_info_for_principal(&room_data, "test_ks_user_a");

        assert!(record_flv.contains("SportHevcUltra4kL2Promax.flv"));
        assert!(play_url.contains("SportHevcUltra4kL2Promax.flv"));
        assert!(hls.contains(".m3u8"));
        assert_eq!(marker.as_deref(), Some("test_stream_marker"));
        assert!(client.has_active_live_room_for_principal(&room_data, "test_ks_user_a"));
        assert!(client.indicates_principal_living(&room_data, "test_ks_user_a"));
        assert!(!client.is_anti_crawl_blocked(&room_data));
    }

    #[test]
    fn kuaishou_offline_room_does_not_fake_live_urls() {
        let client = KuaishouClient::new(String::new());
        let room_data = kuaishou_offline_room_sample();

        assert!(
            client
                .extract_best_flv_url_for_principal(&room_data, "test_ks_user_a")
                .is_none()
        );
        assert!(
            client
                .extract_best_hls_url_for_principal(&room_data, "test_ks_user_a")
                .is_none()
        );
        assert!(
            client
                .extract_best_play_url_for_principal(&room_data, "test_ks_user_a")
                .is_none()
        );
        assert!(!client.has_active_live_room_for_principal(&room_data, "test_ks_user_a"));
        assert!(!client.indicates_principal_living(&room_data, "test_ks_user_a"));
    }

    #[test]
    fn extracts_principal_urls_from_current_when_root_principal_matches() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "principalId": "test_ks_user_a",
            "current": {
                "config": {
                    "liveStreamId": "test_live_marker",
                    "hlsPlayUrl": "https://example.com/requested-live.m3u8",
                    "multiResolutionPlayUrls": {
                        "h264": {
                            "adaptationSet": {
                                "representation": [{
                                    "url": "https://example.com/requested-live.flv",
                                    "level": 50,
                                    "bitrate": 2000
                                }]
                            }
                        }
                    }
                }
            }
        });

        assert_eq!(
            client.extract_best_hls_url_for_principal(&room_data, "test_ks_user_a"),
            Some("https://example.com/requested-live.m3u8".to_string())
        );
        assert_eq!(
            client.extract_best_flv_url_for_principal(&room_data, "test_ks_user_a"),
            Some("https://example.com/requested-live.flv".to_string())
        );
        assert!(
            client
                .extract_best_flv_url_for_principal(&room_data, "test_ks_user_b")
                .is_none()
        );
        assert!(client.has_active_live_room_for_principal(&room_data, "test_ks_user_a"));
        assert!(!client.has_active_live_room_for_principal(&room_data, "test_ks_user_b"));
    }

    #[test]
    fn does_not_extract_recommended_streams_for_requested_principal() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "principalId": "test_ks_user_a",
            "current": null,
            "playList": [],
            "upstream_payload": {
                "user_info": {
                    "result": 1,
                    "userInfo": {
                        "id": "test_ks_user_a",
                        "living": true,
                        "name": "Requested User"
                    }
                },
                "live_detail": {
                    "result": 2,
                    "author": {
                        "living": false
                    },
                    "config": {
                        "needLoginToWatchHD": false
                    },
                    "liveStream": {
                        "playUrls": {
                            "h264": {},
                            "hevc": {}
                        },
                        "type": "live",
                        "url": "https://example.com/fw/live/undefined"
                    }
                },
                "reco": {
                    "list": [{
                        "author": {
                            "id": "test_ks_user_b",
                            "name": "Other User"
                        },
                        "config": {
                            "hlsPlayUrl": "https://example.com/other-live.m3u8",
                            "multiResolutionPlayUrls": {
                                "h264": {
                                    "adaptationSet": {
                                        "representation": [{
                                            "url": "https://example.com/other-live.flv",
                                            "level": 50,
                                            "bitrate": 2000
                                        }]
                                    }
                                }
                            }
                        }
                    }]
                }
            }
        });

        assert!(client.indicates_principal_living(&room_data, "test_ks_user_a"));
        assert!(client.has_recommended_stream_urls(&room_data));
        assert!(
            client
                .extract_best_flv_url_for_principal(&room_data, "test_ks_user_a")
                .is_none()
        );
        assert!(
            client
                .extract_best_hls_url_for_principal(&room_data, "test_ks_user_a")
                .is_none()
        );
        assert!(
            client
                .extract_best_flv_url_for_principal(&room_data, "test_ks_user_b")
                .is_none()
        );
        assert!(
            client
                .extract_best_hls_url_for_principal(&room_data, "test_ks_user_b")
                .is_none()
        );
    }

    #[test]
    fn detects_kuaishou_living_marker_without_stream_urls() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "principalId": "test_ks_user_a",
            "current": {
                "config": {
                    "liveStatus": "living"
                }
            }
        });

        assert!(client.indicates_principal_living(&room_data, "test_ks_user_a"));
    }

    #[test]
    fn detects_kuaishou_anti_crawl_payload() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "result": 403,
            "captcha": {
                "type": "slider"
            },
            "message": "访问过于频繁，请完成安全验证"
        });

        assert!(client.is_anti_crawl_blocked(&room_data));
    }

    #[test]
    fn ignores_random_token_text_that_contains_risk_words() {
        let client = KuaishouClient::new(String::new());
        let room_data = json!({
            "current": {
                "isLiving": true,
                "config": {
                    "hlsPlayUrl": "https://example.com/live.m3u8"
                }
            },
            "emoji": {
                "token": "random-risk-verify-blocked-text",
                "iconUrls": {
                    "[RomanticShiba]": "//example.com/emoji.webp"
                }
            }
        });

        assert!(!client.is_anti_crawl_blocked(&room_data));
    }
}
