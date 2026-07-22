//! Public endpoint policy for Discovery Peer advertisements.

use crate::domain::DiscoveryPeerAdmissionRejectionReason;
use std::net::IpAddr;
use url::Url;

/// Endpoint policy failure mapped onto a stable rejection reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointPolicyError {
    /// URL is empty or not parseable.
    Malformed,
    /// Scheme is not HTTPS outside loopback.
    Insecure,
    /// Host is a private, link-local, or otherwise non-public address.
    Private,
}

impl EndpointPolicyError {
    /// Maps policy failure onto a public rejection reason.
    #[must_use]
    pub const fn as_rejection(self) -> DiscoveryPeerAdmissionRejectionReason {
        match self {
            Self::Malformed => DiscoveryPeerAdmissionRejectionReason::Malformed,
            Self::Insecure => DiscoveryPeerAdmissionRejectionReason::InsecureEndpoint,
            Self::Private => DiscoveryPeerAdmissionRejectionReason::PrivateEndpoint,
        }
    }
}

/// Normalizes and validates a Discovery Peer public base endpoint.
///
/// Policy:
/// - HTTPS is required except for loopback hosts (`localhost` / loopback IPs),
///   which may use HTTP for local development.
/// - Literal private, link-local, unspecified, and multicast IP hosts are
///   rejected (loopback is allowed only with the loopback HTTP exception above
///   or HTTPS).
/// - Path, query, and fragment are stripped; trailing slashes removed.
///
/// # Errors
///
/// Returns [`EndpointPolicyError`] when the endpoint violates public policy.
pub fn normalize_discovery_peer_endpoint(value: &str) -> Result<String, EndpointPolicyError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(EndpointPolicyError::Malformed);
    }
    let mut url = Url::parse(trimmed).map_err(|_| EndpointPolicyError::Malformed)?;
    let host = url.host_str().ok_or(EndpointPolicyError::Malformed)?;
    let host_ip = host.parse::<IpAddr>().ok();
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host_ip.is_some_and(|address| address.is_loopback());
    let is_loopback_http = url.scheme() == "http" && is_loopback;
    if url.scheme() != "https" && !is_loopback_http {
        return Err(EndpointPolicyError::Insecure);
    }
    if let Some(address) = host_ip {
        if !address.is_loopback()
            && (is_private_or_reserved(address)
                || address.is_unspecified()
                || address.is_multicast())
        {
            return Err(EndpointPolicyError::Private);
        }
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    let mut normalized = url.to_string();
    while normalized.ends_with('/') {
        normalized.pop();
    }
    if normalized.is_empty() {
        return Err(EndpointPolicyError::Malformed);
    }
    Ok(normalized)
}

fn is_private_or_reserved(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_broadcast(),
        IpAddr::V6(v6) => {
            // Unique local (fc00::/7) and link-local (fe80::/10).
            let segments = v6.segments();
            let unique_local = (segments[0] & 0xfe00) == 0xfc00;
            let link_local = (segments[0] & 0xffc0) == 0xfe80;
            unique_local || link_local
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_public_hosts_and_loopback_http() {
        assert_eq!(
            normalize_discovery_peer_endpoint("https://peer.example/path?x=1").unwrap(),
            "https://peer.example"
        );
        assert_eq!(
            normalize_discovery_peer_endpoint("http://localhost:9090/").unwrap(),
            "http://localhost:9090"
        );
        assert_eq!(
            normalize_discovery_peer_endpoint("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn rejects_insecure_and_private_literals() {
        assert_eq!(
            normalize_discovery_peer_endpoint("http://peer.example").unwrap_err(),
            EndpointPolicyError::Insecure
        );
        assert_eq!(
            normalize_discovery_peer_endpoint("https://192.168.1.10").unwrap_err(),
            EndpointPolicyError::Private
        );
        assert_eq!(
            normalize_discovery_peer_endpoint("https://10.1.2.3").unwrap_err(),
            EndpointPolicyError::Private
        );
    }
}
