//! The domain model, split by area. Every submodule glob re-exports here, so
//! `crate::domain::*` (and the crate root's `pub use domain::*`) still exposes
//! one flat namespace. Submodules import shared externals via `use super::*;`.

mod announcements;
mod bootstrap;
mod candidates;
mod content;
mod discovery;
mod discovery_peers;
mod federation;
mod feed;
mod harness;
mod ids;
mod personal_discovery;
mod pods;
mod requests;
mod taste;
mod trust;
mod user_context;

pub use announcements::*;
pub use bootstrap::*;
pub use candidates::*;
pub use content::*;
pub use discovery::*;
pub use discovery_peers::*;
pub use federation::*;
pub use feed::*;
pub use harness::*;
pub use ids::*;
pub use personal_discovery::*;
pub use pods::*;
pub use requests::*;
pub use taste::*;
pub use trust::*;
pub use user_context::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
