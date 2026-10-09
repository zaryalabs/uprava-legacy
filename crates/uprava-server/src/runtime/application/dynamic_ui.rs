//! Core-owned Generated React proposal, build, state and action lifecycle.

use std::collections::HashSet;

use uprava_protocol::{
    ArtifactDetail, ContributionTarget, CreateDynamicUiProposalRequest, EffectiveContributionState,
    GeneratedUiActionBridgeContributionV1, GeneratedUiActionDefinition, GeneratedUiActionKind,
    GeneratedUiActionResult, GeneratedUiArtifactPayload, GeneratedUiBuild,
    GeneratedUiBuildDiagnostic, GeneratedUiBuildState, GeneratedUiCapability,
    GeneratedUiDiagnosticSeverity, GeneratedUiRuntimeContributionV1, GeneratedUiRuntimeDetail,
    GeneratedUiSdkContributionV1, GeneratedUiState, InvokeGeneratedUiActionRequest,
    PluginContribution, PluginManifest, UpdateGeneratedUiStateRequest,
};

use super::super::*;

const GENERATED_UI_ARTIFACT_TYPE: &str = "uprava.generated-react";
const MAX_DYNAMIC_UI_TITLE_CHARS: usize = 240;
const MAX_DYNAMIC_UI_DESCRIPTION_CHARS: usize = 2_000;
const MAX_DYNAMIC_UI_FALLBACK_CHARS: usize = 65_536;
const MAX_DYNAMIC_UI_SNAPSHOT_BYTES: usize = 256 * 1024;
const MAX_DYNAMIC_UI_STATE_BYTES: usize = 256 * 1024;
const MAX_DYNAMIC_UI_ACTIONS: usize = 32;
const MAX_DYNAMIC_UI_ACTION_ID_CHARS: usize = 128;
const MAX_DYNAMIC_UI_ACTION_LABEL_CHARS: usize = 240;
const MAX_DYNAMIC_UI_IDEMPOTENCY_KEY_CHARS: usize = 200;
const MAX_DYNAMIC_UI_INPUT_BYTES: usize = 64 * 1024;
const MAX_BUILDER_RESPONSE_OVERHEAD_BYTES: usize = 512 * 1024;
const GENERATED_REACT_PLUGIN_ID: &str = "uprava.generated-react";

#[derive(Debug, Serialize)]
struct BuilderRequest<'a> {
    source: &'a str,
    runtime_id: &'a str,
    runtime_version: &'a str,
    sdk_version: &'a str,
    allowed_imports: &'a [String],
    max_bundle_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct BuilderResponse {
    bundle: String,
    dependency_lock: serde_json::Value,
    #[serde(default)]
    diagnostics: Vec<GeneratedUiBuildDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct SendAgentInputAction {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenReferenceAction {
    reference: UpravaRef,
}

pub(crate) async fn create_dynamic_ui_proposal_route(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateDynamicUiProposalRequest>,
) -> Result<(StatusCode, Json<GeneratedUiRuntimeDetail>), AppError> {
    create_dynamic_ui_proposal(&state, request)
        .await
        .map(|detail| (StatusCode::CREATED, Json(detail)))
}

pub(crate) async fn inspect_generated_ui_capabilities(
    state: &AppState,
) -> Result<serde_json::Value, AppError> {
    let (effective_state, manifest_json): (String, String) = sqlx::query_as(
        r#"
        select i.effective_state, p.manifest_json
        from plugin_installations i
        join plugin_packages p
          on p.plugin_id = i.plugin_id and p.version = i.active_version
        where i.plugin_id = ?1
        "#,
    )
    .bind(GENERATED_REACT_PLUGIN_ID)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found(
            "generated_ui.plugin_not_found",
            "Generated React plugin is not installed",
        )
    })?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_json)?;
    let mut runtime = None;
    let mut sdk = None;
    let mut action_bridge = None;
    for contribution in manifest.contributions {
        match contribution {
            PluginContribution::GeneratedUiRuntime { contribution, .. } => {
                runtime = Some(contribution);
            }
            PluginContribution::GeneratedUiSdk { contribution, .. } => {
                sdk = Some(contribution);
            }
            PluginContribution::GeneratedUiActionBridge { contribution, .. } => {
                action_bridge = Some(contribution);
            }
            _ => {}
        }
    }
    Ok(json!({
        "plugin_id": GENERATED_REACT_PLUGIN_ID,
        "enabled": effective_state == "active",
        "runtime": runtime.ok_or_else(generated_ui_manifest_invalid)?,
        "sdk": sdk.ok_or_else(generated_ui_manifest_invalid)?,
        "action_bridge": action_bridge.ok_or_else(generated_ui_manifest_invalid)?,
        "proposal_contract": {
            "entrypoint": "default exported React component",
            "source_language": "TypeScript/TSX",
            "fallback_snapshot_formats": ["data:image/png;base64", "data:image/webp;base64"],
            "fallback_markdown_required": true,
            "scope": "forced to the current session for agent tool calls"
        }
    }))
}

pub(crate) async fn generated_ui_runtime_detail_route(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<String>,
) -> Result<Json<GeneratedUiRuntimeDetail>, AppError> {
    load_generated_ui_runtime_detail(&state, &ArtifactId::from(artifact_id))
        .await
        .map(Json)
}

