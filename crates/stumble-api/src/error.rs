//! HTTP error envelope and conversions from core and sync errors.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use stumble_core::*;

#[derive(Debug)]
pub struct ApiError {
    pub(crate) status: StatusCode,
    /// Machine-readable failure class for Agent Harnesses and operators.
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
                "code": self.code,
            })),
        )
            .into_response()
    }
}

fn agent_tools_error_code(error: &AgentToolsError) -> &'static str {
    match error {
        AgentToolsError::Forbidden { .. } => "forbidden",
        AgentToolsError::BootstrapRejected { reason, .. } => reason.as_code(),
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => reason.as_code(),
        AgentToolsError::IndexSearch(failure) => failure.kind.as_code(),
        AgentToolsError::Store(StoreError::InvalidSignature) | AgentToolsError::Signing(_) => {
            "invalid_signature"
        }
        AgentToolsError::Store(StoreError::AnnouncementExpired) => "announcement_expired",
        AgentToolsError::Store(StoreError::AnnouncementWithdrawn) => "announcement_withdrawn",
        AgentToolsError::Store(StoreError::AnnouncementStale) => "announcement_stale",
        AgentToolsError::Store(StoreError::WithdrawalStale) => "withdrawal_stale",
        AgentToolsError::Store(StoreError::NotFound(_)) => "not_found",
        AgentToolsError::Store(StoreError::UntrustedPeer) => "untrusted_peer",
        AgentToolsError::Store(StoreError::Validation(_)) | AgentToolsError::BadUrl(_) => {
            "validation_error"
        }
        AgentToolsError::Store(StoreError::Duplicate(_)) => "duplicate",
        AgentToolsError::Store(StoreError::TenantBoundary) => "tenant_boundary",
        AgentToolsError::LockPoisoned | AgentToolsError::Persistence(_) => "internal_error",
        AgentToolsError::IncompatibleProtocol { .. } => "incompatible_protocol",
        AgentToolsError::RelayDisabled => "relay_disabled",
        AgentToolsError::RelayPayloadTooLarge => "payload_too_large",
        _ => "request_error",
    }
}

impl From<AgentToolsError> for ApiError {
    fn from(value: AgentToolsError) -> Self {
        let status = if matches!(value, AgentToolsError::RelayDisabled) {
            // Same disabled pattern as Bootstrap: absent capability reads as 404.
            StatusCode::NOT_FOUND
        } else if matches!(value, AgentToolsError::Forbidden { .. }) {
            StatusCode::FORBIDDEN
        } else if matches!(value, AgentToolsError::Store(StoreError::NotFound(_))) {
            StatusCode::NOT_FOUND
        } else if matches!(
            value,
            AgentToolsError::LockPoisoned | AgentToolsError::Persistence(_)
        ) {
            StatusCode::INTERNAL_SERVER_ERROR
        } else if matches!(
            value,
            AgentToolsError::BootstrapRejected {
                reason: BootstrapAdmissionRejectionReason::RateLimited,
                ..
            } | AgentToolsError::IndexSearch(IndexSearchFailure {
                kind: IndexSearchFailureKind::RateLimited,
                ..
            }) | AgentToolsError::DiscoveryPeerRejected {
                reason: DiscoveryPeerAdmissionRejectionReason::RateLimited,
                ..
            }
        ) {
            StatusCode::TOO_MANY_REQUESTS
        } else if matches!(
            value,
            AgentToolsError::BootstrapRejected {
                reason: BootstrapAdmissionRejectionReason::BootstrapDisabled,
                ..
            } | AgentToolsError::IndexSearch(IndexSearchFailure {
                kind: IndexSearchFailureKind::IndexDisabled,
                ..
            }) | AgentToolsError::DiscoveryPeerRejected {
                reason: DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
                    | DiscoveryPeerAdmissionRejectionReason::BootstrapDisabled,
                ..
            }
        ) {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        Self {
            status,
            code: agent_tools_error_code(&value),
            message: value.to_string(),
        }
    }
}

impl From<stumble_sync::PeerSyncError> for ApiError {
    fn from(value: stumble_sync::PeerSyncError) -> Self {
        match value {
            stumble_sync::PeerSyncError::Core(source) => source.into(),
            source @ stumble_sync::PeerSyncError::Request { .. } => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "upstream_error",
                message: source.to_string(),
            },
            source @ stumble_sync::PeerSyncError::ImportTask(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal_error",
                message: source.to_string(),
            },
            source => Self {
                status: StatusCode::BAD_REQUEST,
                code: "request_error",
                message: source.to_string(),
            },
        }
    }
}
