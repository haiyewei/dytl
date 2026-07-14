//! Application configuration: platforms, validated models, and YAML loading.
//!
//! - [`platform`] — platform identifiers
//! - [`model`] — runtime types after validation
//! - [`load`] — YAML parsing, defaults, and cookie/target checks

mod load;
mod model;
mod platform;

pub use load::{load_config, set_config_path, try_init_logging_from_current_config};
pub use model::{AppConfig, MonitorConfig, MonitorTarget};
pub use platform::Platform;

// Types are reachable via `AppConfig` fields; re-export for call-site ergonomics.
#[allow(unused_imports)]
pub use load::{current_config_path, load_config_from_path};
#[allow(unused_imports)]
pub use model::{
    AutoRescueConfig, DouyinConfig, KuaishouConfig, LoggingConfig, TimeConfig, TwitterConfig,
};
