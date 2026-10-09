//! Axum router, middleware, authentication and public operational endpoints.

use super::super::*;

pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = cors_layer(&state.config);
    let mcp_factory_state = state.clone();
    let mcp_service: rmcp::transport::streamable_http_server::StreamableHttpService<
        UpravaMcpServer,
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
    > = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        move || Ok(UpravaMcpServer::new(mcp_factory_state.clone())),
        Default::default(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .with_json_response(true),
    );
    let mcp = Router::new()
        .nest_service(UPRAVA_MCP_PATH, mcp_service)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_mcp_lease,
        ));
    let public_api = Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/metrics", get(metrics))
        .route("/auth/status", get(auth_status))
        .route("/auth/setup", post(auth_setup))
        .route("/auth/login", post(auth_login))
        .route("/auth/logout", post(auth_logout))
        .route("/node/enrollment-requests", post(node_enrollment_request))
        .route("/node/enrollment-claims", post(node_enrollment_claim))
        .route("/node/heartbeat", post(node_heartbeat_route))
        .route("/node/provider-mcp-access", post(node_provider_mcp_access))
        .route("/node/control", get(node_control));

    let client_api = Router::new()
        .route("/client/logs", post(client_logs))
        .route("/inventory", get(inventory))
        .route("/nodes", get(nodes))
        .route("/nodes/{node_id}", get(node_detail).delete(delete_node))
        .route("/nodes/{node_id}/revoke", post(revoke_node))
        .route(
            "/nodes/{node_id}/rotate-credential",
            post(rotate_node_credential),
        )
        .route(
            "/node-enrollments",
            get(node_enrollments).post(create_client_node_enrollment),
        )
        .route(
            "/node-enrollments/{enrollment_id}/approve",
            post(approve_node_enrollment),
        )
        .route(
            "/project-placements/validate",
            post(validate_placement_route),
        )
        .route(
            "/placements/{placement_id}",
            get(placement_detail).delete(delete_placement),
        )
        .route(
            "/placements/{placement_id}/resource-snapshot/refresh",
            post(refresh_resource_snapshot_route),
        )
        .route(
            "/placements/{placement_id}/workspace/tree",
            get(workspace_tree_route),
        )
        .route(
            "/placements/{placement_id}/workspace/file",
            get(workspace_file_route).post(workspace_file_write_route),
        )
        .route(
            "/placements/{placement_id}/workspace/commands",
            get(workspace_command_history_route).post(workspace_command_run_route),
        )
        .route(
            "/placements/{placement_id}/workspace/commands/async",
            post(workspace_command_accept_route),
        )
        .route(
            "/placements/{placement_id}/workspace/commands/async/{command_id}",
            get(workspace_command_resource_route).delete(workspace_command_cancel_route),
        )
        .route(
            "/placements/{placement_id}/workspace/diff",
            get(workspace_diff_route),
        )
        .route(
            "/placements/{placement_id}/workspace/review",
            get(workspace_review_route),
        )
        .route(
            "/placements/{placement_id}/workspace/terminals",
            get(workspace_terminal_list_route).post(workspace_terminal_open_route),
        )
        .route(
            "/placements/{placement_id}/workspace/terminals/{terminal_id}/stream",
            get(workspace_terminal_stream_route),
        )
        .route("/jobs", get(list_jobs_route).post(create_job_route))
        .route(
            "/jobs/{job_id}",
            get(job_detail_route).patch(update_job_route),
        )
        .route("/jobs/{job_id}/enable", post(enable_job_route))
        .route("/jobs/{job_id}/disable", post(disable_job_route))
        .route("/jobs/{job_id}/runs", post(run_job_route))
        .route("/job-runs/{job_run_id}", get(job_run_detail_route))
        .route("/job-runs/{job_run_id}/cancel", post(cancel_job_run_route))
        .route(
            "/task-runs",
            get(list_task_runs_route).post(create_task_run_route),
        )
        .route("/task-runs/{task_run_id}", get(task_run_detail_route))
        .route(
            "/task-runs/{task_run_id}/cancel",
            post(cancel_task_run_route),
        )
        .route("/provider-quota/{provider}", get(provider_quota_route))
        .route("/sessions", post(create_session_route))
        .route("/sessions/{session_thread_id}", get(session_detail))
        .route("/sessions/{session_thread_id}/attach", post(attach_session))
        .route("/sessions/{session_thread_id}/detach", post(detach_session))
        .route(
            "/sessions/{session_thread_id}/messages",
            get(session_messages),
        )
        .route("/sessions/{session_thread_id}/events", get(session_events))
        .route("/events", get(event_log_route))
        .route("/events/{event_id}", get(event_detail_route))
        .route("/tool-definitions", get(tool_definitions_route))
        .route("/tool-definitions/{tool_id}", get(tool_definition_route))
        .route("/tool-availability", get(tool_availability_route))
        .route(
            "/nodes/{node_id}/observed-capabilities",
            get(observed_capabilities_route),
        )
        .route(
            "/integrations",
            get(integration_connections_route).post(connect_integration_route),
        )
        .route(
            "/integrations/{integration_id}/disconnect",
            post(disconnect_integration_route),
        )
        .route("/mcp-dependencies", get(mcp_dependency_statuses_route))
        .route("/tool-calls", get(tool_calls_route))
        .route("/tool-calls/{tool_call_id}", get(tool_call_detail_route))
        .route("/plugins", get(plugin_list_route))
        .route("/plugins/{plugin_id}", get(plugin_detail_route))
        .route("/plugins/{plugin_id}/enable", post(enable_plugin_route))
        .route("/plugins/{plugin_id}/disable", post(disable_plugin_route))
        .route("/plugin-contributions", get(plugin_contributions_route))
        .route(
            "/artifacts",
            get(artifact_list_route).post(create_artifact_route),
        )
        .route("/artifacts/{artifact_id}", get(artifact_detail_route))
        .route(
            "/artifacts/{artifact_id}/versions",
            post(create_artifact_version_route),
        )
        .route(
            "/dynamic-ui/proposals",
            post(create_dynamic_ui_proposal_route),
        )
        .route(
            "/artifacts/{artifact_id}/dynamic-ui",
            get(generated_ui_runtime_detail_route),
        )
        .route(
            "/artifacts/{artifact_id}/dynamic-ui/source",
            get(generated_ui_source_route),
        )
        .route(
            "/artifacts/{artifact_id}/dynamic-ui/state",
            put(update_generated_ui_state_route),
        )
        .route(
            "/artifacts/{artifact_id}/dynamic-ui/actions/{action_id}",
            post(invoke_generated_ui_action_route),
        )
        .route(
            "/generated-ui/bundles/{blob_hash}",
            get(generated_ui_bundle_route),
        )
        .route(
            "/plugin-contribution-targets/{target_id}",
            put(update_plugin_contribution_preferences_route),
        )
        .route("/references/resolve", post(resolve_reference_route))
        .route("/sessions/policy-preview", post(preview_session_policy_route))
        .route("/sessions/{session_thread_id}/stream", get(session_stream))
        .route(
            "/sessions/{session_thread_id}/evidence-projection",
            get(session_evidence_projection),
        )
        .route(
            "/sessions/{session_thread_id}/trace",
            get(session_trace_projection),
        )
        .route(
            "/sessions/{session_thread_id}/agent-projection",
            get(session_agent_projection),
        )
        .route("/sessions/{session_thread_id}/turns", post(send_turn_route))
        .route(
            "/sessions/{session_thread_id}/deductions",
            post(create_deduction_route),
        )
        .route("/deductions/{deduction_id}", get(deduction_detail_route))
        .route(
            "/deductions/{deduction_id}/cancel",
            post(cancel_deduction_route),
        )
        .route(
            "/deductions/{deduction_id}/persist",
            post(persist_deduction_route),
        )
        .route(
            "/sessions/{session_thread_id}/scheduled-messages",
            post(create_scheduled_message_route),
        )
        .route(
            "/sessions/{session_thread_id}/scheduled-messages/{scheduled_message_id}",
            patch(update_scheduled_message_route).delete(cancel_scheduled_message_route),
        )
        .route(
            "/sessions/{session_thread_id}/scheduled-messages/{scheduled_message_id}/send-now",
            post(send_scheduled_message_now_route),
        )
        .route(
            "/sessions/{session_thread_id}/scheduled-messages/{scheduled_message_id}/retry",
            post(retry_scheduled_message_route),
        )
        .route(
            "/sessions/{session_thread_id}/approvals/{approval_id}/resolve",
            post(resolve_approval_route),
        )
        .route(
            "/sessions/{session_thread_id}/provider-interactions/{provider_interaction_id}/input",
            post(submit_provider_input_route),
        )
        .route(
            "/sessions/{session_thread_id}/provider-interactions/{provider_interaction_id}/approval",
            post(resolve_provider_approval_route),
        )
        .route(
            "/sessions/{session_thread_id}/warnings/{warning_kind}/acknowledge",
            post(acknowledge_warning_route),
        )
        .route(
            "/runtime-sessions/{runtime_session_id}/interrupt",
            post(interrupt_runtime_route),
        )
        .route(
            "/runtime-sessions/{runtime_session_id}/stop",
            post(stop_runtime_route),
        )
        .route(
            "/runtime-sessions/{runtime_session_id}/resume",
            post(resume_runtime_route),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_web_auth,
        ));

    Router::new()
        .merge(mcp)
        .nest("/api/v1", public_api.merge(client_api))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            public_ingress_guard,
        ))
        .with_state(state.clone())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            HTTP_REQUEST_TIMEOUT,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_HTTP_REQUEST_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_request_metrics,
        ))
        .layer(middleware::from_fn(correlation_response_header))
        .layer(cors)
}

