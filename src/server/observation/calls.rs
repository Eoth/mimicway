//! The calls kept by traffic observation, read back one by one: the list of a service's panel and the details of one
//! call, with what a person needs to read it again or send it again by hand (the interface builds the command,
//! `frontend/src/lib/curl.js`). Nothing more than the store holds: credentials were redacted before anything was kept,
//! and bodies cut at `max_body_size()`.
use super::{FailedCall, ObservationKey, ObservedExchange};
use crate::engine::FailureCause;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// One line of the list.
#[derive(Debug, Clone, Serialize)]
pub struct CallSummary {
    pub id: u64,
    pub timestamp: u64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub operation: Option<String>,
    /// The backend's status; `None` when no response came, and `failure` says why.
    pub status: Option<u16>,
    pub failure: Option<FailureCause>,
}

impl CallSummary {
    pub(super) fn of_exchange(key: &ObservationKey, exchange: &ObservedExchange) -> Self {
        Self {
            id: exchange.id,
            timestamp: exchange.timestamp,
            method: key.method.clone(),
            path: exchange.request_path.clone(),
            query: exchange.request_query.clone(),
            operation: key.operation.clone(),
            status: Some(exchange.response_status),
            failure: None,
        }
    }

    pub(super) fn of_failure(call: &FailedCall) -> Self {
        Self {
            id: call.request.id,
            timestamp: call.request.timestamp,
            method: call.method.clone(),
            path: call.request.request_path.clone(),
            query: call.request.request_query.clone(),
            operation: call.operation.clone(),
            status: None,
            failure: Some(call.cause),
        }
    }
}

/// One call, whole as it was kept: the request, then the response or why none came.
#[derive(Debug, Clone, Serialize)]
pub struct CallDetail {
    pub id: u64,
    pub timestamp: u64,
    pub method: String,
    pub operation: Option<String>,
    pub request: CallRequest,
    pub response: Option<CallResponse>,
    pub failure: Option<CallFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallRequest {
    pub path: String,
    pub query: Option<String>,
    /// Mimicway's address as the caller reached it (`Host`), when the request named one.
    pub host: Option<String>,
    /// By name, for a stable display; one value per name, credentials as `[redacted]`.
    pub headers: BTreeMap<String, String>,
    pub body: KeptBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: KeptBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallFailure {
    pub cause: FailureCause,
    pub detail: Option<String>,
    /// What Mimicway answered the caller instead.
    pub status: u16,
    pub target: String,
}

/// A body as observation kept it.
#[derive(Debug, Clone, Serialize)]
pub struct KeptBody {
    /// The text kept: the first `max_body_size()` bytes, a byte that is not UTF-8 replaced by U+FFFD.
    pub text: String,
    pub truncated: bool,
    /// The size of the whole body, in bytes.
    pub size: usize,
    /// Whether the whole body was UTF-8: otherwise `text` is not the original, byte for byte.
    pub utf8: bool,
    pub content_type: Option<String>,
}

fn sorted(headers: &HashMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn request_of(exchange: &ObservedExchange) -> CallRequest {
    CallRequest {
        path: exchange.request_path.clone(),
        query: exchange.request_query.clone(),
        host: exchange.request_host.clone(),
        headers: sorted(&exchange.request_headers),
        body: KeptBody {
            text: exchange.request_body.clone(),
            truncated: exchange.request_body_truncated,
            size: exchange.request_body_size,
            utf8: exchange.request_body_utf8,
            content_type: exchange.request_content_type.clone(),
        },
    }
}

impl CallDetail {
    pub(super) fn of_exchange(key: &ObservationKey, exchange: &ObservedExchange) -> Self {
        Self {
            id: exchange.id,
            timestamp: exchange.timestamp,
            method: key.method.clone(),
            operation: key.operation.clone(),
            request: request_of(exchange),
            response: Some(CallResponse {
                status: exchange.response_status,
                headers: sorted(&exchange.response_headers),
                body: KeptBody {
                    text: exchange.response_body.clone(),
                    truncated: exchange.response_body_truncated,
                    size: exchange.response_body_size,
                    utf8: exchange.response_body_utf8,
                    content_type: exchange.response_content_type.clone(),
                },
            }),
            failure: None,
        }
    }

    pub(super) fn of_failure(call: &FailedCall) -> Self {
        Self {
            id: call.request.id,
            timestamp: call.request.timestamp,
            method: call.method.clone(),
            operation: call.operation.clone(),
            request: request_of(&call.request),
            response: None,
            failure: Some(CallFailure {
                cause: call.cause,
                detail: call.detail.clone(),
                status: call.status,
                target: call.target.clone(),
            }),
        }
    }
}