pub(crate) async fn generated_ui_bundle_route(
    State(state): State<Arc<AppState>>,
    Path(blob_hash): Path<String>,
) -> Result<Response<Body>, AppError> {
    let bundle = load_generated_ui_bundle(&state, &blob_hash).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-content-type-options", "nosniff")
        .header(
            axum::http::header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .body(Body::from(bundle))
        .map_err(|error| {
            AppError::internal(format!("generated UI bundle response failed: {error}"))
        })
}

pub(crate) async fn generated_ui_source_route(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<String>,
) -> Result<Response<Body>, AppError> {
    let source = load_generated_ui_source(&state, &ArtifactId::from(artifact_id)).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-content-type-options", "nosniff")
        .header(axum::http::header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(source))
        .map_err(|error| {
            AppError::internal(format!("generated UI source response failed: {error}"))
        })
}

pub(crate) async fn update_generated_ui_state_route(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<String>,
    Json(request): Json<UpdateGeneratedUiStateRequest>,
) -> Result<Json<GeneratedUiState>, AppError> {
    update_generated_ui_state(&state, &ArtifactId::from(artifact_id), request)
        .await
        .map(Json)
}

pub(crate) async fn invoke_generated_ui_action_route(
    State(state): State<Arc<AppState>>,
    Path((artifact_id, action_id)): Path<(String, String)>,
    Json(request): Json<InvokeGeneratedUiActionRequest>,
) -> Result<Json<GeneratedUiActionResult>, AppError> {
    invoke_generated_ui_action(&state, &ArtifactId::from(artifact_id), &action_id, request)
        .await
        .map(Json)
}

pub(crate) async fn create_dynamic_ui_proposal(
    state: &AppState,
    request: CreateDynamicUiProposalRequest,
) -> Result<GeneratedUiRuntimeDetail, AppError> {
    create_dynamic_ui_proposal_as(state, request, ActorRef::local_user()).await
}

pub(crate) async fn create_dynamic_ui_proposal_as(
    state: &AppState,
    request: CreateDynamicUiProposalRequest,
    created_by: ActorRef,
) -> Result<GeneratedUiRuntimeDetail, AppError> {
    let (runtime, sdk, action_bridge) =
        resolve_generated_ui_runtime(state, &request.runtime_id).await?;
    validate_dynamic_ui_proposal(&request, &runtime, &sdk, &action_bridge)?;

    let source_hash = artifact_blob_hash(request.source.as_bytes());
    let payload = GeneratedUiArtifactPayload {
        description: request.description.clone(),
        runtime_id: runtime.runtime_id.clone(),
        sdk_version: request.sdk_version.clone(),
        layout_intent: request.layout_intent,
        source_blob_hash: source_hash.clone(),
        data_model: request.data_model.clone(),
        actions: request.actions.clone(),
        granted_capabilities: request.requested_capabilities.clone(),
        fallback_snapshot: request.fallback_snapshot.clone(),
    };
    let artifact_request = uprava_protocol::CreateArtifactRequest {
        artifact_type: GENERATED_UI_ARTIFACT_TYPE.to_owned(),
        title: request.title,
        scope_ref: request.scope_ref,
        schema_version: 1,
        payload: serde_json::to_value(&payload)?.into(),
        fallback_text: request.fallback_markdown,
        source_version: Some(format!(
            "{}@{}+{}",
            runtime.runtime_id, runtime.runtime_version, source_hash
        )),
        source_refs: request.source_refs,
        evidence_refs: request.evidence_refs,
        cause_refs: request.cause_refs,
        trace_refs: request.trace_refs,
        provenance: json!({
            "kind": "dynamic_ui.generated_react",
            "runtime_id": runtime.runtime_id,
            "runtime_version": runtime.runtime_version,
            "sdk_version": request.sdk_version,
            "source_blob_hash": source_hash,
        })
        .into(),
    };
    let prepared = prepare_artifact_creation_as(state, &artifact_request, created_by).await?;
    let artifact_id = prepared.artifact_id.clone();
    let build_id = format!("build-{}", Uuid::new_v4());
    let now = Utc::now();
    let mut transaction = state.pool.begin().await?;
    insert_artifact_blob(
        &mut transaction,
        &source_hash,
        "text/typescript-jsx",
        request.source.as_bytes(),
    )
    .await?;
    insert_prepared_artifact(&mut transaction, &prepared, &artifact_request).await?;
    sqlx::query(
        r#"
        insert into generated_ui_builds (
            build_id, artifact_id, artifact_version, state, runtime_id,
            runtime_version, sdk_version, source_blob_hash, bundle_blob_hash,
            dependency_lock_json, diagnostics_json, created_at, completed_at
        ) values (?1, ?2, 1, 'pending', ?3, ?4, ?5, ?6, null, '{}', '[]', ?7, null)
        "#,
    )
    .bind(&build_id)
    .bind(artifact_id.as_str())
    .bind(&payload.runtime_id)
    .bind(&runtime.runtime_version)
    .bind(&payload.sdk_version)
    .bind(&source_hash)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "insert into generated_ui_states (artifact_id, revision, values_json, updated_at) values (?1, 0, ?2, ?3)",
    )
    .bind(artifact_id.as_str())
    .bind(serde_json::to_string(&payload.data_model)?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let artifact = load_artifact_detail(state, &artifact_id, Some(1)).await?;

    record_dynamic_ui_event(
        state,
        &artifact,
        "dynamic_ui.proposed",
        json!({ "build_id": build_id, "runtime_id": payload.runtime_id }),
    )
    .await?;
    complete_generated_ui_build(
        state,
        &artifact,
        &build_id,
        &request.source,
        &runtime,
        &payload.sdk_version,
    )
    .await?;
    load_generated_ui_runtime_detail(state, &artifact_id).await
}

async fn complete_generated_ui_build(
    state: &AppState,
    artifact: &ArtifactDetail,
    build_id: &str,
    source: &str,
    runtime: &GeneratedUiRuntimeContributionV1,
    sdk_version: &str,
) -> Result<(), AppError> {
    let endpoint = format!(
        "{}/build",
        state.config.generated_ui_builder_url.trim_end_matches('/')
    );
    let result = reqwest::Client::new()
        .post(endpoint)
        .timeout(Duration::from_secs(
            u64::try_from(state.config.generated_ui_builder_timeout_seconds).unwrap_or(15),
        ))
        .json(&BuilderRequest {
            source,
            runtime_id: &runtime.runtime_id,
            runtime_version: &runtime.runtime_version,
            sdk_version,
            allowed_imports: &runtime.allowed_imports,
            max_bundle_bytes: runtime.max_bundle_bytes,
        })
        .send()
        .await;
    let response_limit = usize::try_from(runtime.max_bundle_bytes)
        .unwrap_or(4 * 1024 * 1024)
        .saturating_add(MAX_BUILDER_RESPONSE_OVERHEAD_BYTES);
    let built: Result<BuilderResponse, String> = match result {
        Ok(response) if response.status().is_success() => {
            read_bounded_builder_body(response, response_limit)
                .await
                .and_then(|body| {
                    serde_json::from_slice::<BuilderResponse>(&body)
                        .map_err(|error| error.to_string())
                })
        }
        Ok(response) => {
            let status = response.status();
            let message = read_bounded_builder_body(response, MAX_DYNAMIC_UI_INPUT_BYTES)
                .await
                .map(|body| String::from_utf8_lossy(&body).into_owned())
                .unwrap_or_else(|_| "builder returned no diagnostic body".to_owned());
            Err(format!(
                "builder returned {status}: {}",
                truncate_chars(&message, 1_500)
            ))
        }
        Err(error) => Err(error.to_string()),
    };
    match built {
        Ok(built) if built.bundle.len() <= runtime.max_bundle_bytes as usize => {
            let bundle_hash = store_artifact_blob(
                state,
                "text/javascript; charset=utf-8",
                built.bundle.as_bytes(),
            )
            .await?;
            sqlx::query(
                r#"
                update generated_ui_builds
                set state = 'ready', bundle_blob_hash = ?1,
                    dependency_lock_json = ?2, diagnostics_json = ?3,
                    completed_at = ?4
                where build_id = ?5 and state = 'pending'
                "#,
            )
            .bind(bundle_hash)
            .bind(serde_json::to_string(&built.dependency_lock)?)
            .bind(serde_json::to_string(&built.diagnostics)?)
            .bind(Utc::now())
            .bind(build_id)
            .execute(&state.pool)
            .await?;
            record_dynamic_ui_event(
                state,
                artifact,
                "artifact.build.completed",
                json!({ "build_id": build_id }),
            )
            .await?;
        }
        Ok(_) => {
            fail_generated_ui_build(
                state,
                artifact,
                build_id,
                "Builder output exceeds the runtime bundle limit",
            )
            .await?;
        }
        Err(error) => {
            fail_generated_ui_build(state, artifact, build_id, &error.to_string()).await?;
        }
    }
    Ok(())
}

async fn read_bounded_builder_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err("builder response exceeds the size limit".to_owned());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("builder response exceeds the size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fail_generated_ui_build(
    state: &AppState,
    artifact: &ArtifactDetail,
    build_id: &str,
    message: &str,
) -> Result<(), AppError> {
    let diagnostics = vec![GeneratedUiBuildDiagnostic {
        severity: GeneratedUiDiagnosticSeverity::Error,
        message: truncate_chars(message, 2_000),
        line: None,
        column: None,
    }];
    sqlx::query(
        r#"
        update generated_ui_builds
        set state = 'failed', diagnostics_json = ?1, completed_at = ?2
        where build_id = ?3 and state = 'pending'
        "#,
    )
    .bind(serde_json::to_string(&diagnostics)?)
    .bind(Utc::now())
    .bind(build_id)
    .execute(&state.pool)
    .await?;
    record_dynamic_ui_event(
        state,
        artifact,
        "artifact.build.failed",
        json!({ "build_id": build_id, "diagnostics": diagnostics }),
    )
    .await?;
    Ok(())
}

pub(crate) async fn load_generated_ui_runtime_detail(
    state: &AppState,
    artifact_id: &ArtifactId,
) -> Result<GeneratedUiRuntimeDetail, AppError> {
    let artifact = load_artifact_detail(state, artifact_id, None).await?;
    ensure_generated_ui_artifact(&artifact)?;
    let build_row = sqlx::query(
        r#"
        select build_id, artifact_id, artifact_version, state, runtime_id,
               runtime_version, sdk_version, source_blob_hash, bundle_blob_hash,
               dependency_lock_json, diagnostics_json, created_at, completed_at
        from generated_ui_builds
        where artifact_id = ?1 and artifact_version = ?2
        "#,
    )
    .bind(artifact_id.as_str())
    .bind(i64::try_from(artifact.version.version).map_err(|_| generated_ui_invalid())?)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found(
            "generated_ui.build_not_found",
            "Generated UI build not found",
        )
    })?;
    let state_row = sqlx::query(
        "select artifact_id, revision, values_json, updated_at from generated_ui_states where artifact_id = ?1",
    )
    .bind(artifact_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("generated_ui.state_not_found", "Generated UI state not found"))?;
    Ok(GeneratedUiRuntimeDetail {
        artifact,
        build: generated_ui_build_from_row(&build_row)?,
        state: generated_ui_state_from_row(&state_row)?,
    })
}

