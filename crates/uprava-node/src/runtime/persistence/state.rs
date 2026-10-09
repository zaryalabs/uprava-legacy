//! Versioned local state, the single-owner state actor and SQLite durability.

use super::super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct NodeLocalState {
    #[serde(default = "default_node_state_slot")]
    pub(crate) state_slot: String,
    #[serde(default = "default_node_state_schema_version")]
    pub(crate) schema_version: u32,
    #[serde(default = "new_daemon_installation_id")]
    pub(crate) daemon_installation_id: String,
    pub(crate) node_id: Option<NodeId>,
    pub(crate) credential: Option<String>,
    pub(crate) enrollment_id: Option<EnrollmentId>,
    pub(crate) pairing_code: Option<String>,
    #[serde(default)]
    pub(crate) command_status: HashMap<String, CommandState>,
    #[serde(default)]
    pub(crate) command_result_payloads: HashMap<String, JsonValue>,
    #[serde(default)]
    pub(crate) runtime_seqs: HashMap<String, i64>,
    #[serde(default)]
    pub(crate) event_outbox: Vec<EventEnvelope>,
    #[serde(default)]
    pub(crate) runtime_providers: HashMap<String, String>,
    #[serde(default)]
    pub(crate) runtime_workspace_paths: HashMap<String, String>,
    #[serde(default)]
    pub(crate) runtime_execution_profiles: HashMap<String, AgentExecutionProfile>,
    #[serde(default)]
    pub(crate) managed_attempts: HashMap<String, ManagedAttemptDescriptor>,
    #[serde(default)]
    pub(crate) runtime_states: HashMap<String, RuntimeSessionState>,
    #[serde(default)]
    pub(crate) runtime_transcripts: HashMap<String, Vec<ProviderTranscriptMessage>>,
    #[serde(default)]
    pub(crate) runtime_provider_resume_refs: HashMap<String, ProviderResumeRef>,
    #[serde(default)]
    pub(crate) cancelled_deductions: HashSet<String>,
    #[serde(default)]
    pub(crate) tool_dependencies: HashMap<String, ToolDependencyDesired>,
    #[serde(default)]
    pub(crate) task_runtime_mappings: HashMap<String, TaskRuntimeMapping>,
    #[serde(default)]
    pub(crate) placement_seqs: HashMap<String, i64>,
    #[serde(default)]
    pub(crate) reconnect_attempts: u64,
    #[serde(default)]
    pub(crate) dropped_event_count: u64,
    #[serde(default)]
    pub(crate) heartbeat_failures: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProviderTranscriptMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProviderResumeRef {
    #[serde(default)]
    pub(crate) provider_session_id: Option<String>,
    #[serde(default)]
    pub(crate) resume_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TaskRuntimeMapping {
    pub(crate) task_run_id: uprava_protocol::TaskRunId,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) sandbox_id: Option<String>,
    pub(crate) updated_at: DateTime<Utc>,
}

impl Default for NodeLocalState {
    fn default() -> Self {
        Self {
            state_slot: default_node_state_slot(),
            schema_version: default_node_state_schema_version(),
            daemon_installation_id: new_daemon_installation_id(),
            node_id: None,
            credential: None,
            enrollment_id: None,
            pairing_code: None,
            command_status: HashMap::new(),
            command_result_payloads: HashMap::new(),
            runtime_seqs: HashMap::new(),
            event_outbox: Vec::new(),
            runtime_providers: HashMap::new(),
            runtime_workspace_paths: HashMap::new(),
            runtime_execution_profiles: HashMap::new(),
            managed_attempts: HashMap::new(),
            runtime_states: HashMap::new(),
            runtime_transcripts: HashMap::new(),
            runtime_provider_resume_refs: HashMap::new(),
            cancelled_deductions: HashSet::new(),
            tool_dependencies: HashMap::new(),
            task_runtime_mappings: HashMap::new(),
            placement_seqs: HashMap::new(),
            reconnect_attempts: 0,
            dropped_event_count: 0,
            heartbeat_failures: 0,
        }
    }
}

impl std::fmt::Debug for NodeLocalState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let runtime_transcript_counts = self
            .runtime_transcripts
            .iter()
            .map(|(runtime_id, transcript)| (runtime_id, transcript.len()))
            .collect::<BTreeMap<_, _>>();
        let runtime_provider_resume_ref_count = self.runtime_provider_resume_refs.len();
        formatter
            .debug_struct("NodeLocalState")
            .field("state_slot", &self.state_slot)
            .field("schema_version", &self.schema_version)
            .field("daemon_installation_id", &self.daemon_installation_id)
            .field("node_id", &self.node_id)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "[redacted]"),
            )
            .field("enrollment_id", &self.enrollment_id)
            .field(
                "pairing_code",
                &self.pairing_code.as_ref().map(|_| "[redacted]"),
            )
            .field("command_status", &self.command_status)
            .field(
                "command_result_payload_count",
                &self.command_result_payloads.len(),
            )
            .field("runtime_seqs", &self.runtime_seqs)
            .field("event_outbox", &self.event_outbox)
            .field("runtime_providers", &self.runtime_providers)
            .field("runtime_workspace_paths", &self.runtime_workspace_paths)
            .field(
                "runtime_execution_profiles",
                &self.runtime_execution_profiles,
            )
            .field("managed_attempts", &self.managed_attempts)
            .field("runtime_states", &self.runtime_states)
            .field("runtime_transcript_counts", &runtime_transcript_counts)
            .field(
                "runtime_provider_resume_ref_count",
                &runtime_provider_resume_ref_count,
            )
            .field("cancelled_deductions", &self.cancelled_deductions)
            .field("tool_dependency_count", &self.tool_dependencies.len())
            .field(
                "task_runtime_mapping_count",
                &self.task_runtime_mappings.len(),
            )
            .field("placement_seqs", &self.placement_seqs)
            .field("reconnect_attempts", &self.reconnect_attempts)
            .field("dropped_event_count", &self.dropped_event_count)
            .field("heartbeat_failures", &self.heartbeat_failures)
            .finish()
    }
}

