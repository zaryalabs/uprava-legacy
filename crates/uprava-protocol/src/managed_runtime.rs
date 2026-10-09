//! Provider-neutral contracts for managed Agent runtime execution.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::*;

/// Execution contract selected for an Agent runtime lineage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionProfile {
    Managed,
    /// Legacy serde/event fallback. New Agent session admission defaults explicitly to Managed.
    #[default]
    ExecCompatibility,
}

/// Lifecycle of one concrete provider process/connection incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAttemptState {
    Starting,
    Ready,
    Disconnected,
    Reconnecting,
    Recovered,
    Stopping,
    Stopped,
    Failed,
    Lost,
}

/// Capability facts used by Core when admitting a provider runtime profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProviderRuntimeCapability {
    #[serde(rename = "provider.codex.exec")]
    CodexExec,
    #[serde(rename = "provider.codex.managed")]
    CodexManaged,
    #[serde(rename = "provider.codex.managed.approval")]
    CodexManagedApproval,
    #[serde(rename = "provider.codex.managed.interrupt")]
    CodexManagedInterrupt,
    #[serde(rename = "provider.codex.managed.resume")]
    CodexManagedResume,
}

impl ProviderRuntimeCapability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CodexExec => "provider.codex.exec",
            Self::CodexManaged => "provider.codex.managed",
            Self::CodexManagedApproval => "provider.codex.managed.approval",
            Self::CodexManagedInterrupt => "provider.codex.managed.interrupt",
            Self::CodexManagedResume => "provider.codex.managed.resume",
        }
    }

    #[must_use]
    pub const fn required_for_managed_codex() -> &'static [Self] {
        &[
            Self::CodexManaged,
            Self::CodexManagedApproval,
            Self::CodexManagedInterrupt,
            Self::CodexManagedResume,
        ]
    }
}

/// Provider request that blocks or otherwise requires human input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInteractionKind {
    Approval,
    UserInput,
}

/// Durable lifecycle of a provider interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInteractionState {
    Requested,
    /// Core durably accepted a decision, but the provider has not confirmed it yet.
    Resolving,
    Approved,
    Denied,
    Answered,
    Expired,
    Cancelled,
    Superseded,
}

/// Recovery state visible to Core and Web after transport or process loss.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRecoveryStatus {
    #[default]
    NotRequired,
    Live,
    Reconnecting,
    Recovered,
    ProviderResumable,
    Degraded,
    Lost,
    Failed,
}

/// Provider sandbox mode captured before a runtime attempt starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderSandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

/// Provider approval posture captured before a runtime attempt starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApprovalMode {
    Untrusted,
    OnFailure,
    OnRequest,
    Never,
}

/// Network enforcement represented by the selected provider protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeNetworkPosture {
    Denied,
    Restricted,
    Allowed,
    ProviderDefault,
    Unsupported,
}

/// Bounded, secret-free summary of MCP/tool exposure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeToolExposureSummary {
    pub server_count: u32,
    pub tool_count: u32,
    #[serde(default)]
    pub server_names: Vec<String>,
}

/// Explicit time-bounded unsafe policy override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePolicyOverride {
    pub actor: ActorRef,
    pub reason: String,
    pub expires_at: DateTime<Utc>,
}

/// Immutable effective runtime policy calculated by Core before dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveRuntimePolicy {
    pub contract_version: u32,
    pub execution_profile: AgentExecutionProfile,
    pub provider: String,
    pub provider_version: Option<String>,
    #[serde(default)]
    pub provider_capabilities: Vec<ProviderRuntimeCapability>,
    pub sandbox_mode: ProviderSandboxMode,
    pub approval_mode: ProviderApprovalMode,
    pub workspace_root: String,
    #[serde(default)]
    pub additional_writable_paths: Vec<String>,
    pub network_posture: RuntimeNetworkPosture,
    pub tool_exposure: RuntimeToolExposureSummary,
    pub credential_profile_ref: Option<String>,
    pub unsafe_override: Option<RuntimePolicyOverride>,
    /// Provider-neutral bounded metadata. Secrets and raw provider config are forbidden.
    #[serde(default)]
    pub capability_metadata: BTreeMap<String, String>,
}

