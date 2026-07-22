//! Open Bootstrap admission for Discovery Peer Advertisements.

use super::endpoint::normalize_discovery_peer_endpoint;
use super::probe::{DiscoveryPeerProbe, DiscoveryPeerProbeError};
use super::types::{
    estimated_payload_bytes, peer_rate_limit_would_exceed, record_peer_admission_attempt,
    MAX_PEER_ADVERTISEMENT_PAYLOAD_BYTES,
};
use crate::domain::{
    BootstrapAdmissionOutcomeKind, DiscoveryPeerAdmissionAcceptance,
    DiscoveryPeerAdmissionRejectionReason, DiscoveryPeerAdvertisement, DiscoveryPeerCapability,
    DiscoveryPeerIdentityView, KnownDiscoveryPeerAdvertisement, CURRENT_PROTOCOL_VERSION,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};

/// Admits a signed Discovery Peer Advertisement on a Bootstrap-capable node.
///
/// Verifies identity, signature, lease, protocol, capability, public endpoint
/// policy, reachability, identity match against the live probe view, and rate
/// limits. Does not assign trust, rank, or quality.
///
/// # Errors
///
/// Returns a stable [`DiscoveryPeerAdmissionRejectionReason`] on policy or
/// verification failure.
pub fn admit_discovery_peer_advertisement(
    store: &mut InMemoryStore,
    advertisement: DiscoveryPeerAdvertisement,
    probe: &dyn DiscoveryPeerProbe,
    bootstrap_enabled: bool,
    now: DateTime<Utc>,
) -> Result<DiscoveryPeerAdmissionAcceptance, DiscoveryPeerAdmissionRejectionReason> {
    if !bootstrap_enabled {
        return Err(DiscoveryPeerAdmissionRejectionReason::BootstrapDisabled);
    }

    if estimated_payload_bytes(&advertisement) > MAX_PEER_ADVERTISEMENT_PAYLOAD_BYTES {
        return Err(DiscoveryPeerAdmissionRejectionReason::PayloadTooLarge);
    }

    if advertisement.node_id != advertisement.signer.node_id
        || advertisement.signer.public_key.trim().is_empty()
    {
        return Err(DiscoveryPeerAdmissionRejectionReason::InvalidIdentity);
    }

    if advertisement.protocol_version != CURRENT_PROTOCOL_VERSION
        || advertisement.signer.supported_protocol_version != CURRENT_PROTOCOL_VERSION
    {
        return Err(DiscoveryPeerAdmissionRejectionReason::IncompatibleProtocol);
    }

    if advertisement.capability != DiscoveryPeerCapability::AnnouncementServing {
        return Err(DiscoveryPeerAdmissionRejectionReason::UnsupportedCapability);
    }

    let endpoint = normalize_discovery_peer_endpoint(&advertisement.public_endpoint)
        .map_err(|error| error.as_rejection())?;
    if endpoint != advertisement.public_endpoint {
        // Require advertisements to already carry a normalized endpoint so
        // signature coverage matches the served address exactly.
        return Err(DiscoveryPeerAdmissionRejectionReason::Malformed);
    }

    if !advertisement.lease_is_active(now) {
        return Err(DiscoveryPeerAdmissionRejectionReason::StaleLease);
    }

    match advertisement.verify() {
        Ok(true) => {}
        Ok(false) => return Err(DiscoveryPeerAdmissionRejectionReason::InvalidSignature),
        Err(_) => return Err(DiscoveryPeerAdmissionRejectionReason::InvalidSignature),
    }

    // Idempotent replay of the exact signed advertisement.
    if let Some(existing) = store
        .known_discovery_peer_advertisements
        .get(&advertisement.node_id)
    {
        if existing.advertisement == advertisement {
            return Ok(DiscoveryPeerAdmissionAcceptance {
                outcome: BootstrapAdmissionOutcomeKind::Idempotent,
                known: existing.clone(),
            });
        }
    }

    if peer_rate_limit_would_exceed(store, advertisement.node_id, now) {
        return Err(DiscoveryPeerAdmissionRejectionReason::RateLimited);
    }

    let view = probe
        .probe_peer_endpoint(&advertisement.public_endpoint)
        .map_err(|error| match error {
            DiscoveryPeerProbeError::Unreachable => {
                DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
            }
        })?;
    ensure_identity_matches_advertisement(&advertisement, &view)?;

    let known = KnownDiscoveryPeerAdvertisement {
        advertisement: advertisement.clone(),
        received_at: now,
        learned_from: std::collections::BTreeSet::new(),
    };
    let is_new = !store
        .known_discovery_peer_advertisements
        .contains_key(&advertisement.node_id);
    store
        .known_discovery_peer_advertisements
        .insert(advertisement.node_id, known.clone());
    record_peer_admission_attempt(store, advertisement.node_id, now);

    Ok(DiscoveryPeerAdmissionAcceptance {
        outcome: if is_new {
            BootstrapAdmissionOutcomeKind::Admitted
        } else {
            BootstrapAdmissionOutcomeKind::Renewed
        },
        known,
    })
}

fn ensure_identity_matches_advertisement(
    advertisement: &DiscoveryPeerAdvertisement,
    view: &DiscoveryPeerIdentityView,
) -> Result<(), DiscoveryPeerAdmissionRejectionReason> {
    if view.node_id != advertisement.node_id
        || view.public_key != advertisement.signer.public_key
        || view.protocol_version != advertisement.protocol_version
    {
        return Err(DiscoveryPeerAdmissionRejectionReason::IdentityMismatch);
    }
    Ok(())
}