pub(crate) fn new_daemon_installation_id() -> String {
    format!("daemon-{}", Uuid::new_v4())
}

pub(crate) fn default_node_state_slot() -> String {
    NODE_STATE_SLOT.to_owned()
}

pub(crate) fn default_node_state_schema_version() -> u32 {
    NODE_STATE_SCHEMA_VERSION
}

pub(crate) fn merge_changed_map<K, V>(
    owner: &mut HashMap<K, V>,
    baseline: &HashMap<K, V>,
    candidate: &HashMap<K, V>,
) where
    K: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    let keys = baseline
        .keys()
        .chain(candidate.keys())
        .cloned()
        .collect::<HashSet<_>>();
    for key in keys {
        match (baseline.get(&key), candidate.get(&key)) {
            (Some(previous), Some(next)) if previous != next => {
                owner.insert(key, next.clone());
            }
            (None, Some(next)) => {
                owner.insert(key, next.clone());
            }
            (Some(previous), None) if owner.get(&key) == Some(previous) => {
                owner.remove(&key);
            }
            _ => {}
        }
    }
}

impl NodeLocalState {
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            if let Some(legacy_path) = legacy_state_path(path) {
                if legacy_path.exists() {
                    anyhow::bail!(
                        "legacy Uprava Node state found at {}; state slot {} is isolated; move or remove the legacy state and re-enroll",
                        legacy_path.display(),
                        NODE_STATE_SLOT
                    );
                }
            }
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read node state {}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse node state {}", path.display()))?;
        if is_versioned_state_path(path) {
            let slot = value.get("state_slot").and_then(serde_json::Value::as_str);
            let schema_version = value
                .get("schema_version")
                .and_then(serde_json::Value::as_u64);
            if slot != Some(NODE_STATE_SLOT)
                || schema_version != Some(NODE_STATE_SCHEMA_VERSION as u64)
            {
                anyhow::bail!(
                    "Node state at {} is not compatible with slot {} schema {}; move it aside and re-enroll",
                    path.display(),
                    NODE_STATE_SLOT,
                    NODE_STATE_SCHEMA_VERSION
                );
            }
        }
        serde_json::from_value(value)
            .with_context(|| format!("failed to decode node state {}", path.display()))
    }

    pub(crate) fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
            set_private_dir_permissions(parent);
        }
        let mut snapshot = self.clone();
        snapshot.compact_for_persistence();
        let content =
            serde_json::to_string_pretty(&snapshot).context("failed to serialize node state")?;
        write_private_file(path, content.as_bytes())
            .with_context(|| format!("failed to write node state {}", path.display()))
    }

    pub(crate) async fn load_async(path: &Path) -> anyhow::Result<Self> {
        if !is_sqlite_state_path(path) {
            return Self::load(path);
        }
        if !path.exists() {
            if let Some(legacy_path) = legacy_state_path(path) {
                if legacy_path.exists() {
                    anyhow::bail!(
                        "legacy Uprava Node state found at {}; state slot {} is isolated; move or remove the legacy state and re-enroll",
                        legacy_path.display(),
                        NODE_STATE_SLOT
                    );
                }
            }
        }
        let pool = open_state_store(path).await?;
        initialize_state_store(&pool).await?;
        let row = sqlx::query(
            "select state_slot, schema_version, snapshot_json from node_state where state_id = 1",
        )
        .fetch_optional(&pool)
        .await?;
        let Some(row) = row else {
            pool.close().await;
            return Ok(Self::default());
        };
        let slot: String = row.try_get("state_slot")?;
        let schema_version: i64 = row.try_get("schema_version")?;
        if slot != NODE_STATE_SLOT || schema_version != NODE_STATE_SCHEMA_VERSION as i64 {
            pool.close().await;
            anyhow::bail!(
                "Node state at {} is not compatible with slot {} schema {}; move it aside and re-enroll",
                path.display(),
                NODE_STATE_SLOT,
                NODE_STATE_SCHEMA_VERSION
            );
        }
        let snapshot: String = row.try_get("snapshot_json")?;
        pool.close().await;
        let mut state: Self = serde_json::from_str(&snapshot)
            .with_context(|| format!("failed to decode node state {}", path.display()))?;
        let pool = open_state_store(path).await?;
        hydrate_from_normalized_tables(&pool, &mut state).await?;
        pool.close().await;
        Ok(state)
    }

    pub(crate) async fn save_async(&self, path: &Path) -> anyhow::Result<()> {
        if !is_sqlite_state_path(path) {
            return self.save(path);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
            set_private_dir_permissions(parent);
        }
        let mut snapshot = self.clone();
        snapshot.compact_for_persistence();
        let snapshot_json = serde_json::to_string(&snapshot.sqlite_compatibility_seed())
            .context("failed to serialize node state snapshot")?;
        let pool = open_state_store(path).await?;
        initialize_state_store(&pool).await?;
        let mut transaction = pool.begin().await?;
        sqlx::query(
            r#"
            insert into node_state (state_id, state_slot, schema_version, snapshot_json, updated_at)
            values (1, ?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            on conflict(state_id) do update set
                state_slot = excluded.state_slot,
                schema_version = excluded.schema_version,
                snapshot_json = excluded.snapshot_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(NODE_STATE_SLOT)
        .bind(NODE_STATE_SCHEMA_VERSION as i64)
        .bind(snapshot_json)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("delete from node_command_cache")
            .execute(&mut *transaction)
            .await?;
        for (command_id, status) in &snapshot.command_status {
            sqlx::query(
                "insert into node_command_cache (command_id, state, result_payload_json, updated_at) values (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(command_id)
            .bind(command_state_storage(*status))
            .bind(
                snapshot
                    .command_result_payloads
                    .get(command_id)
                    .map(serde_json::to_string)
                    .transpose()?,
            )
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("delete from node_event_outbox")
            .execute(&mut *transaction)
            .await?;
        for event in &snapshot.event_outbox {
            sqlx::query(
                "insert into node_event_outbox (event_id, event_json, seq, created_at) values (?1, ?2, ?3, ?4)",
            )
            .bind(event.event_id.as_str())
            .bind(serde_json::to_string(event)?)
            .bind(event.seq)
            .bind(event.happened_at)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("delete from node_registration")
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "insert into node_registration (state_id, daemon_installation_id, node_id, credential, enrollment_id, pairing_code, updated_at) values (1, ?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(&snapshot.daemon_installation_id)
        .bind(snapshot.node_id.as_ref().map(NodeId::as_str))
        .bind(snapshot.credential.as_deref())
        .bind(snapshot.enrollment_id.as_ref().map(EnrollmentId::as_str))
        .bind(snapshot.pairing_code.as_deref())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            insert into node_metrics (
                state_id, reconnect_attempts, dropped_event_count, heartbeat_failures, updated_at
            )
            values (1, ?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            on conflict(state_id) do update set
                reconnect_attempts = excluded.reconnect_attempts,
                dropped_event_count = excluded.dropped_event_count,
                heartbeat_failures = excluded.heartbeat_failures,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(snapshot.reconnect_attempts as i64)
        .bind(snapshot.dropped_event_count as i64)
        .bind(snapshot.heartbeat_failures as i64)
        .execute(&mut *transaction)
        .await?;

        let runtime_ids = snapshot
            .runtime_seqs
            .keys()
            .chain(snapshot.runtime_providers.keys())
            .chain(snapshot.runtime_workspace_paths.keys())
            .chain(snapshot.runtime_states.keys())
            .chain(snapshot.runtime_transcripts.keys())
            .chain(snapshot.runtime_provider_resume_refs.keys())
            .cloned()
            .collect::<HashSet<_>>();
        sqlx::query("delete from node_runtime_metadata")
            .execute(&mut *transaction)
            .await?;
        for runtime_id in runtime_ids {
            sqlx::query(
                "insert into node_runtime_metadata (runtime_session_id, runtime_seq, provider, workspace_path, state_json, transcript_json, resume_ref_json, updated_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(&runtime_id)
            .bind(snapshot.runtime_seqs.get(&runtime_id).copied())
            .bind(snapshot.runtime_providers.get(&runtime_id))
            .bind(snapshot.runtime_workspace_paths.get(&runtime_id))
            .bind(
                snapshot
                    .runtime_states
                    .get(&runtime_id)
                    .map(serde_json::to_string)
                    .transpose()?,
            )
            .bind(
                snapshot
                    .runtime_transcripts
                    .get(&runtime_id)
                    .map(serde_json::to_string)
                    .transpose()?,
            )
            .bind(
                snapshot
                    .runtime_provider_resume_refs
                    .get(&runtime_id)
                    .map(serde_json::to_string)
                    .transpose()?,
            )
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("delete from node_placement_sequences")
            .execute(&mut *transaction)
            .await?;
        for (placement_id, seq) in &snapshot.placement_seqs {
            sqlx::query(
                "insert into node_placement_sequences (placement_id, seq, updated_at) values (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(placement_id)
            .bind(*seq)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("delete from node_task_runtime_mappings")
            .execute(&mut *transaction)
            .await?;
        for mapping in snapshot.task_runtime_mappings.values() {
            sqlx::query(
                "insert into node_task_runtime_mappings (task_run_id, worktree_path, branch, sandbox_id, updated_at) values (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(mapping.task_run_id.as_str())
            .bind(&mapping.worktree_path)
            .bind(&mapping.branch)
            .bind(&mapping.sandbox_id)
            .bind(mapping.updated_at)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        pool.close().await;
        #[cfg(unix)]
        std::fs::set_permissions(path, PermissionsExt::from_mode(0o600))?;
        Ok(())
    }

    pub(crate) fn sqlite_compatibility_seed(&self) -> Self {
        Self {
            state_slot: self.state_slot.clone(),
            schema_version: self.schema_version,
            daemon_installation_id: self.daemon_installation_id.clone(),
            cancelled_deductions: self.cancelled_deductions.clone(),
            tool_dependencies: self.tool_dependencies.clone(),
            task_runtime_mappings: self.task_runtime_mappings.clone(),
            runtime_execution_profiles: self.runtime_execution_profiles.clone(),
            managed_attempts: self.managed_attempts.clone(),
            ..Self::default()
        }
    }

    pub(crate) fn compact_for_persistence(&mut self) {
        if self.command_status.len() > MAX_RETAINED_COMMANDS {
            let removable = self
                .command_status
                .iter()
                .filter(|(_, status)| {
                    matches!(
                        status,
                        CommandState::Completed
                            | CommandState::Failed
                            | CommandState::Blocked
                            | CommandState::Expired
                    )
                })
                .map(|(command_id, _)| command_id.clone())
                .collect::<Vec<_>>();
            let remove_count = self.command_status.len() - MAX_RETAINED_COMMANDS;
            for command_id in removable.into_iter().take(remove_count) {
                self.command_status.remove(&command_id);
                self.command_result_payloads.remove(&command_id);
            }
        }
        self.command_result_payloads
            .retain(|command_id, _| self.command_status.contains_key(command_id));
    }

    pub(crate) fn is_enrolled(&self) -> bool {
        self.node_id.is_some() && self.credential.is_some()
    }

    pub(crate) fn clear_core_registration(&mut self) {
        self.node_id = None;
        self.credential = None;
        self.enrollment_id = None;
        self.pairing_code = None;
    }

    pub(crate) fn clear_enrollment_attempt(&mut self) {
        self.enrollment_id = None;
        self.pairing_code = None;
    }

    pub(crate) fn merge_command_state_from(&mut self, baseline: &Self, command_state: &Self) {
        // Apply only changes made to the command snapshot. A stale snapshot
        // must never replace an ACKed outbox or newer runtime metadata.
        merge_changed_map(
            &mut self.command_status,
            &baseline.command_status,
            &command_state.command_status,
        );
        merge_changed_map(
            &mut self.command_result_payloads,
            &baseline.command_result_payloads,
            &command_state.command_result_payloads,
        );
        for event in &command_state.event_outbox {
            if !baseline
                .event_outbox
                .iter()
                .any(|old| old.event_id == event.event_id)
                && !self
                    .event_outbox
                    .iter()
                    .any(|old| old.event_id == event.event_id)
            {
                self.event_outbox.push(event.clone());
            }
        }
        for (runtime_id, seq) in &command_state.runtime_seqs {
            if baseline.runtime_seqs.get(runtime_id) != Some(seq) {
                let current = self.runtime_seqs.entry(runtime_id.clone()).or_default();
                *current = (*current).max(*seq);
            }
        }
        merge_changed_map(
            &mut self.runtime_providers,
            &baseline.runtime_providers,
            &command_state.runtime_providers,
        );
        merge_changed_map(
            &mut self.runtime_workspace_paths,
            &baseline.runtime_workspace_paths,
            &command_state.runtime_workspace_paths,
        );
        merge_changed_map(
            &mut self.runtime_execution_profiles,
            &baseline.runtime_execution_profiles,
            &command_state.runtime_execution_profiles,
        );
        merge_changed_map(
            &mut self.managed_attempts,
            &baseline.managed_attempts,
            &command_state.managed_attempts,
        );
        merge_changed_map(
            &mut self.runtime_states,
            &baseline.runtime_states,
            &command_state.runtime_states,
        );
        merge_changed_map(
            &mut self.runtime_transcripts,
            &baseline.runtime_transcripts,
            &command_state.runtime_transcripts,
        );
        merge_changed_map(
            &mut self.runtime_provider_resume_refs,
            &baseline.runtime_provider_resume_refs,
            &command_state.runtime_provider_resume_refs,
        );
        for deduction_id in command_state
            .cancelled_deductions
            .difference(&baseline.cancelled_deductions)
        {
            remember_cancelled_deduction(&mut self.cancelled_deductions, deduction_id.clone());
        }
        for deduction_id in baseline
            .cancelled_deductions
            .difference(&command_state.cancelled_deductions)
        {
            self.cancelled_deductions.remove(deduction_id);
        }
        merge_changed_map(
            &mut self.tool_dependencies,
            &baseline.tool_dependencies,
            &command_state.tool_dependencies,
        );
        merge_changed_map(
            &mut self.task_runtime_mappings,
            &baseline.task_runtime_mappings,
            &command_state.task_runtime_mappings,
        );
        for (placement_id, seq) in &command_state.placement_seqs {
            if baseline.placement_seqs.get(placement_id) != Some(seq) {
                let current = self.placement_seqs.entry(placement_id.clone()).or_default();
                *current = (*current).max(*seq);
            }
        }
        self.dropped_event_count = self
            .dropped_event_count
            .max(command_state.dropped_event_count);
        self.heartbeat_failures = self
            .heartbeat_failures
            .max(command_state.heartbeat_failures);
    }
}

pub(crate) fn remember_cancelled_deduction(tombstones: &mut HashSet<String>, deduction_id: String) {
    if tombstones.contains(&deduction_id) {
        return;
    }
    if tombstones.len() >= MAX_CANCELLED_DEDUCTION_TOMBSTONES {
        if let Some(expired) = tombstones.iter().next().cloned() {
            tombstones.remove(&expired);
        }
    }
    tombstones.insert(deduction_id);
}

/// The single owner boundary for durable Node state mutations.
///
/// Runtime tasks may keep a short-lived in-memory snapshot while doing I/O,
/// but every mutation that crosses the control path goes through this store.
#[derive(Clone)]
pub(crate) struct NodeStateStore {
    pub(crate) sender: mpsc::Sender<NodeStateRequest>,
    pub(crate) actor: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

pub(crate) enum NodeStateRequest {
    Snapshot {
        respond_to: oneshot::Sender<anyhow::Result<NodeLocalState>>,
    },
    IsEnrolled {
        respond_to: oneshot::Sender<anyhow::Result<bool>>,
    },
    Mutate {
        mutation: Box<NodeStateMutation>,
        respond_to: oneshot::Sender<anyhow::Result<NodeStateMutationResult>>,
    },
    Shutdown {
        respond_to: oneshot::Sender<()>,
    },
}

pub(crate) enum NodeStateMutation {
    PersistEnrollmentAttempt {
        enrollment_id: EnrollmentId,
        pairing_code: String,
    },
    ClearEnrollmentAttempt,
    PersistEnrollmentIdentity {
        node_id: NodeId,
        credential: String,
    },
    ClearCoreRegistration,
    PersistReconnectAttempt,
    PersistHeartbeatFailure,
    PersistEventAck {
        accepted_event_ids: Vec<EventId>,
    },
    MergeCommandState {
        baseline: Box<NodeLocalState>,
        command_state: Box<NodeLocalState>,
    },
    UpsertTaskRuntimeMapping {
        mapping: TaskRuntimeMapping,
    },
    RemoveTaskRuntimeMapping {
        task_run_id: uprava_protocol::TaskRunId,
    },
}

pub(crate) enum NodeStateMutationResult {
    Unit,
    RemovedEvents(usize),
}

impl NodeStateStore {
    pub(crate) fn new(state: NodeLocalState, path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel(NODE_STATE_STORE_QUEUE_CAPACITY);
        let actor = tokio::spawn(run_node_state_store(state, path, receiver));
        Self {
            sender,
            actor: Arc::new(Mutex::new(Some(actor))),
        }
    }

    pub(crate) async fn snapshot(&self) -> anyhow::Result<NodeLocalState> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(NodeStateRequest::Snapshot { respond_to })
            .await
            .map_err(|_| anyhow::anyhow!("node state store task stopped"))?;
        response.await.context("node state store task stopped")?
    }

    pub(crate) async fn is_enrolled(&self) -> anyhow::Result<bool> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(NodeStateRequest::IsEnrolled { respond_to })
            .await
            .map_err(|_| anyhow::anyhow!("node state store task stopped"))?;
        response.await.context("node state store task stopped")?
    }

    pub(crate) async fn persist_enrollment_attempt(
        &self,
        enrollment_id: EnrollmentId,
        pairing_code: String,
    ) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::PersistEnrollmentAttempt {
            enrollment_id,
            pairing_code,
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn clear_enrollment_attempt(&self) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::ClearEnrollmentAttempt)
            .await?;
        Ok(())
    }

    pub(crate) async fn persist_enrollment_identity(
        &self,
        node_id: NodeId,
        credential: String,
    ) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::PersistEnrollmentIdentity {
            node_id,
            credential,
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn clear_core_registration(&self) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::ClearCoreRegistration)
            .await?;
        Ok(())
    }

    pub(crate) async fn persist_reconnect_attempt(&self) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::PersistReconnectAttempt)
            .await?;
        Ok(())
    }

    pub(crate) async fn persist_heartbeat_failure(&self) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::PersistHeartbeatFailure)
            .await?;
        Ok(())
    }

    pub(crate) async fn persist_event_ack(
        &self,
        accepted_event_ids: &[EventId],
    ) -> anyhow::Result<usize> {
        match self
            .mutate(NodeStateMutation::PersistEventAck {
                accepted_event_ids: accepted_event_ids.to_vec(),
            })
            .await?
        {
            NodeStateMutationResult::RemovedEvents(removed) => Ok(removed),
            NodeStateMutationResult::Unit => Ok(0),
        }
    }

    pub(crate) async fn merge_command_state(
        &self,
        baseline: &NodeLocalState,
        command_state: &NodeLocalState,
    ) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::MergeCommandState {
            baseline: Box::new(baseline.clone()),
            command_state: Box::new(command_state.clone()),
        })
        .await?;
        Ok(())
    }

    /// Persist a completed command's status, result payload, and generated
    /// event outbox entries as one owner-boundary mutation.
    pub(crate) async fn persist_command_outcome(
        &self,
        baseline: &NodeLocalState,
        command_state: &NodeLocalState,
    ) -> anyhow::Result<()> {
        self.merge_command_state(baseline, command_state).await
    }

    pub(crate) async fn upsert_task_runtime_mapping(
        &self,
        mapping: TaskRuntimeMapping,
    ) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::UpsertTaskRuntimeMapping { mapping })
            .await?;
        Ok(())
    }

    pub(crate) async fn remove_task_runtime_mapping(
        &self,
        task_run_id: uprava_protocol::TaskRunId,
    ) -> anyhow::Result<()> {
        self.mutate(NodeStateMutation::RemoveTaskRuntimeMapping { task_run_id })
            .await?;
        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        let (respond_to, response) = oneshot::channel();
        let _ = self
            .sender
            .send(NodeStateRequest::Shutdown { respond_to })
            .await;
        let _ = response.await;
        if let Some(actor) = self.actor.lock().await.take() {
            actor.await.context("node state store task join failed")?;
        }
        Ok(())
    }

    pub(crate) async fn mutate(
        &self,
        mutation: NodeStateMutation,
    ) -> anyhow::Result<NodeStateMutationResult> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(NodeStateRequest::Mutate {
                mutation: Box::new(mutation),
                respond_to,
            })
            .await
            .map_err(|_| anyhow::anyhow!("node state store task stopped"))?;
        response.await.context("node state store task stopped")?
    }
}