impl EffectiveRuntimePolicy {
    /// Computes a stable SHA-256 over the serde JSON representation.
    ///
    /// # Errors
    /// Returns a serialization error if a future policy field cannot be encoded as JSON.
    pub fn policy_hash(&self) -> Result<RuntimePolicyHash, serde_json::Error> {
        let mut canonical = self.clone();
        canonical.provider_capabilities.sort_unstable();
        canonical.additional_writable_paths.sort_unstable();
        canonical.tool_exposure.server_names.sort_unstable();
        let encoded = serde_json::to_vec(&canonical)?;
        let digest = Sha256::digest(encoded);
        Ok(RuntimePolicyHash(format!("sha256:{digest:x}")))
    }
}

/// Deterministic digest of an [`EffectiveRuntimePolicy`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimePolicyHash(pub String);

impl RuntimePolicyHash {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Current durable attempt projection exposed with a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttemptSummary {
    pub runtime_attempt_id: RuntimeAttemptId,
    pub state: RuntimeAttemptState,
    pub started_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub start_reason: String,
    pub stop_reason: Option<String>,
    pub recovery_reason: Option<String>,
}

/// Secret-free Node actual-state fact used to reconcile Core after reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttemptActualState {
    pub runtime_session_id: RuntimeSessionId,
    pub runtime_attempt_id: RuntimeAttemptId,
    pub state: RuntimeAttemptState,
    pub effective_policy_hash: RuntimePolicyHash,
    pub active_turn_id: Option<TurnId>,
    pub started_at: DateTime<Utc>,
}

/// Pending or resolved provider interaction exposed to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInteractionSummary {
    pub provider_interaction_id: ProviderInteractionId,
    pub runtime_attempt_id: RuntimeAttemptId,
    pub kind: ProviderInteractionKind,
    pub state: ProviderInteractionState,
    pub prompt: String,
    pub requested_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> EffectiveRuntimePolicy {
        EffectiveRuntimePolicy {
            contract_version: 1,
            execution_profile: AgentExecutionProfile::Managed,
            provider: "codex".to_owned(),
            provider_version: Some("0.144.1".to_owned()),
            provider_capabilities: ProviderRuntimeCapability::required_for_managed_codex().to_vec(),
            sandbox_mode: ProviderSandboxMode::WorkspaceWrite,
            approval_mode: ProviderApprovalMode::Untrusted,
            workspace_root: "/workspace".to_owned(),
            additional_writable_paths: vec!["/tmp/uprava".to_owned()],
            network_posture: RuntimeNetworkPosture::Unsupported,
            tool_exposure: RuntimeToolExposureSummary {
                server_count: 1,
                tool_count: 2,
                server_names: vec!["uprava".to_owned()],
            },
            credential_profile_ref: Some("codex:default".to_owned()),
            unsafe_override: None,
            capability_metadata: BTreeMap::from([(
                "transport".to_owned(),
                "app-server-v2".to_owned(),
            )]),
        }
    }

    #[test]
    fn policy_hash_is_deterministic_for_the_same_snapshot() {
        let policy = policy();

        let first = policy.policy_hash().expect("policy hashes");
        let second = policy.policy_hash().expect("policy hashes again");

        assert_eq!(first, second);
    }

    #[test]
    fn policy_hash_changes_when_enforcement_changes() {
        let first = policy();
        let mut second = first.clone();
        second.approval_mode = ProviderApprovalMode::Never;

        assert_ne!(
            first.policy_hash().expect("first policy hashes"),
            second.policy_hash().expect("second policy hashes")
        );
    }

    #[test]
    fn policy_hash_ignores_set_like_field_order() {
        let first = policy();
        let mut second = first.clone();
        second.provider_capabilities.reverse();
        second.additional_writable_paths.reverse();
        second.tool_exposure.server_names.reverse();

        assert_eq!(
            first.policy_hash().expect("first policy hashes"),
            second.policy_hash().expect("second policy hashes")
        );
    }
}
