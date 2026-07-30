//! Self-describing catalog of the network API surface (`/openapi-lite`).

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiRouteDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
}


pub(crate) fn route_docs() -> Vec<ApiRouteDoc> {
    vec![
        ApiRouteDoc {
            method: "GET",
            path: "/health",
            description: "health check",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/.well-known/stumble-node",
            description: "custom Stumble node metadata and endpoint discovery",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/bootstrap/announcements",
            description: "open Bootstrap admission of a signed public Pod Announcement",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/bootstrap/announcements/stream",
            description: "cursor-paginated Announcement Stream of Bootstrap-admitted public Pods",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/bootstrap/withdrawals",
            description: "open Bootstrap admission of an Origin-signed Pod Withdrawal",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/pods",
            description: "list this Origin Node's public Pods",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/pods/:slug/manifest",
            description: "public Pod manifest with latest event hash and package version",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/bootstrap/peer-advertisements",
            description: "open Bootstrap admission of a signed Discovery Peer Advertisement",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/bootstrap/peer-advertisements",
            description: "small randomized unranked sample of Bootstrap-admitted peer advertisements",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/peer/announcements/stream",
            description: "opt-in Discovery Peer Announcement Stream pages (Origin signatures unchanged)",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/peer/advertisements",
            description: "small randomized unranked sample of current peer advertisements",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements",
            description: "verify and index a signed public Pod Announcement with its Announcement Lease",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/announcements",
            description: "Index search of eligible Pod Announcements by explicit query only (no User id)",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements/produce",
            description: "produce an Origin-signed Pod Announcement with a renewable 30-day lease",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements/receive",
            description: "receive a peer-delivered Origin-signed Pod Announcement",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals",
            description: "verify and index an Origin-signed Pod Withdrawal",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals/produce",
            description: "produce an Origin-signed Pod Withdrawal, optionally making the Pod private",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals/receive",
            description: "receive a peer-delivered Origin-signed Pod Withdrawal",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/node",
            description: "node public identity and protocol version",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/pods/:slug/events",
            description: "export signed public events",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/federation/pods/:slug/events",
            description: "import signed public events from a trusted peer",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/federation/pods/:slug/explore-samples",
            description: "bounded Origin-signed Explore samples for the exact current announcement",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/federation/sync/:peer_id/:pod_slug",
            description: "synchronize signed events from a trusted peer",
        },
    ]
}