pub(crate) async fn update_generated_ui_state(
    state: &AppState,
    artifact_id: &ArtifactId,
    request: UpdateGeneratedUiStateRequest,
) -> Result<GeneratedUiState, AppError> {
    let detail = load_generated_ui_runtime_detail(state, artifact_id).await?;
    let payload = generated_ui_payload(&detail.artifact)?;
    resolve_generated_ui_runtime(state, &payload.runtime_id).await?;
    if !payload
        .granted_capabilities
        .contains(&GeneratedUiCapability::PersistState)
    {
        return Err(AppError::auth(
            "generated_ui.permission_denied",
            "Artifact was not granted persisted state access",
        ));
    }
    validate_bounded_json(&request.values, MAX_DYNAMIC_UI_STATE_BYTES, "state")?;
    let next_revision = request.expected_revision.checked_add(1).ok_or_else(|| {
        AppError::bad_request(
            "generated_ui.state_revision_invalid",
            "Generated UI state revision is exhausted",
        )
    })?;
    let updated_at = Utc::now();
    let result = sqlx::query(
        r#"
        update generated_ui_states
        set revision = ?1, values_json = ?2, updated_at = ?3
        where artifact_id = ?4 and revision = ?5
        "#,
    )
    .bind(i64::try_from(next_revision).map_err(|_| generated_ui_invalid())?)
    .bind(serde_json::to_string(&request.values)?)
    .bind(updated_at)
    .bind(artifact_id.as_str())
    .bind(i64::try_from(request.expected_revision).map_err(|_| generated_ui_invalid())?)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "generated_ui.state_conflict",
            "Generated UI state changed; reload and retry",
        ));
    }
    let updated = GeneratedUiState {
        artifact_id: artifact_id.clone(),
        revision: next_revision,
        values: request.values,
        updated_at,
    };
    record_dynamic_ui_event(
        state,
        &detail.artifact,
        "artifact.state.updated",
        json!({ "revision": updated.revision }),
    )
    .await?;
    Ok(updated)
}