pub(crate) async fn record_request_metrics(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let started = Instant::now();
    state.core_metrics.requests.fetch_add(1, Ordering::Relaxed);
    state
        .core_metrics
        .requests_in_flight
        .fetch_add(1, Ordering::Relaxed);
    let response = next.run(request).await;
    state
        .core_metrics
        .requests_in_flight
        .fetch_sub(1, Ordering::Relaxed);
    state.core_metrics.request_duration_micros.fetch_add(
        started.elapsed().as_micros().min(u64::MAX as u128) as u64,
        Ordering::Relaxed,
    );
    if response.status().is_client_error() || response.status().is_server_error() {
        state
            .core_metrics
            .request_errors
            .fetch_add(1, Ordering::Relaxed);
    }
    response
}

pub(crate) async fn public_ingress_guard(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response<Body>, AppError> {
    let path = request.uri().path();
    if matches!(
        path,
        "/api/v1/health" | "/api/v1/version" | "/api/v1/metrics"
    ) {
        return Ok(next.run(request).await);
    }
    let permit = state
        .public_concurrency
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError::rate_limited(
                "request.concurrency_limited",
                "Too many concurrent requests",
            )
        })?;
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    enforce_public_rate(&state, "global", state.config.public_global_rate_limit).await?;
    let (peer_bucket, peer_limit) =
        public_peer_rate_policy(path, state.config.public_peer_rate_limit);
    enforce_public_rate(&state, &format!("peer:{peer}:{peer_bucket}"), peer_limit).await?;
    let response = next.run(request).await;
    drop(permit);
    Ok(response)
}