pub(crate) async fn run_node_state_store(
    mut state: NodeLocalState,
    path: PathBuf,
    mut receiver: mpsc::Receiver<NodeStateRequest>,
) {
    while let Some(request) = receiver.recv().await {
        match request {
            NodeStateRequest::Snapshot { respond_to } => {
                let _ = respond_to.send(Ok(state.clone()));
            }
            NodeStateRequest::IsEnrolled { respond_to } => {
                let _ = respond_to.send(Ok(state.is_enrolled()));
            }
            NodeStateRequest::Mutate {
                mutation,
                respond_to,
            } => {
                let _ =
                    respond_to.send(apply_node_state_mutation(&mut state, &path, *mutation).await);
            }
            NodeStateRequest::Shutdown { respond_to } => {
                let _ = respond_to.send(());
                break;
            }
        }
    }
}

pub(crate) async fn apply_node_state_mutation(
    state: &mut NodeLocalState,
    path: &Path,
    mutation: NodeStateMutation,
) -> anyhow::Result<NodeStateMutationResult> {
    match mutation {
        NodeStateMutation::PersistEnrollmentAttempt {
            enrollment_id,
            pairing_code,
        } => {
            state.enrollment_id = Some(enrollment_id);
            state.pairing_code = Some(pairing_code);
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::ClearEnrollmentAttempt => {
            state.clear_enrollment_attempt();
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::PersistEnrollmentIdentity {
            node_id,
            credential,
        } => {
            state.node_id = Some(node_id);
            state.credential = Some(credential);
            state.enrollment_id = None;
            state.pairing_code = None;
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::ClearCoreRegistration => {
            state.clear_core_registration();
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::PersistReconnectAttempt => {
            state.reconnect_attempts = state.reconnect_attempts.saturating_add(1);
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::PersistHeartbeatFailure => {
            state.heartbeat_failures = state.heartbeat_failures.saturating_add(1);
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::PersistEventAck { accepted_event_ids } => {
            let removed = remove_acked_events(&mut state.event_outbox, &accepted_event_ids);
            if removed > 0 {
                state.save_async(path).await?;
            }
            Ok(NodeStateMutationResult::RemovedEvents(removed))
        }
        NodeStateMutation::MergeCommandState {
            baseline,
            command_state,
        } => {
            state.merge_command_state_from(&baseline, &command_state);
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::UpsertTaskRuntimeMapping { mapping } => {
            state
                .task_runtime_mappings
                .insert(mapping.task_run_id.to_string(), mapping);
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
        NodeStateMutation::RemoveTaskRuntimeMapping { task_run_id } => {
            state.task_runtime_mappings.remove(task_run_id.as_str());
            state.save_async(path).await?;
            Ok(NodeStateMutationResult::Unit)
        }
    }
}

pub(crate) async fn open_state_store(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        set_private_dir_permissions(parent);
    }
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .with_context(|| format!("failed to open node state store {}", path.display()))
}

pub(crate) async fn initialize_state_store(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        create table if not exists node_state (
            state_id integer primary key check (state_id = 1),
            state_slot text not null,
            schema_version integer not null,
            snapshot_json text not null,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_command_cache (
            command_id text primary key,
            state text not null,
            result_payload_json text,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_event_outbox (
            event_id text primary key,
            event_json text not null,
            seq integer not null,
            created_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_registration (
            state_id integer primary key check (state_id = 1),
            daemon_installation_id text not null,
            node_id text,
            credential text,
            enrollment_id text,
            pairing_code text,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_metrics (
            state_id integer primary key check (state_id = 1),
            reconnect_attempts integer not null,
            dropped_event_count integer not null,
            heartbeat_failures integer not null,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_runtime_metadata (
            runtime_session_id text primary key,
            runtime_seq integer,
            provider text,
            workspace_path text,
            state_json text,
            transcript_json text,
            resume_ref_json text,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_placement_sequences (
            placement_id text primary key,
            seq integer not null,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists node_task_runtime_mappings (
            task_run_id text primary key,
            worktree_path text not null,
            branch text not null,
            sandbox_id text,
            updated_at text not null
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) fn command_state_storage(state: CommandState) -> &'static str {
    match state {
        CommandState::Recorded => "recorded",
        CommandState::PendingDispatch => "pending_dispatch",
        CommandState::Dispatched => "dispatched",
        CommandState::Acknowledged => "acknowledged",
        CommandState::Completed => "completed",
        CommandState::Failed => "failed",
        CommandState::Blocked => "blocked",
        CommandState::Expired => "expired",
    }
}

pub(crate) fn command_state_from_storage(value: &str) -> Option<CommandState> {
    Some(match value {
        "recorded" => CommandState::Recorded,
        "pending_dispatch" => CommandState::PendingDispatch,
        "dispatched" => CommandState::Dispatched,
        "acknowledged" => CommandState::Acknowledged,
        "completed" => CommandState::Completed,
        "failed" => CommandState::Failed,
        "blocked" => CommandState::Blocked,
        "expired" => CommandState::Expired,
        _ => return None,
    })
}

pub(crate) async fn hydrate_from_normalized_tables(
    pool: &SqlitePool,
    state: &mut NodeLocalState,
) -> anyhow::Result<()> {
    if let Some(row) = sqlx::query(
        "select daemon_installation_id, node_id, credential, enrollment_id, pairing_code from node_registration where state_id = 1",
    )
    .fetch_optional(pool)
    .await?
    {
        state.daemon_installation_id = row.try_get("daemon_installation_id")?;
        state.node_id = row
            .try_get::<Option<String>, _>("node_id")?
            .map(NodeId::from);
        state.credential = row.try_get("credential")?;
        state.enrollment_id = row
            .try_get::<Option<String>, _>("enrollment_id")?
            .map(EnrollmentId::from);
        state.pairing_code = row.try_get("pairing_code")?;
    }

    if let Some(row) = sqlx::query(
        "select reconnect_attempts, dropped_event_count, heartbeat_failures from node_metrics where state_id = 1",
    )
    .fetch_optional(pool)
    .await?
    {
        state.reconnect_attempts = row
            .try_get::<i64, _>("reconnect_attempts")?
            .try_into()
            .unwrap_or_default();
        state.dropped_event_count = row
            .try_get::<i64, _>("dropped_event_count")?
            .try_into()
            .unwrap_or_default();
        state.heartbeat_failures = row
            .try_get::<i64, _>("heartbeat_failures")?
            .try_into()
            .unwrap_or_default();
    }

    let command_rows =
        sqlx::query("select command_id, state, result_payload_json from node_command_cache")
            .fetch_all(pool)
            .await?;
    if !command_rows.is_empty() {
        state.command_status.clear();
        state.command_result_payloads.clear();
        for row in command_rows {
            let command_id: String = row.try_get("command_id")?;
            let stored_state: String = row.try_get("state")?;
            if let Some(command_state) = command_state_from_storage(&stored_state) {
                state
                    .command_status
                    .insert(command_id.clone(), command_state);
            }
            if let Some(payload) = row.try_get::<Option<String>, _>("result_payload_json")? {
                state
                    .command_result_payloads
                    .insert(command_id, serde_json::from_str(&payload)?);
            }
        }
    }

    let event_rows = sqlx::query("select event_json from node_event_outbox order by rowid")
        .fetch_all(pool)
        .await?;
    if !event_rows.is_empty() {
        state.event_outbox = event_rows
            .into_iter()
            .map(|row| {
                let event_json: String = row.try_get("event_json")?;
                serde_json::from_str(&event_json).map_err(anyhow::Error::from)
            })
            .collect::<anyhow::Result<Vec<EventEnvelope>>>()?;
    }

    let runtime_rows = sqlx::query(
        "select runtime_session_id, runtime_seq, provider, workspace_path, state_json, transcript_json, resume_ref_json from node_runtime_metadata",
    )
    .fetch_all(pool)
    .await?;
    if !runtime_rows.is_empty() {
        state.runtime_seqs.clear();
        state.runtime_providers.clear();
        state.runtime_workspace_paths.clear();
        state.runtime_states.clear();
        state.runtime_transcripts.clear();
        state.runtime_provider_resume_refs.clear();
        for row in runtime_rows {
            let runtime_id: String = row.try_get("runtime_session_id")?;
            if let Some(seq) = row.try_get::<Option<i64>, _>("runtime_seq")? {
                state.runtime_seqs.insert(runtime_id.clone(), seq);
            }
            if let Some(provider) = row.try_get::<Option<String>, _>("provider")? {
                state.runtime_providers.insert(runtime_id.clone(), provider);
            }
            if let Some(path) = row.try_get::<Option<String>, _>("workspace_path")? {
                state
                    .runtime_workspace_paths
                    .insert(runtime_id.clone(), path);
            }
            if let Some(value) = row.try_get::<Option<String>, _>("state_json")? {
                state
                    .runtime_states
                    .insert(runtime_id.clone(), serde_json::from_str(&value)?);
            }
            if let Some(value) = row.try_get::<Option<String>, _>("transcript_json")? {
                state
                    .runtime_transcripts
                    .insert(runtime_id.clone(), serde_json::from_str(&value)?);
            }
            if let Some(value) = row.try_get::<Option<String>, _>("resume_ref_json")? {
                state
                    .runtime_provider_resume_refs
                    .insert(runtime_id, serde_json::from_str(&value)?);
            }
        }
    }

    let placement_rows = sqlx::query("select placement_id, seq from node_placement_sequences")
        .fetch_all(pool)
        .await?;
    if !placement_rows.is_empty() {
        state.placement_seqs.clear();
        for row in placement_rows {
            state
                .placement_seqs
                .insert(row.try_get("placement_id")?, row.try_get("seq")?);
        }
    }
    let task_rows = sqlx::query(
        "select task_run_id, worktree_path, branch, sandbox_id, updated_at from node_task_runtime_mappings",
    )
    .fetch_all(pool)
    .await?;
    if !task_rows.is_empty() {
        state.task_runtime_mappings.clear();
        for row in task_rows {
            let task_run_id =
                uprava_protocol::TaskRunId::from(row.try_get::<String, _>("task_run_id")?);
            state.task_runtime_mappings.insert(
                task_run_id.to_string(),
                TaskRuntimeMapping {
                    task_run_id,
                    worktree_path: row.try_get("worktree_path")?,
                    branch: row.try_get("branch")?,
                    sandbox_id: row.try_get("sandbox_id")?,
                    updated_at: row.try_get("updated_at")?,
                },
            );
        }
    }
    Ok(())
}

pub(crate) fn write_private_file(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    set_private_dir_permissions(parent);
    let file_name = path
        .file_name()
        .context("private file path must include a file name")?
        .to_string_lossy();
    let temp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        sanitize_filename_segment(&Uuid::new_v4().to_string())
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&temp_path)?;
    file.write_all(content)?;
    file.flush()?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&temp_path, PermissionsExt::from_mode(0o600))?;
    }
    std::fs::rename(&temp_path, path)?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(path, PermissionsExt::from_mode(0o600))?;
    }
    sync_parent_directory(parent)?;
    Ok(())
}

pub(crate) fn sync_parent_directory(parent: &Path) -> anyhow::Result<()> {
    match fs::File::open(parent) {
        Ok(directory) => {
            directory.sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn set_private_dir_permissions(path: &Path) {
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(path, PermissionsExt::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
