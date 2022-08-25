use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Http(#[from] http::Error),
    #[error(transparent)]
    Hyper(#[from] hyper::Error),
    #[error(transparent)]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error("ServiceError: {0}")]
    Service(tower::BoxError),
    #[error(transparent)]
    Other {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}