pub(crate) fn public_peer_rate_policy(path: &str, ui_limit: usize) -> (&'static str, usize) {
    if path.contains("/auth/") {
        ("auth", 30)
    } else if path.contains("enrollment") {
        ("enrollment", 30)
    } else if path.ends_with("/client/logs") {
        ("client_logs", 120)
    } else if path.starts_with("/api/v1/node/") {
        ("node", PUBLIC_NODE_RATE_LIMIT)
    } else if path.ends_with("/stream") {
        ("stream", PUBLIC_STREAM_RATE_LIMIT)
    } else {
        ("ui", ui_limit)
    }
}

pub(crate) async fn enforce_public_rate(
    state: &AppState,
    key: &str,
    limit: usize,
) -> Result<(), AppError> {
    let now = Utc::now();
    let cutoff = now - chrono::Duration::seconds(state.config.public_rate_window_seconds);
    let mut requests = state.public_requests.write().await;
    let entries = requests.entry(key.to_owned()).or_default();
    entries.retain(|timestamp| *timestamp > cutoff);
    if entries.len() >= limit {
        state
            .core_metrics
            .public_rate_rejections
            .fetch_add(1, Ordering::Relaxed);
        return Err(AppError::rate_limited(
            "request.rate_limited",
            "Request rate limit exceeded",
        ));
    }
    entries.push(now);
    Ok(())
}

