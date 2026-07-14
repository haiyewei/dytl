use serde_json::{Map, Value};

pub fn parse_json_lines(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }

    let mut last = None;
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            last = Some(value);
        }
    }
    last
}

pub fn find_object_recursive<'a, F>(
    value: &'a Value,
    predicate: &F,
) -> Option<&'a Map<String, Value>>
where
    F: Fn(&Map<String, Value>) -> bool,
{
    match value {
        Value::Object(map) => {
            if predicate(map) {
                return Some(map);
            }

            map.values()
                .find_map(|child| find_object_recursive(child, predicate))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_object_recursive(child, predicate)),
        _ => None,
    }
}

pub fn find_string_by_key<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                return Some(found);
            }
            map.values()
                .find_map(|child| find_string_by_key(child, key))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_string_by_key(child, key)),
        _ => None,
    }
}

pub fn get_path<'a>(value: &'a Value, paths: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for path in paths {
        current = match current {
            Value::Object(map) => map.get(*path)?,
            Value::Array(items) => {
                let index = path.parse::<usize>().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}
