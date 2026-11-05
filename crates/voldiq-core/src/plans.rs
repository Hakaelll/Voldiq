//! Exact immutable manifests and single-use human grants. No file mutation is performed here.
mod catalogue;
#[cfg(windows)]
mod copying;
mod desktop;
mod human;
mod offload;
#[cfg(windows)]
mod recycling;
mod store;
pub use crate::cleanup::record::FileEvidence;
use crate::{Error, Result};
pub use catalogue::{TransferCatalogue, TransferFile, TransferPhase, TransferUpdate};
#[cfg(windows)]
pub use copying::{CopyFailure, CopyPublication, PublishedContent, TransferWorker};
pub use desktop::{Desktop, Review, ReviewPage};
pub use human::{Begin, HumanAuthority};
pub use offload::{Destination, Offload, OffloadRequest, Originals, VolumeBinding};
#[cfg(windows)]
pub use recycling::{PreparedRecycling, RecyclingWorker};
use serde::{Deserialize, Serialize};
pub use store::{Enqueue, ExecutionRequest, Store};

const POLICY_VERSION: u32 = 2;
const PLAN_TTL: i64 = 900;
const APPROVAL_TTL: i64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Draft,
    AwaitingApproval,
    Approved,
    Rejected,
    Expired,
    Executing,
    Completed,
    PartiallyCompleted,
    Failed,
    Cancelled,
    Invalidated,
}

impl State {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::PartiallyCompleted => "partially_completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Invalidated => "invalidated",
        }
    }
    pub(super) fn terminal(self) -> bool {
        !matches!(
            self,
            Self::Draft | Self::AwaitingApproval | Self::Approved | Self::Executing
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub policy_version: u32,
    pub id: String,
    pub connection_id: i64,
    pub user: String,
    pub source_snapshot: i64,
    pub root: String,
    pub root_identity: String,
    pub exclusions_count: usize,
    pub action: Action,
    pub risk: String,
    pub created: i64,
    pub expires: i64,
    pub entries: Vec<FileEvidence>,
    pub logical_bytes: String,
    pub potential_reclaim_bytes: String,
    pub immediate_free_bytes: String,
    pub native_record_hash: String,
    pub protection_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Recycle,
    Offload(Box<Offload>),
}

impl Action {
    fn schema_version(&self) -> u32 {
        match self {
            Self::Recycle => 2,
            Self::Offload(_) => 3,
        }
    }
    pub fn execution_available(&self) -> bool {
        matches!(self, Self::Recycle)
    }
}

#[derive(Serialize)]
pub struct View {
    pub manifest: Manifest,
    pub hash: String,
    pub state: State,
    pub approval_expires: Option<i64>,
    pub result: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct AuditEvent {
    pub sequence: i64,
    pub at: i64,
    pub event: String,
}

/// Opaque execution ticket. It is not serializable or cloneable and cannot be reconstructed from a database row.
pub struct ExecutionLease {
    id: String,
    hash: String,
    owner: i64,
    nonce: String,
    directory: (u64, u64),
    #[cfg(windows)]
    runtime: Box<RuntimeBinding>,
}

#[cfg(windows)]
struct RuntimeBinding {
    issuer: String,
    policy: String,
    deadline: i64,
    created: i64,
    user: String,
    protection_fingerprint: String,
}

fn invalid() -> Error {
    Error::Invalid("Invalid plan, changed evidence or incompatible plan state.".into())
}

fn random_id() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| invalid())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn current_user() -> Result<String> {
    #[cfg(windows)]
    {
        Ok(crate::windows::security::current_user_sid()?)
    }
    #[cfg(unix)]
    {
        Ok(unsafe { libc::geteuid() }.to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(invalid())
    }
}

fn digest(value: &[u8]) -> String {
    blake3::hash(value).to_hex().to_string()
}

pub(crate) fn now() -> Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|time| i64::try_from(time.as_secs()).ok())
        .ok_or_else(invalid)
}