pub(crate) async fn correlation_response_header(
    mut request: Request,
    next: Next,
) -> Response<Body> {
    let correlation_id = request_correlation_id(request.headers());
    let method = request.method().clone();
    let header = HeaderValue::from_str(correlation_id.as_str())
        .unwrap_or_else(|_| HeaderValue::from_static("invalid-correlation-id"));
    request.headers_mut().insert(
        HeaderName::from_static(CORRELATION_ID_HEADER),
        header.clone(),
    );
    let span = tracing::info_span!("http.request", %correlation_id, %method);
    let mut response = REQUEST_CORRELATION_ID
        .scope(correlation_id, next.run(request).instrument(span))
        .await;
    response
        .headers_mut()
        .insert(HeaderName::from_static(CORRELATION_ID_HEADER), header);
    response
}

pub(crate) fn cors_layer(config: &AppConfig) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(config.allowed_origins.clone()))
        .allow_methods([Method::DELETE, Method::GET, Method::POST])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("last-event-id"),
            HeaderName::from_static(CSRF_HEADER),
            HeaderName::from_static(CORRELATION_ID_HEADER),
            HeaderName::from_static(REQUEST_ID_HEADER),
        ])
        .expose_headers([HeaderName::from_static(CORRELATION_ID_HEADER)])
        .allow_credentials(true)
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            let _ = signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

pub(crate) async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        profile: state.config.profile,
        security: security_status(&state).await,
    })
}

pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pending_outbox: i64 = sqlx::query_scalar(
        "select count(*) from event_publication_outbox where published_at is null",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    let runtime_states: Vec<(String, String, String, i64)> = sqlx::query_as(
        "select provider, execution_profile, recovery_status, count(*) from runtime_sessions group by provider, execution_profile, recovery_status",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let interaction_states: Vec<(String, String, String, i64)> = sqlx::query_as(
        r#"
        select r.provider, r.execution_profile, i.state, count(*)
        from provider_interactions i
        join runtime_sessions r on r.runtime_session_id = i.runtime_session_id
        group by r.provider, r.execution_profile, i.state
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let mut managed_metrics = String::from(
        "# HELP uprava_core_managed_runtime_states Managed runtime sessions by bounded provider/profile/recovery state.\n# TYPE uprava_core_managed_runtime_states gauge\n",
    );
    for (provider, profile, recovery, count) in runtime_states {
        managed_metrics.push_str(&format!(
            "uprava_core_managed_runtime_states{{provider=\"{}\",profile=\"{}\",state=\"{}\"}} {}\n",
            prometheus_label(&provider),
            prometheus_label(&profile),
            prometheus_label(&recovery),
            count,
        ));
    }
    managed_metrics.push_str(
        "# HELP uprava_core_provider_interactions Provider interactions by bounded provider/profile/state.\n# TYPE uprava_core_provider_interactions gauge\n",
    );
    for (provider, profile, interaction_state, count) in interaction_states {
        managed_metrics.push_str(&format!(
            "uprava_core_provider_interactions{{provider=\"{}\",profile=\"{}\",state=\"{}\"}} {}\n",
            prometheus_label(&provider),
            prometheus_label(&profile),
            prometheus_label(&interaction_state),
            count,
        ));
    }
    let body = format!(
        "{}# HELP uprava_core_event_publication_outbox_pending Pending event publications.\n# TYPE uprava_core_event_publication_outbox_pending gauge\nuprava_core_event_publication_outbox_pending {}\n{}",
        state.core_metrics.render(),
        pending_outbox,
        managed_metrics,
    );
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

fn prometheus_label(value: &str) -> String {
    value
        .chars()
        .take(100)
        .fold(String::new(), |mut escaped, character| {
            match character {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' | '\r' => escaped.push(' '),
                other => escaped.push(other),
            }
            escaped
        })
}

pub(crate) async fn version(State(state): State<Arc<AppState>>) -> Json<VersionResponse> {
    Json(VersionResponse {
        name: "uprava-core".to_owned(),
        version: APP_VERSION.to_owned(),
        release_id: option_env!("UPRAVA_BUILD_GIT_SHA")
            .unwrap_or("dev")
            .to_owned(),
        api_version: API_VERSION.to_owned(),
        schema_version: SCHEMA_VERSION,
        profile: state.config.profile,
        security: security_status(&state).await,
    })
}

pub(crate) async fn auth_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<WebAuthStatusResponse>, AppError> {
    let security = security_status(&state).await;
    let authenticated = if state.config.web_auth_required {
        authenticated_web_session(&state, &headers).await?.is_some()
    } else {
        true
    };

    Ok(Json(WebAuthStatusResponse {
        auth_required: state.config.web_auth_required,
        setup_required: state.config.web_auth_required && !web_auth_configured(&state).await?,
        authenticated,
        profile: state.config.profile,
        security,
    }))
}

pub(crate) async fn auth_setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WebAuthSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_allowed_request_origin(&state, &headers, "web.auth.setup").await?;
    if !state.config.web_auth_required {
        return Ok(auth_response_with_cookies(
            &state,
            WebAuthResponse {
                authenticated: true,
                setup_required: false,
                csrf_token: None,
                security: security_status(&state).await,
            },
            None,
        ));
    }
    if web_auth_configured(&state).await? {
        return Err(AppError::bad_request(
            "auth.setup_unavailable",
            "Local web auth is already configured",
        ));
    }
    validate_local_password(&request.password)?;

    let now = Utc::now();
    let password_hash = hash_password(&request.password)?;
    sqlx::query(
        r#"
        insert into web_admin (id, password_hash, created_at, updated_at)
        values (1, ?1, ?2, ?2)
        "#,
    )
    .bind(password_hash)
    .bind(now)
    .execute(&state.pool)
    .await?;
    audit_security_event(
        &state,
        "web.auth.setup",
        None,
        Some(header_value(&headers, "origin").unwrap_or_default()),
        "accepted",
        JsonValue(json!({})),
    )
    .await?;

    let session = create_web_session(&state).await?;
    Ok(auth_response_with_cookies(
        &state,
        WebAuthResponse {
            authenticated: true,
            setup_required: false,
            csrf_token: Some(session.csrf_token.clone()),
            security: security_status(&state).await,
        },
        Some(session),
    ))
}

pub(crate) async fn auth_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WebAuthLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    ensure_allowed_request_origin(&state, &headers, "web.auth.login").await?;
    if !state.config.web_auth_required {
        return Ok(auth_response_with_cookies(
            &state,
            WebAuthResponse {
                authenticated: true,
                setup_required: false,
                csrf_token: None,
                security: security_status(&state).await,
            },
            None,
        ));
    }
    reject_if_auth_rate_limited(&state, "web:login").await?;
    let Some(stored_hash) =
        sqlx::query_scalar::<_, String>("select password_hash from web_admin where id = 1")
            .fetch_optional(&state.pool)
            .await?
    else {
        return Err(AppError::auth(
            "auth.setup_required",
            "Local web auth setup is required",
        ));
    };
    if !verify_password(&stored_hash, &request.password) {
        audit_security_event(
            &state,
            "web.auth.login_failed",
            None,
            Some(header_value(&headers, "origin").unwrap_or_default()),
            "rejected",
            JsonValue(json!({ "reason": "invalid_password" })),
        )
        .await?;
        record_auth_failure(&state, "web:login").await;
        return Err(AppError::auth(
            "auth.invalid_credentials",
            "Password is invalid",
        ));
    }

    clear_auth_failures(&state, "web:login").await;
    let session = create_web_session(&state).await?;
    audit_security_event(
        &state,
        "web.auth.login",
        None,
        Some(header_value(&headers, "origin").unwrap_or_default()),
        "accepted",
        JsonValue(json!({})),
    )
    .await?;
    Ok(auth_response_with_cookies(
        &state,
        WebAuthResponse {
            authenticated: true,
            setup_required: false,
            csrf_token: Some(session.csrf_token.clone()),
            security: security_status(&state).await,
        },
        Some(session),
    ))
}

pub(crate) async fn auth_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    ensure_allowed_request_origin(&state, &headers, "web.auth.logout").await?;
    if let Some(session_token) = cookie_value(&headers, SESSION_COOKIE) {
        sqlx::query("delete from web_sessions where session_hash = ?1")
            .bind(hash_secret(&session_token))
            .execute(&state.pool)
            .await?;
    }
    audit_security_event(
        &state,
        "web.auth.logout",
        None,
        Some(header_value(&headers, "origin").unwrap_or_default()),
        "accepted",
        JsonValue(json!({})),
    )
    .await?;

    let mut response = Json(WebAuthResponse {
        authenticated: false,
        setup_required: state.config.web_auth_required && !web_auth_configured(&state).await?,
        csrf_token: None,
        security: security_status(&state).await,
    })
    .into_response();
    clear_auth_cookies(&state, response.headers_mut());
    Ok(response)
}

