//! Platform API clients.
//!
//! All network data is obtained through the external `amagi` CLI. Each client
//! parses platform-specific JSON and exposes stable helpers for live status,
//! stream URLs, and user profiles.

mod amagi;
mod douyin;
mod kuaishou;
mod twitter;

pub use douyin::DouyinClient;
pub use kuaishou::KuaishouClient;
pub use twitter::{HlsMediaPlaylist, TwitterClient};