pub(crate) async fn invoke_generated_ui_action(
    state: &AppState,
    artifact_id: &ArtifactId,
    action_id: &str,
    request: InvokeGeneratedUiActionRequest,
) -> Result<GeneratedUiActionResult, AppError> {
    validate_action_request(action_id, &request)?;
    if let Some(existing) =
        load_completed_action(state, artifact_id, &request.idempotency_key).await?
    {
        return Ok(existing);
    }
    let detail = load_generated_ui_runtime_detail(state, artifact_id).await?;
    if request.artifact_version != detail.artifact.version.version {
        return Err(AppError::conflict(
            "generated_ui.artifact_version_conflict",
            "Generated UI artifact changed; reload and retry",
        ));
    }
    let payload = generated_ui_payload(&detail.artifact)?;
    resolve_generated_ui_runtime(state, &payload.runtime_id).await?;
    let action = payload
        .actions
        .iter()
        .find(|candidate| candidate.action_id == action_id)
        .ok_or_else(|| {
            AppError::not_found(
                "generated_ui.action_not_found",
                "Generated UI action not found",
            )
        })?;
    authorize_action(action, &payload, request.confirmed)?;
    validate_action_input(&action.input_schema.0, &request.input.0)?;

    let action_request_id = format!("ui-action-{}", Uuid::new_v4());
    let created_at = Utc::now();
    let inserted = sqlx::query(
        r#"
        insert into generated_ui_action_requests (
            action_request_id, artifact_id, artifact_version, action_id,
            action_kind, input_json, idempotency_key, state, result_json,
            error_code, actor_ref_json, created_at, completed_at
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'requested', null, null, ?8, ?9, null)
        on conflict(artifact_id, idempotency_key) do nothing
        "#,
    )
    .bind(&action_request_id)
    .bind(artifact_id.as_str())
    .bind(i64::try_from(request.artifact_version).map_err(|_| generated_ui_invalid())?)
    .bind(action_id)
    .bind(format_action_kind(action.kind))
    .bind(serde_json::to_string(&request.input)?)
    .bind(&request.idempotency_key)
    .bind(serde_json::to_string(&ActorRef::local_user())?)
    .bind(created_at)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() != 1 {
        return load_completed_action(state, artifact_id, &request.idempotency_key)
            .await?
            .ok_or_else(|| {
                AppError::conflict(
                    "generated_ui.action_in_progress",
                    "Generated UI action with this idempotency key is already in progress",
                )
            });
    }

    let execution = execute_generated_ui_action(state, &detail, action, request.input).await;
    let (result_value, updated_state, command_id) = match execution {
        Ok(value) => value,
        Err(error) => {
            let error_code = error.code();
            sqlx::query(
                "update generated_ui_action_requests set state = 'failed', error_code = ?1, completed_at = ?2 where action_request_id = ?3",
            )
            .bind(error_code)
            .bind(Utc::now())
            .bind(&action_request_id)
            .execute(&state.pool)
            .await?;
            record_dynamic_ui_event(
                state,
                &detail.artifact,
                "artifact.action.failed",
                json!({
                    "action_request_id": action_request_id,
                    "action_id": action_id,
                    "action_kind": format_action_kind(action.kind),
                    "error_code": error_code,
                }),
            )
            .await?;
            return Err(error);
        }
    };
    let completed_at = Utc::now();
    let action_result = GeneratedUiActionResult {
        action_request_id: action_request_id.clone(),
        artifact_id: artifact_id.clone(),
        action_id: action_id.to_owned(),
        kind: action.kind,
        result: result_value,
        state: updated_state,
        command_id,
        completed_at,
    };
    sqlx::query(
        "update generated_ui_action_requests set state = 'completed', result_json = ?1, completed_at = ?2 where action_request_id = ?3",
    )
    .bind(serde_json::to_string(&action_result)?)
    .bind(completed_at)
    .bind(&action_request_id)
    .execute(&state.pool)
    .await?;
    record_dynamic_ui_event(
        state,
        &detail.artifact,
        "artifact.action.completed",
        json!({
            "action_request_id": action_request_id,
            "action_id": action_id,
            "action_kind": format_action_kind(action.kind),
        }),
    )
    .await?;
    Ok(action_result)
}