#[derive(Debug, Clone)]
pub(crate) struct WebSessionTokens {
    pub(crate) session_token: String,
    pub(crate) csrf_token: String,
}

pub(crate) async fn security_status(state: &AppState) -> SecurityStatus {
    SecurityStatus {
        mode: SecurityMode::Hardened,
        web_auth_required: state.config.web_auth_required,
        web_auth_configured: web_auth_configured(state).await.unwrap_or(false),
        cookie_secure: state.config.cookie_secure,
    }
}

pub(crate) async fn web_auth_configured(state: &AppState) -> Result<bool, AppError> {
    Ok(
        sqlx::query_scalar::<_, i64>("select count(*) from web_admin where id = 1")
            .fetch_one(&state.pool)
            .await?
            > 0,
    )
}

pub(crate) async fn reject_if_auth_rate_limited(
    state: &AppState,
    key: &str,
) -> Result<(), AppError> {
    let cutoff = Utc::now() - chrono::Duration::seconds(AUTH_FAILURE_WINDOW_SECONDS);
    let mut failures = state.auth_failures.write().await;
    let entries = failures.entry(key.to_owned()).or_default();
    entries.retain(|timestamp| *timestamp > cutoff);
    if entries.len() >= AUTH_FAILURE_LIMIT {
        return Err(AppError::rate_limited(
            "auth.rate_limited",
            "Too many authentication failures; retry later",
        ));
    }
    Ok(())
}

pub(crate) async fn record_auth_failure(state: &AppState, key: &str) {
    state
        .core_metrics
        .auth_failures
        .fetch_add(1, Ordering::Relaxed);
    let cutoff = Utc::now() - chrono::Duration::seconds(AUTH_FAILURE_WINDOW_SECONDS);
    let mut failures = state.auth_failures.write().await;
    let entries = failures.entry(key.to_owned()).or_default();
    entries.retain(|timestamp| *timestamp > cutoff);
    entries.push(Utc::now());
}

pub(crate) async fn clear_auth_failures(state: &AppState, key: &str) {
    state.auth_failures.write().await.remove(key);
}

pub(crate) async fn create_web_session(state: &AppState) -> Result<WebSessionTokens, AppError> {
    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(state.config.web_session_ttl_seconds);
    let tokens = WebSessionTokens {
        session_token: new_secret("web-session"),
        csrf_token: new_secret("csrf"),
    };
    sqlx::query(
        r#"
        insert into web_sessions (
            session_hash, csrf_hash, created_at, last_seen_at, expires_at
        )
        values (?1, ?2, ?3, ?3, ?4)
        "#,
    )
    .bind(hash_secret(&tokens.session_token))
    .bind(hash_secret(&tokens.csrf_token))
    .bind(now)
    .bind(expires_at)
    .execute(&state.pool)
    .await?;
    Ok(tokens)
}

