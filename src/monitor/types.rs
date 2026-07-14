use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;

#[derive(Default)]
pub struct MonitorState {
    pub active: HashMap<String, RecordingProcess>,
    pub stopping: Vec<RecordingProcess>,
    pub pending_restart: bool,
}

pub struct RecordingProcess {
    pub name: String,
    pub live_marker: String,
    pub stop_file: PathBuf,
    pub child: Child,
}

pub struct LiveSession {
    pub name: String,
    pub live_marker: String,
    pub record_arg: String,
}

pub enum TargetStatus {
    Online(LiveSession),
    Offline { name: String },
}

#[derive(Default)]
pub struct PollSummary {
    pub live_count: usize,
    pub recording_count: usize,
    pub offline_count: usize,
    pub failed_count: usize,
}
