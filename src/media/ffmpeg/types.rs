//! Shared recording result types.

use std::path::PathBuf;

/// Why a live recording session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordEndReason {
    StreamEnded,
    StopFile,
    ShutdownSignal,
}

/// Result of a completed live recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordResult {
    pub output_path: PathBuf,
    pub end_reason: RecordEndReason,
}