pub(crate) async fn authenticated_web_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, AppError> {
    let Some(session_token) = cookie_value(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let session_hash = hash_secret(&session_token);
    let row = sqlx::query(
        r#"
        select expires_at
        from web_sessions
        where session_hash = ?1
        "#,
    )
    .bind(&session_hash)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let expires_at: DateTime<Utc> = row.try_get("expires_at")?;
    let now = Utc::now();
    if expires_at <= now {
        sqlx::query("delete from web_sessions where session_hash = ?1")
            .bind(&session_hash)
            .execute(&state.pool)
            .await?;
        return Ok(None);
    }
    sqlx::query("update web_sessions set last_seen_at = ?1 where session_hash = ?2")
        .bind(now)
        .bind(&session_hash)
        .execute(&state.pool)
        .await?;
    Ok(Some(session_hash))
}

pub(crate) async fn require_web_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response<Body>, AppError> {
    if !state.config.web_auth_required {
        return Ok(next.run(request).await);
    }
    if !web_auth_configured(&state).await? {
        return Err(AppError::auth(
            "auth.setup_required",
            "Local web auth setup is required",
        ));
    }

    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let Some(session_hash) = authenticated_web_session(&state, &headers).await? else {
        audit_security_event(
            &state,
            "web.auth.required",
            None,
            header_value(&headers, "origin"),
            "rejected",
            JsonValue(json!({ "path": path })),
        )
        .await?;
        return Err(AppError::auth(
            "auth.required",
            "Authentication is required",
        ));
    };

    if let Some(origin) = header_value(&headers, "origin") {
        if !origin_allowed(&state.config, &origin) {
            audit_security_event(
                &state,
                "web.auth.origin_rejected",
                None,
                Some(origin),
                "rejected",
                JsonValue(json!({ "path": path })),
            )
            .await?;
            return Err(AppError::auth(
                "auth.origin_rejected",
                "Request origin is not allowed",
            ));
        }
    }

    if matches!(
        method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        verify_csrf(&state, &headers, &session_hash).await?;
    }

    Ok(next.run(request).await)
}

pub(crate) async fn verify_csrf(
    state: &AppState,
    headers: &HeaderMap,
    session_hash: &str,
) -> Result<(), AppError> {
    let header_token = header_value(headers, CSRF_HEADER);
    let cookie_token = cookie_value(headers, CSRF_COOKIE);
    let Some(header_token) = header_token else {
        audit_security_event(
            state,
            "web.auth.csrf_rejected",
            None,
            header_value(headers, "origin"),
            "rejected",
            JsonValue(json!({ "reason": "missing_header" })),
        )
        .await?;
        return Err(AppError::auth(
            "auth.csrf_required",
            "CSRF token is required",
        ));
    };
    let Some(cookie_token) = cookie_token else {
        audit_security_event(
            state,
            "web.auth.csrf_rejected",
            None,
            header_value(headers, "origin"),
            "rejected",
            JsonValue(json!({ "reason": "missing_cookie" })),
        )
        .await?;
        return Err(AppError::auth(
            "auth.csrf_required",
            "CSRF token is required",
        ));
    };
    if !constant_time_eq(header_token.as_bytes(), cookie_token.as_bytes()) {
        audit_security_event(
            state,
            "web.auth.csrf_rejected",
            None,
            header_value(headers, "origin"),
            "rejected",
            JsonValue(json!({ "reason": "header_cookie_mismatch" })),
        )
        .await?;
        return Err(AppError::auth("auth.csrf_invalid", "CSRF token is invalid"));
    }
    let Some(stored_hash) = sqlx::query_scalar::<_, String>(
        "select csrf_hash from web_sessions where session_hash = ?1",
    )
    .bind(session_hash)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Err(AppError::auth(
            "auth.required",
            "Authentication is required",
        ));
    };
    if !constant_time_eq(
        stored_hash.as_bytes(),
        hash_secret(&header_token).as_bytes(),
    ) {
        audit_security_event(
            state,
            "web.auth.csrf_rejected",
            None,
            header_value(headers, "origin"),
            "rejected",
            JsonValue(json!({ "reason": "stored_hash_mismatch" })),
        )
        .await?;
        return Err(AppError::auth("auth.csrf_invalid", "CSRF token is invalid"));
    }
    Ok(())
}

