use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Windows API error: {0}")]
    WinApi(&'static str),
    #[error("Invalid setting: {0}")]
    InvalidSetting(String),
}

pub type AppResult<T> = Result<T, AppError>;