async fn execute_generated_ui_action(
    state: &AppState,
    detail: &GeneratedUiRuntimeDetail,
    action: &GeneratedUiActionDefinition,
    input: JsonValue,
) -> Result<(JsonValue, Option<GeneratedUiState>, Option<CommandId>), AppError> {
    match action.kind {
        GeneratedUiActionKind::UpdateArtifactState => {
            let request: UpdateGeneratedUiStateRequest =
                serde_json::from_value(input.0).map_err(|_| {
                    AppError::bad_request(
                        "generated_ui.action_input_invalid",
                        "State action input is invalid",
                    )
                })?;
            let updated =
                update_generated_ui_state(state, &detail.artifact.artifact.artifact_id, request)
                    .await?;
            Ok((
                json!({ "revision": updated.revision }).into(),
                Some(updated),
                None,
            ))
        }
        GeneratedUiActionKind::SendAgentInput => {
            let request: SendAgentInputAction = serde_json::from_value(input.0).map_err(|_| {
                AppError::bad_request(
                    "generated_ui.action_input_invalid",
                    "Agent input action is invalid",
                )
            })?;
            let ScopeRef::Session { session_thread_id } = &detail.artifact.artifact.scope_ref
            else {
                return Err(AppError::bad_request(
                    "generated_ui.action_scope_invalid",
                    "Agent input requires a session-scoped artifact",
                ));
            };
            let accepted = send_turn_with_correlation(
                state,
                session_thread_id.clone(),
                SendTurnRequest {
                    content: request.content,
                    collaboration_mode: None,
                },
                CorrelationId::new(),
            )
            .await?;
            Ok((
                serde_json::to_value(&accepted)?.into(),
                None,
                Some(accepted.command_id),
            ))
        }
        GeneratedUiActionKind::OpenReference => {
            let request: OpenReferenceAction = serde_json::from_value(input.0).map_err(|_| {
                AppError::bad_request(
                    "generated_ui.action_input_invalid",
                    "Reference action is invalid",
                )
            })?;
            let version = &detail.artifact.version;
            if !version
                .source_refs
                .iter()
                .chain(&version.evidence_refs)
                .chain(&version.cause_refs)
                .chain(&version.trace_refs)
                .any(|reference| reference == &request.reference)
            {
                return Err(AppError::auth(
                    "generated_ui.reference_denied",
                    "Generated UI may only open a declared artifact reference",
                ));
            }
            Ok((json!({ "reference": request.reference }).into(), None, None))
        }
    }
}

async fn load_completed_action(
    state: &AppState,
    artifact_id: &ArtifactId,
    idempotency_key: &str,
) -> Result<Option<GeneratedUiActionResult>, AppError> {
    let stored: Option<(String, Option<String>)> = sqlx::query_as(
        "select state, result_json from generated_ui_action_requests where artifact_id = ?1 and idempotency_key = ?2",
    )
    .bind(artifact_id.as_str())
    .bind(idempotency_key)
    .fetch_optional(&state.pool)
    .await?;
    match stored {
        Some((state, Some(result))) if state == "completed" => {
            Ok(Some(serde_json::from_str(&result)?))
        }
        _ => Ok(None),
    }
}

async fn resolve_generated_ui_runtime(
    state: &AppState,
    runtime_id: &str,
) -> Result<
    (
        GeneratedUiRuntimeContributionV1,
        GeneratedUiSdkContributionV1,
        GeneratedUiActionBridgeContributionV1,
    ),
    AppError,
