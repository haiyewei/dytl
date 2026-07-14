use std::process::Command;

use serde_json::{Map, Value};

use crate::core::error::{AppError, AppResult};
use crate::core::json::parse_json_lines;

pub(crate) fn run_platform_json(
    platform: &str,
    cookie_flag: &str,
    cookie: &str,
    args: &[&str],
) -> AppResult<Value> {
    let mut command = base_amagi_json_command();
    if !cookie.trim().is_empty() {
        command.args([cookie_flag, cookie]);
    }
    let output = command.args(["run", platform]).args(args).output()?;

    parse_amagi_json_output(output)
}

fn base_amagi_json_command() -> Command {
    let mut command = Command::new("amagi");
    command.args(["--output", "json", "--log-level", "error"]);
    command
}

fn parse_amagi_json_output(output: std::process::Output) -> AppResult<Value> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("amagi exited with {}", output.status)
        };
        return Err(AppError::new(detail));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_json_lines(&stdout).ok_or_else(|| {
        AppError::new(format!(
            "amagi 没有返回可解析的 JSON 输出: {}",
            stdout.trim()
        ))
    })
}

pub(crate) fn find_value_by_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|child| find_value_by_key(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find_value_by_key(child, key)),
        _ => None,
    }
}

pub(crate) fn find_object_value_recursive<'a, F>(
    value: &'a Value,
    predicate: &F,
) -> Option<&'a Value>
where
    F: Fn(&Map<String, Value>) -> bool,
{
    match value {
        Value::Object(map) => {
            if predicate(map) {
                return Some(value);
            }
            map.values()
                .find_map(|child| find_object_value_recursive(child, predicate))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_object_value_recursive(child, predicate)),
        _ => None,
    }
}

pub(crate) fn is_http_url(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed)
    } else {
        None
    }
}

pub(crate) fn is_flv_url(text: &str) -> bool {
    text.contains(".flv")
}

pub(crate) fn is_m3u8_url(text: &str) -> bool {
    text.contains(".m3u8")
}

pub(crate) fn value_to_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .or_else(|| value.as_i64().map(|value| value.to_string()))
}

pub(crate) fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|value| value as i64))
}
