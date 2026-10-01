use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn report_intake_limit() -> Self {
        Self { status: StatusCode::CONFLICT, code: "report_intake_limit", message: babble_graph::moderation::REPORT_INTAKE_LIMIT.into() }
    }
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "invalid or expired credentials".into(),
        }
    }

    pub fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: "principal is not authorized for this operation".into(),
        }
    }

    pub fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "authentication capacity exceeded; retry later".into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "provider_unavailable",
            message: message.into(),
        }
    }

    pub fn storage_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "storage_unavailable",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl From<babble_types::Error> for ApiError {
    fn from(value: babble_types::Error) -> Self {
        match value {
            babble_types::Error::InvalidPrefix { .. }
            | babble_types::Error::InvalidHashLength { .. }
            | babble_types::Error::Canonical(_) => Self::bad_request(value.to_string()),
            babble_types::Error::NotFound(_) => Self::not_found(value.to_string()),
            babble_types::Error::Conflict(_) => Self::conflict(value.to_string()),
            babble_types::Error::ProviderUnavailable(_) => Self::unavailable(value.to_string()),
            babble_types::Error::StorageUnavailable(_) => {
                Self::storage_unavailable(value.to_string())
            }
            babble_types::Error::Signature
            | babble_types::Error::UnsignedObject
            | babble_types::Error::UnsignedEdge
            | babble_types::Error::UnsignedEvent => Self::bad_request(value.to_string()),
        }
    }
}

impl From<babble_store::BlobReadError> for ApiError {
    fn from(value: babble_store::BlobReadError) -> Self {
        match value {
            babble_store::BlobReadError::TooLarge { .. } => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: value.to_string(),
            },
            babble_store::BlobReadError::Storage(error) => error.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        (
            status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}
