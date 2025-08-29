use poem::{Response, error::ResponseError, http::StatusCode};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Invalid input: {0}")]
    BadRequest(String),

    #[error("Database error: {0}")]
    Database(#[from] anyhow::Error),
}

impl ResponseError for AppError {
    fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn as_response(&self) -> Response {
        Response::builder()
            .status(self.status())
            .body(self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
