//! Recording, playback, and post-processing of live streams.
//!
//! - [`ffmpeg`] — record / play / HLS download / TS merge
//! - [`live_stream`] — shared live record-or-play workflow
//! - [`rescue`] — repair interrupted `temp_record_*` directories

pub mod ffmpeg;
pub mod live_stream;
pub mod rescue;
