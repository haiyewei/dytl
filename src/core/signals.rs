use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::error::{AppError, AppResult};
use crate::core::logger;

pub fn install_shutdown_flag(flag: Arc<AtomicBool>, message: &'static str) -> AppResult<()> {
    ctrlc::set_handler(move || {
        if !flag.swap(true, Ordering::SeqCst) {
            logger::warn(message);
        }
    })
    .map_err(|err| AppError::new(err.to_string()))
}