> {
    let snapshot = effective_plugin_snapshot(state).await?;
    let runtime = snapshot
        .resolutions
        .iter()
        .find(|resolution| {
            matches!(
                &resolution.target,
                ContributionTarget::GeneratedUiRuntime { runtime_id: candidate }
                    if candidate == runtime_id
            )
        })
        .and_then(|resolution| {
            resolution.contributions.iter().find_map(|effective| {
                if effective.effective_state != EffectiveContributionState::Available {
                    return None;
                }
                let PluginContribution::GeneratedUiRuntime { contribution, .. } =
                    &effective.contribution
                else {
                    return None;
                };
                Some(contribution.clone())
            })
        })
        .ok_or_else(|| {
            AppError::bad_request(
                "generated_ui.runtime_unavailable",
                "Generated UI runtime is disabled, incompatible or unavailable",
            )
        })?;
    let sdk = snapshot
        .resolutions
        .iter()
        .find(|resolution| {
            matches!(
                &resolution.target,
                ContributionTarget::GeneratedUiSdk { sdk_id }
                    if sdk_id == &runtime.sdk_id
            )
        })
        .and_then(|resolution| {
            resolution.contributions.iter().find_map(|effective| {
                if effective.effective_state != EffectiveContributionState::Available {
                    return None;
                }
                let PluginContribution::GeneratedUiSdk { contribution, .. } =
                    &effective.contribution
                else {
                    return None;
                };
                Some(contribution.clone())
            })
        })
        .ok_or_else(|| {
            AppError::bad_request(
                "generated_ui.sdk_unavailable",
                "Generated UI SDK is disabled, incompatible or unavailable",
            )
        })?;
    let action_bridge = snapshot
        .resolutions
        .iter()
        .find(|resolution| {
            matches!(
                &resolution.target,
                ContributionTarget::GeneratedUiActionBridge { bridge_id }
                    if bridge_id == &runtime.action_bridge_id
            )
        })
        .and_then(|resolution| {
            resolution.contributions.iter().find_map(|effective| {
                if effective.effective_state != EffectiveContributionState::Available {
                    return None;
                }
                let PluginContribution::GeneratedUiActionBridge { contribution, .. } =
                    &effective.contribution
                else {
                    return None;
                };
                Some(contribution.clone())
            })
        })
        .ok_or_else(|| {
            AppError::bad_request(
                "generated_ui.action_bridge_unavailable",
                "Generated UI action bridge is disabled, incompatible or unavailable",
            )
        })?;
    Ok((runtime, sdk, action_bridge))
}

fn validate_dynamic_ui_proposal(
    request: &CreateDynamicUiProposalRequest,
    runtime: &GeneratedUiRuntimeContributionV1,
    sdk: &GeneratedUiSdkContributionV1,
    action_bridge: &GeneratedUiActionBridgeContributionV1,
) -> Result<(), AppError> {
    if request.title.trim().is_empty()
        || request.title.chars().count() > MAX_DYNAMIC_UI_TITLE_CHARS
        || request
            .description
            .as_ref()
            .is_some_and(|value| value.chars().count() > MAX_DYNAMIC_UI_DESCRIPTION_CHARS)
        || request.fallback_markdown.trim().is_empty()
        || request.fallback_markdown.chars().count() > MAX_DYNAMIC_UI_FALLBACK_CHARS
        || request
            .fallback_snapshot
            .as_deref()
            .is_some_and(|snapshot| !valid_fallback_snapshot(snapshot))
        || request.source.trim().is_empty()
        || request.source.len() > runtime.max_source_bytes as usize
    {
        return Err(AppError::bad_request(
            "generated_ui.proposal_invalid",
            "Generated UI title, source or fallback is empty or oversized",
        ));
    }
    if request.sdk_version != sdk.api_version
        || !runtime
            .supported_sdk_versions
            .contains(&request.sdk_version)
        || !runtime.supported_layouts.contains(&request.layout_intent)
    {
        return Err(AppError::bad_request(
            "generated_ui.compatibility_failed",
            "Generated UI SDK version or layout is not supported by the runtime",
        ));
    }
    if request.actions.len() > MAX_DYNAMIC_UI_ACTIONS
        || has_duplicate_values(&request.requested_capabilities)
        || request
            .requested_capabilities
            .iter()
            .any(|capability| !runtime.sandbox_capabilities.contains(capability))
    {
        return Err(AppError::bad_request(
            "generated_ui.capability_denied",
            "Generated UI requests unsupported or duplicated capabilities",
        ));
    }
    validate_bounded_json(
        &request.data_model,
        MAX_DYNAMIC_UI_STATE_BYTES,
        "data model",
    )?;
    let mut action_ids = HashSet::new();
    for action in &request.actions {
        if !valid_action_id(&action.action_id)
            || !action_ids.insert(&action.action_id)
            || action.label.trim().is_empty()
            || action.label.chars().count() > MAX_DYNAMIC_UI_ACTION_LABEL_CHARS
            || has_duplicate_values(&action.required_capabilities)
            || action
                .required_capabilities
                .iter()
                .any(|capability| !request.requested_capabilities.contains(capability))
            || !action
                .required_capabilities
                .contains(&capability_for_action(action.kind))
            || !action_bridge.supported_actions.contains(&action.kind)
            || (action.kind == GeneratedUiActionKind::SendAgentInput
                && !action.confirmation_required)
        {
            return Err(AppError::bad_request(
                "generated_ui.action_invalid",
                "Generated UI action identifier, label or capabilities are invalid",
            ));
        }
        validate_bounded_json(
            &action.input_schema,
            MAX_DYNAMIC_UI_INPUT_BYTES,
            "action schema",
        )?;
        validate_action_schema(&action.input_schema.0)?;
    }
    for forbidden in ["import(", "eval(", "new Function", "WebSocket(", "Worker("] {
        if request.source.contains(forbidden) {
            return Err(AppError::bad_request(
                "generated_ui.source_unsafe",
                "Generated UI source uses an unsupported dynamic capability",
            ));
        }
    }
    Ok(())
}