pub(crate) async fn ensure_allowed_request_origin(
    state: &AppState,
    headers: &HeaderMap,
    event_kind: &str,
) -> Result<(), AppError> {
    let Some(origin) = header_value(headers, "origin") else {
        return Ok(());
    };
    if origin_allowed(&state.config, &origin) {
        return Ok(());
    }
    audit_security_event(
        state,
        event_kind,
        None,
        Some(origin),
        "rejected",
        JsonValue(json!({ "reason": "origin_not_allowed" })),
    )
    .await?;
    Err(AppError::auth(
        "auth.origin_rejected",
        "Request origin is not allowed",
    ))
}

pub(crate) fn auth_response_with_cookies(
    state: &AppState,
    body: WebAuthResponse,
    tokens: Option<WebSessionTokens>,
) -> Response<Body> {
    let mut response = Json(body).into_response();
    if let Some(tokens) = tokens {
        set_auth_cookies(state, response.headers_mut(), &tokens);
    }
    response
}

pub(crate) fn set_auth_cookies(
    state: &AppState,
    headers: &mut HeaderMap,
    tokens: &WebSessionTokens,
) {
    append_set_cookie(
        headers,
        build_cookie(
            SESSION_COOKIE,
            &tokens.session_token,
            true,
            state.config.cookie_secure,
            state.config.web_session_ttl_seconds,
        ),
    );
    append_set_cookie(
        headers,
        build_cookie(
            CSRF_COOKIE,
            &tokens.csrf_token,
            false,
            state.config.cookie_secure,
            state.config.web_session_ttl_seconds,
        ),
    );
}

pub(crate) fn clear_auth_cookies(state: &AppState, headers: &mut HeaderMap) {
    append_set_cookie(
        headers,
        build_cookie(SESSION_COOKIE, "", true, state.config.cookie_secure, 0),
    );
    append_set_cookie(
        headers,
        build_cookie(CSRF_COOKIE, "", false, state.config.cookie_secure, 0),
    );
}

pub(crate) fn append_set_cookie(headers: &mut HeaderMap, value: String) {
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.append(SET_COOKIE, value);
    }
}

pub(crate) fn build_cookie(
    name: &str,
    value: &str,
    http_only: bool,
    secure: bool,
    max_age_seconds: i64,
) -> String {
    let mut cookie = format!(
        "{name}={value}; Path=/; Max-Age={}; SameSite=Lax",
        max_age_seconds.max(0)
    );
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub(crate) async fn client_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ClientLogRequest>,
) -> Result<Json<ClientLogResponse>, AppError> {
    let accepted_at = Utc::now();
    let user_agent = request
        .user_agent
        .filter(|value| !value.trim().is_empty())
        .or_else(|| header_value(&headers, "user-agent"))
        .map(|value| truncate_chars(value.trim(), MAX_CLIENT_LOG_FIELD_CHARS));
    let record = json!({
        "accepted_at": accepted_at,
        "occurred_at": request.occurred_at,
        "level": format_client_log_level(request.level),
        "source": truncate_chars(request.source.trim(), MAX_CLIENT_LOG_FIELD_CHARS),
        "message": truncate_chars(request.message.trim(), MAX_CLIENT_LOG_FIELD_CHARS),
        "route": request
            .route
            .filter(|value| !value.trim().is_empty())
            .map(|value| truncate_chars(value.trim(), MAX_CLIENT_LOG_FIELD_CHARS)),
        "user_agent": user_agent,
        "detail": truncate_chars(
            &serde_json::to_string(&request.detail.0).unwrap_or_else(|_| "null".to_owned()),
            MAX_CLIENT_LOG_DETAIL_CHARS,
        ),
    });
    let _log_guard = state.client_log_lock.lock().await;
    append_jsonl_log(
        state.config.client_log_file.clone(),
        serde_json::to_string(&record)?,
    )
    .await?;
    tracing::debug!(
        level = format_client_log_level(request.level),
        source = %request.source,
        "client log accepted"
    );
    Ok(Json(ClientLogResponse { accepted: true }))
}
