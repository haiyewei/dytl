//! FFmpeg-backed recording, playback, HLS download, and TS merge.

mod hls;
mod merge;
mod player;
mod record;
mod types;
mod util;

pub use hls::{download_hls_segments_concurrent, probe_media_duration_seconds};
pub use merge::merge_ts_segments;
pub use player::{is_ffplay_available, spawn_player};
pub use record::record;
pub use types::{RecordEndReason, RecordResult};