fn valid_fallback_snapshot(snapshot: &str) -> bool {
    let encoded = snapshot
        .strip_prefix("data:image/png;base64,")
        .or_else(|| snapshot.strip_prefix("data:image/webp;base64,"));
    let Some(encoded) = encoded else {
        return false;
    };
    !encoded.is_empty()
        && encoded.len() <= MAX_DYNAMIC_UI_SNAPSHOT_BYTES.saturating_mul(4).div_ceil(3) + 4
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

fn authorize_action(
    action: &GeneratedUiActionDefinition,
    payload: &GeneratedUiArtifactPayload,
    confirmed: bool,
) -> Result<(), AppError> {
    if (action.confirmation_required || action.kind == GeneratedUiActionKind::SendAgentInput)
        && !confirmed
    {
        return Err(AppError::bad_request(
            "generated_ui.confirmation_required",
            "Generated UI action requires explicit confirmation",
        ));
    }
    if action
        .required_capabilities
        .iter()
        .any(|capability| !payload.granted_capabilities.contains(capability))
    {
        return Err(AppError::auth(
            "generated_ui.permission_denied",
            "Generated UI action was not granted its required capabilities",
        ));
    }
    Ok(())
}

fn validate_action_schema(schema: &serde_json::Value) -> Result<(), AppError> {
    let Some(object) = schema.as_object() else {
        return Err(generated_ui_action_invalid());
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("object")
        || object.get("required").is_some_and(|value| {
            value.as_array().is_none_or(|items| {
                items
                    .iter()
                    .any(|item| item.as_str().is_none_or(str::is_empty))
            })
        })
        || object
            .get("properties")
            .is_some_and(|value| !value.is_object())
        || object
            .get("additionalProperties")
            .is_some_and(|value| !value.is_boolean())
    {
        return Err(generated_ui_action_invalid());
    }
    Ok(())
}

fn validate_action_input(
    schema: &serde_json::Value,
    input: &serde_json::Value,
) -> Result<(), AppError> {
    let schema = schema.as_object().ok_or_else(generated_ui_action_invalid)?;
    let input = input.as_object().ok_or_else(|| {
        AppError::bad_request(
            "generated_ui.action_input_invalid",
            "Generated UI action input must be an object",
        )
    })?;
    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        for name in required.iter().filter_map(serde_json::Value::as_str) {
            if !input.contains_key(name) {
                return Err(AppError::bad_request(
                    "generated_ui.action_input_invalid",
                    format!("Generated UI action input is missing `{name}`"),
                ));
            }
        }
    }
    let properties = schema
        .get("properties")
        .and_then(serde_json::Value::as_object);
    if schema
        .get("additionalProperties")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
        && input
            .keys()
            .any(|key| properties.is_none_or(|items| !items.contains_key(key)))
    {
        return Err(AppError::bad_request(
            "generated_ui.action_input_invalid",
            "Generated UI action input contains an unknown property",
        ));
    }
    if let Some(properties) = properties {
        for (name, value) in input {
            let Some(expected) = properties
                .get(name)
                .and_then(|property| property.get("type"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if !json_value_matches_type(expected, value) {
                return Err(AppError::bad_request(
                    "generated_ui.action_input_invalid",
                    format!("Generated UI action input `{name}` has the wrong type"),
                ));
            }
        }
    }
    Ok(())
}

fn json_value_matches_type(expected: &str, value: &serde_json::Value) -> bool {
    match expected {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn generated_ui_action_invalid() -> AppError {
    AppError::bad_request(
        "generated_ui.action_invalid",
        "Generated UI action identifier, capabilities or input schema are invalid",
    )
}

fn validate_action_request(
    action_id: &str,
    request: &InvokeGeneratedUiActionRequest,
) -> Result<(), AppError> {
    if !valid_action_id(action_id)
        || request.idempotency_key.is_empty()
        || request.idempotency_key.len() > MAX_DYNAMIC_UI_IDEMPOTENCY_KEY_CHARS
        || request
            .idempotency_key
            .bytes()
            .any(|byte| byte.is_ascii_control())
    {
        return Err(AppError::bad_request(
            "generated_ui.action_request_invalid",
            "Generated UI action request identifier is invalid",
        ));
    }
    validate_bounded_json(&request.input, MAX_DYNAMIC_UI_INPUT_BYTES, "action input")
}

fn generated_ui_payload(artifact: &ArtifactDetail) -> Result<GeneratedUiArtifactPayload, AppError> {
    ensure_generated_ui_artifact(artifact)?;
    serde_json::from_value(artifact.version.payload.0.clone()).map_err(|_| {
        AppError::bad_request(
            "generated_ui.payload_invalid",
            "Generated UI artifact payload is invalid",
        )
    })
}

fn ensure_generated_ui_artifact(artifact: &ArtifactDetail) -> Result<(), AppError> {
    if artifact.artifact.artifact_type == GENERATED_UI_ARTIFACT_TYPE {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "generated_ui.artifact_type_invalid",
            "Artifact is not a Generated React artifact",
        ))
    }
}

async fn store_artifact_blob(
    state: &AppState,
    media_type: &str,
    content: &[u8],
) -> Result<String, AppError> {
    let blob_hash = artifact_blob_hash(content);
    let mut transaction = state.pool.begin().await?;
    insert_artifact_blob(&mut transaction, &blob_hash, media_type, content).await?;
    transaction.commit().await?;
    Ok(blob_hash)
}

fn artifact_blob_hash(content: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(content))
}

async fn insert_artifact_blob(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    blob_hash: &str,
    media_type: &str,
    content: &[u8],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into artifact_blobs (blob_hash, media_type, content, size_bytes, created_at)
        values (?1, ?2, ?3, ?4, ?5)
        on conflict(blob_hash) do nothing
        "#,
    )
    .bind(blob_hash)
    .bind(media_type)
    .bind(content)
    .bind(i64::try_from(content.len()).map_err(|_| generated_ui_invalid())?)
    .bind(Utc::now())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_generated_ui_bundle(state: &AppState, blob_hash: &str) -> Result<Vec<u8>, AppError> {
    if !valid_blob_hash(blob_hash) {
        return Err(AppError::bad_request(
            "generated_ui.blob_hash_invalid",
            "Generated UI blob hash is invalid",
        ));
    }
    sqlx::query_scalar(
        r#"
        select b.content
        from artifact_blobs b
        join generated_ui_builds g on g.bundle_blob_hash = b.blob_hash
        where b.blob_hash = ?1 and g.state = 'ready'
        limit 1
        "#,
    )
    .bind(blob_hash)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found(
            "generated_ui.bundle_not_found",
            "Generated UI bundle not found",
        )
    })
}

