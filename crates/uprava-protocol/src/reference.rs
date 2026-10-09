use super::*;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            #[must_use]
            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

id_type!(NodeId);
id_type!(EnrollmentId);
id_type!(ProjectId);
id_type!(ProjectPlacementId);
id_type!(ActorId);
id_type!(SessionThreadId);
id_type!(RuntimeSessionId);
id_type!(RuntimeAttemptId);
id_type!(TurnId);
id_type!(MessageId);
id_type!(CommandId);
id_type!(TerminalId);
id_type!(EventId);
id_type!(ApprovalId);
id_type!(ProviderInteractionId);
id_type!(ArtifactId);
id_type!(EvidenceId);
id_type!(BlockId);
id_type!(CorrelationId);
id_type!(DeductionId);
id_type!(JobId);
id_type!(JobRunId);
id_type!(TaskRunId);
id_type!(ToolId);
id_type!(ToolSourceId);
id_type!(IntegrationId);
id_type!(McpDependencyInstanceId);
id_type!(ToolCallId);
id_type!(McpAccessLeaseId);
id_type!(PluginId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfile {
    ControlledDev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMode {
    Hardened,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActorRef {
    LocalUser { actor_id: Option<ActorId> },
    System,
    Node { node_id: NodeId },
    Provider { provider: String },
    Unknown,
}

impl ActorRef {
    #[must_use]
    pub fn local_user() -> Self {
        Self::LocalUser { actor_id: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeRef {
    Runtime {
        runtime_session_id: RuntimeSessionId,
    },
    RuntimeAttempt {
        runtime_attempt_id: RuntimeAttemptId,
    },
    ProviderInteraction {
        provider_interaction_id: ProviderInteractionId,
    },
    Session {
        session_thread_id: SessionThreadId,
    },
    Node {
        node_id: NodeId,
    },
    Placement {
        project_placement_id: ProjectPlacementId,
    },
    TaskRun {
        task_run_id: TaskRunId,
    },
    Unknown {
        scope: String,
    },
}
