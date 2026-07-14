use serde_json::{Map, Value};

use crate::core::error::{AppError, AppResult};
use crate::core::json::{find_object_recursive, find_string_by_key, get_path};
use crate::core::logger;

use super::amagi::{
    find_object_value_recursive, find_value_by_key, is_http_url, run_platform_json, value_to_string,
};

const QUALITY_PRIORITY: [&str; 5] = ["FULL_HD1", "origin", "HD1", "SD1", "SD2"];
const DOUYIN_STREAM_DATA_PRIORITY: [&str; 7] = ["uhd", "origin", "hd", "sd", "md", "ld", "ao"];

#[derive(Debug, Clone)]
pub struct DouyinClient {
    cookie: String,
}

impl DouyinClient {
    pub fn new(cookie: String) -> Self {
        Self { cookie }
    }

    pub fn fetch_live_room_info(&self, room_id: &str, web_rid: &str) -> AppResult<Value> {
        self.run_json(&["live-room-info", "--web-rid", web_rid, room_id])
    }

    pub fn fetch_user_profile(&self, sec_uid: &str) -> AppResult<Value> {
        match self.run_json(&["user-profile", sec_uid]) {
            Ok(value) => Ok(value),
            Err(err) => {
                logger::warn(format!(
                    "直接获取用户主页失败，尝试通过视频列表保底获取作者信息: {err}"
                ));
                let video_data = self.fetch_user_video_list(sec_uid, Some(1), None)?;
                if let Some(author) = extract_first_author(&video_data) {
                    return Ok(serde_json::json!({
                        "user": author,
                        "m_user": author,
                    }));
                }

                Err(AppError::new(format!(
                    "无法获取用户 [{sec_uid}] 数据: {err}"
                )))
            }
        }
    }

    pub fn fetch_user_video_list(
        &self,
        sec_uid: &str,
        number: Option<u64>,
        max_cursor: Option<u64>,
    ) -> AppResult<Value> {
        let mut args = vec!["user-video-list".to_string(), sec_uid.to_string()];
        if let Some(number) = number {
            args.push("--number".to_string());
            args.push(number.to_string());
        }
        if let Some(max_cursor) = max_cursor {
            args.push("--max-cursor".to_string());
            args.push(max_cursor.to_string());
        }
        let ref_args = args.iter().map(String::as_str).collect::<Vec<_>>();
        self.run_json(&ref_args)
    }

    pub fn extract_true_room_id(&self, room_data: &Value) -> Option<String> {
        get_path(room_data, &["data", "data", "0", "id_str"])
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| find_string_by_key(room_data, "id_str").map(str::to_string))
    }

    pub fn extract_best_hls_url(&self, room_data: &Value) -> Option<String> {
        let stream = extract_stream_url_info(room_data)?;
        stream
            .get("hls_pull_url_map")
            .and_then(pick_best_url)
            .or_else(|| stream.get("hls_pull_url").and_then(value_to_string))
            .or_else(|| extract_douyin_stream_data_url(stream, "hls"))
    }

    pub fn extract_best_flv_url(&self, room_data: &Value) -> Option<String> {
        let stream = extract_stream_url_info(room_data)?;
        stream
            .get("flv_pull_url")
            .and_then(pick_best_url)
            .or_else(|| extract_douyin_stream_data_url(stream, "flv"))
    }

    pub fn extract_monitor_live_info(
        &self,
        profile_data: &Value,
    ) -> (Option<String>, Option<String>) {
        let user = extract_user_like(profile_data).unwrap_or(profile_data);
        let nickname = user
            .get("nickname")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| find_string_by_key(user, "nickname").map(str::to_string));

        let room_data_value = user
            .get("room_data")
            .or_else(|| find_value_by_key(user, "room_data"));
        let room_data = match room_data_value {
            Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
            Some(value) => Some(value.clone()),
            None => None,
        };

        let web_rid = room_data
            .as_ref()
            .and_then(|value| get_path(value, &["owner", "web_rid"]))
            .and_then(value_to_string);

        (nickname, web_rid)
    }

    fn run_json(&self, args: &[&str]) -> AppResult<Value> {
        run_platform_json("douyin", "--douyin-cookie", &self.cookie, args)
    }
}

fn extract_first_author(video_data: &Value) -> Option<Value> {
    get_path(video_data, &["aweme_list", "0", "author"])
        .cloned()
        .or_else(|| {
            find_object_recursive(video_data, &|map| map.contains_key("author"))
                .and_then(|map| map.get("author"))
                .cloned()
        })
}