async fn load_generated_ui_source(
    state: &AppState,
    artifact_id: &ArtifactId,
) -> Result<Vec<u8>, AppError> {
    let detail = load_generated_ui_runtime_detail(state, artifact_id).await?;
    sqlx::query_scalar(
        "select content from artifact_blobs where blob_hash = ?1 and media_type = 'text/typescript-jsx'",
    )
    .bind(&detail.build.source_blob_hash)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found(
            "generated_ui.source_not_found",
            "Generated UI source not found",
        )
    })
}

fn generated_ui_build_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GeneratedUiBuild, AppError> {
    Ok(GeneratedUiBuild {
        build_id: row.try_get("build_id")?,
        artifact_id: ArtifactId::from(row.try_get::<String, _>("artifact_id")?),
        artifact_version: u64::try_from(row.try_get::<i64, _>("artifact_version")?)
            .map_err(|_| generated_ui_invalid())?,
        state: parse_build_state(row.try_get::<String, _>("state")?.as_str())?,
        runtime_id: row.try_get("runtime_id")?,
        runtime_version: row.try_get("runtime_version")?,
        sdk_version: row.try_get("sdk_version")?,
        source_blob_hash: row.try_get("source_blob_hash")?,
        bundle_blob_hash: row.try_get("bundle_blob_hash")?,
        dependency_lock: serde_json::from_str::<serde_json::Value>(
            row.try_get::<String, _>("dependency_lock_json")?.as_str(),
        )?
        .into(),
        diagnostics: serde_json::from_str(row.try_get("diagnostics_json")?)?,
        created_at: row.try_get("created_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

fn generated_ui_state_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<GeneratedUiState, AppError> {
    Ok(GeneratedUiState {
        artifact_id: ArtifactId::from(row.try_get::<String, _>("artifact_id")?),
        revision: u64::try_from(row.try_get::<i64, _>("revision")?)
            .map_err(|_| generated_ui_invalid())?,
        values: serde_json::from_str::<serde_json::Value>(
            row.try_get::<String, _>("values_json")?.as_str(),
        )?
        .into(),
        updated_at: row.try_get("updated_at")?,
    })
}

async fn record_dynamic_ui_event(
    state: &AppState,
    artifact: &ArtifactDetail,
    name: &str,
    value: serde_json::Value,
) -> Result<(), AppError> {
    let ScopeRef::Session { session_thread_id } = &artifact.artifact.scope_ref else {
        return Ok(());
    };
    append_core_session_event(
        state,
        session_thread_id,
        None,
        EventKind::Extension,
        json!({ "name": name, "value": value }),
        vec![UpravaRef::ArtifactVersion {
            artifact_id: artifact.artifact.artifact_id.clone(),
            version: artifact.version.version,
        }],
        artifact.version.source_refs.clone(),
        artifact.version.evidence_refs.clone(),
    )
    .await?;
    Ok(())
}

fn validate_bounded_json(value: &JsonValue, limit: usize, label: &str) -> Result<(), AppError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() <= limit {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "generated_ui.payload_too_large",
            format!("Generated UI {label} exceeds the size limit"),
        ))
    }
}

fn has_duplicate_values<T: Eq + std::hash::Hash>(values: &[T]) -> bool {
    let mut seen = HashSet::with_capacity(values.len());
    values.iter().any(|value| !seen.insert(value))
}

fn valid_action_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DYNAMIC_UI_ACTION_ID_CHARS
        && value.contains('.')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn valid_blob_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn capability_for_action(action: GeneratedUiActionKind) -> GeneratedUiCapability {
    match action {
        GeneratedUiActionKind::UpdateArtifactState => GeneratedUiCapability::PersistState,
        GeneratedUiActionKind::SendAgentInput => GeneratedUiCapability::SendAgentInput,
        GeneratedUiActionKind::OpenReference => GeneratedUiCapability::OpenReference,
    }
}

fn format_action_kind(kind: GeneratedUiActionKind) -> &'static str {
    match kind {
        GeneratedUiActionKind::UpdateArtifactState => "update_artifact_state",
        GeneratedUiActionKind::SendAgentInput => "send_agent_input",
        GeneratedUiActionKind::OpenReference => "open_reference",
    }
}

fn parse_build_state(value: &str) -> Result<GeneratedUiBuildState, AppError> {
    match value {
        "pending" => Ok(GeneratedUiBuildState::Pending),
        "ready" => Ok(GeneratedUiBuildState::Ready),
        "failed" => Ok(GeneratedUiBuildState::Failed),
        "fallback_only" => Ok(GeneratedUiBuildState::FallbackOnly),
        _ => Err(generated_ui_invalid()),
    }
}

fn generated_ui_invalid() -> AppError {
    AppError::internal("generated UI persisted state is invalid")
}

fn generated_ui_manifest_invalid() -> AppError {
    AppError::internal("Generated React plugin manifest is incomplete")
}
