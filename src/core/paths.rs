use std::path::PathBuf;

use crate::config::Platform;

pub fn content_root() -> PathBuf {
    PathBuf::from("content")
}

pub fn platform_root(platform: Platform) -> PathBuf {
    content_root().join(platform.as_str())
}

pub fn live_dir(platform: Platform) -> PathBuf {
    platform_root(platform).join("live")
}

pub fn user_dir(platform: Platform) -> PathBuf {
    platform_root(platform).join("user")
}

pub fn download_dir(platform: Platform) -> PathBuf {
    platform_root(platform).join("download")
}

pub fn monitor_dir() -> PathBuf {
    content_root().join("monitor")
}