fn extract_stream_url_info(room_data: &Value) -> Option<&Map<String, Value>> {
    get_path(room_data, &["data", "data", "0", "stream_url"])
        .or_else(|| get_path(room_data, &["data", "data", "0", "web_stream_url"]))
        .or_else(|| get_path(room_data, &["data", "web_stream_url"]))
        .and_then(Value::as_object)
}

fn pick_best_url(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    for quality in QUALITY_PRIORITY {
        if let Some(found) = map.get(quality).and_then(value_to_string) {
            return Some(found);
        }
    }

    map.values().find_map(value_to_string)
}

fn extract_douyin_stream_data_url(stream: &Map<String, Value>, kind: &str) -> Option<String> {
    let raw_stream_data = stream
        .get("live_core_sdk_data")
        .and_then(|value| get_path(value, &["pull_data", "stream_data"]))?
        .as_str()?;
    let parsed = serde_json::from_str::<Value>(raw_stream_data).ok()?;

    for quality in DOUYIN_STREAM_DATA_PRIORITY {
        if let Some(url) = get_path(&parsed, &["data", quality, "main", kind])
            .and_then(Value::as_str)
            .filter(|url| is_http_url(url).is_some())
            .map(str::to_string)
        {
            return Some(url);
        }
    }

    None
}

fn extract_user_like(value: &Value) -> Option<&Value> {
    if let Some(user) = value.get("user") {
        return Some(user);
    }

    find_object_value_recursive(value, &|map| {
        map.contains_key("nickname") || map.contains_key("room_data")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::DouyinClient;

    fn douyin_room_sample() -> Value {
        json!({
            "data": {
                "data": [{
                    "stream_url": {
                        "flv_pull_url": {
                            "FULL_HD1": "http://example.com/live_uhd.flv",
                            "HD1": "http://example.com/live_hd.flv"
                        },
                        "hls_pull_url": "http://example.com/live_default.m3u8",
                        "hls_pull_url_map": {
                            "FULL_HD1": "http://example.com/live_uhd.m3u8",
                            "HD1": "http://example.com/live_hd.m3u8"
                        },
                        "live_core_sdk_data": {
                            "pull_data": {
                                "stream_data": "{\"data\":{\"uhd\":{\"main\":{\"flv\":\"http://fallback.example.com/live_uhd.flv\",\"hls\":\"http://fallback.example.com/live_uhd.m3u8\"}},\"hd\":{\"main\":{\"flv\":\"http://fallback.example.com/live_hd.flv\",\"hls\":\"http://fallback.example.com/live_hd.m3u8\"}}}}"
                            }
                        }
                    }
                }]
            }
        })
    }

    #[test]
    fn extracts_douyin_urls_from_room_sample() {
        let client = DouyinClient::new(String::new());
        let room_data = douyin_room_sample();

        let flv = client
            .extract_best_flv_url(&room_data)
            .expect("douyin flv url should exist");
        let hls = client
            .extract_best_hls_url(&room_data)
            .expect("douyin hls url should exist");

        assert!(flv.contains("_uhd.flv"));
        assert!(hls.contains("_uhd.m3u8"));
    }

    #[test]
    fn falls_back_to_douyin_stream_data_when_maps_are_empty() {
        let client = DouyinClient::new(String::new());
        let mut room_data = douyin_room_sample();
        room_data["data"]["data"][0]["stream_url"]["flv_pull_url"] = serde_json::json!({});
        room_data["data"]["data"][0]["stream_url"]["hls_pull_url_map"] = serde_json::json!({});

        let flv = client
            .extract_best_flv_url(&room_data)
            .expect("douyin flv fallback should exist");
        let hls = client
            .extract_best_hls_url(&room_data)
            .expect("douyin hls fallback should exist");

        assert!(flv.contains(".flv"));
        assert!(hls.contains(".m3u8"));
    }

    #[test]
    fn does_not_extract_douyin_recommended_streams() {
        let client = DouyinClient::new(String::new());
        let room_data = json!({
            "data": {
                "data": [{
                    "owner": {
                        "web_rid": "test_dy_room_a"
                    }
                }],
                "similar_rooms": [{
                    "room": {
                        "owner": {
                            "web_rid": "test_dy_room_b"
                        },
                        "stream_url": {
                            "flv_pull_url": {
                                "FULL_HD1": "http://example.com/recommended.flv"
                            },
                            "hls_pull_url_map": {
                                "FULL_HD1": "http://example.com/recommended.m3u8"
                            }
                        }
                    }
                }]
            }
        });

        assert!(client.extract_best_flv_url(&room_data).is_none());
        assert!(client.extract_best_hls_url(&room_data).is_none());
    }
}
