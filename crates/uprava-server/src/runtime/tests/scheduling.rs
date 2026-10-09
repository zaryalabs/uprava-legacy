use super::*;

#[tokio::test]
async fn scheduled_message_persists_and_is_visible_in_its_session() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let due_at = Utc::now() + chrono::Duration::minutes(5);

    let scheduled = create_scheduled_message_route(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(CreateScheduledMessageRequest {
            content: "check the final result".to_owned(),
            due_at,
            timezone: "Europe/Moscow".to_owned(),
        }),
    )
    .await
    .expect("scheduled message creates")
    .0;
    let reloaded = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session detail loads");

    assert_eq!(scheduled.state, ScheduledMessageState::Scheduled);
    assert_eq!(scheduled.timezone, "Europe/Moscow");
    assert_eq!(reloaded.scheduled_messages, vec![scheduled]);
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn scheduled_message_dispatches_through_the_normal_turn_path() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let scheduled = create_scheduled_message_route(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(CreateScheduledMessageRequest {
            content: "run the deferred turn".to_owned(),
            due_at: Utc::now() + chrono::Duration::minutes(5),
            timezone: "UTC".to_owned(),
        }),
    )
    .await
    .expect("scheduled message creates")
    .0;

    claim_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
        "scheduled",
    )
    .await
    .expect("scheduled message claims");
    dispatch_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("scheduled message dispatches");
    let dispatched = load_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("scheduled message reloads");

    assert_eq!(dispatched.state, ScheduledMessageState::Sent);
    assert!(dispatched.command_id.is_some());
    assert!(dispatched.turn_id.is_some());
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn due_scheduled_message_is_claimed_and_dispatched_once() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let scheduled = create_scheduled_message_route(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(CreateScheduledMessageRequest {
            content: "dispatch when due".to_owned(),
            due_at: Utc::now() + chrono::Duration::minutes(5),
            timezone: "UTC".to_owned(),
        }),
    )
    .await
    .expect("scheduled message creates")
    .0;
    sqlx::query("update scheduled_messages set due_at = ?1 where scheduled_message_id = ?2")
        .bind(Utc::now() - chrono::Duration::seconds(1))
        .bind(&scheduled.scheduled_message_id)
        .execute(&state.pool)
        .await
        .expect("message becomes due");

    dispatch_due_scheduled_messages(&state)
        .await
        .expect("due messages dispatch");
    dispatch_due_scheduled_messages(&state)
        .await
        .expect("sent message is not redispatched");
    let command_count: i64 = sqlx::query_scalar(
        "select count(*) from commands where session_thread_id = ?1 and kind = 'SendTurn'",
    )
    .bind(detail.session.session_thread_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("command count loads");
    let dispatched = load_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("scheduled message reloads");

    assert_eq!(dispatched.state, ScheduledMessageState::Sent);
    assert_eq!(command_count, 1);
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn scheduled_message_records_typed_guard_failure_without_retrying() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let scheduled = create_scheduled_message_route(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(CreateScheduledMessageRequest {
            content: "do not dispatch while detached".to_owned(),
            due_at: Utc::now() + chrono::Duration::minutes(5),
            timezone: "UTC".to_owned(),
        }),
    )
    .await
    .expect("scheduled message creates")
    .0;
    sqlx::query("update session_threads set state = 'detached' where session_thread_id = ?1")
        .bind(detail.session.session_thread_id.as_str())
        .execute(&state.pool)
        .await
        .expect("session detaches");

    claim_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
        "scheduled",
    )
    .await
    .expect("scheduled message claims");
    dispatch_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("failed dispatch is recorded");
    let failed = load_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("scheduled message reloads");

    assert_eq!(failed.state, ScheduledMessageState::Failed);
    assert_eq!(
        failed.failure.expect("typed failure exists").code,
        "session.detached"
    );
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn interrupted_scheduled_dispatch_becomes_a_visible_manual_retry() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let scheduled = create_scheduled_message_route(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(CreateScheduledMessageRequest {
            content: "recover after Core restart".to_owned(),
            due_at: Utc::now() + chrono::Duration::minutes(5),
            timezone: "UTC".to_owned(),
        }),
    )
    .await
    .expect("scheduled message creates")
    .0;
    claim_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
        "scheduled",
    )
    .await
    .expect("scheduled message claims");

    recover_interrupted_scheduled_messages(&state)
        .await
        .expect("interrupted dispatches recover");
    let failed = load_scheduled_message(
        &state,
        &detail.session.session_thread_id,
        &scheduled.scheduled_message_id,
    )
    .await
    .expect("scheduled message reloads");

    assert_eq!(failed.state, ScheduledMessageState::Failed);
    assert_eq!(
        failed.failure.expect("typed failure exists").code,
        "scheduled_message.dispatch_interrupted"
    );
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn background_job_is_paused_by_default_and_manual_overlap_is_visible() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let created = create_job_route(
        State(state.clone()),
        Json(CreateJobRequest {
            name: "Nightly checks".to_owned(),
            project_placement_id: detail.placement.project_placement_id.clone(),
            prompt: "Run make c and summarize failures".to_owned(),
            provider: "codex".to_owned(),
            schedule: Some(JobSchedule::Daily {
                hour: 2,
                minute: 30,
            }),
            timezone: "Europe/Moscow".to_owned(),
            continue_after_error: false,
        }),
    )
    .await
    .expect("Job creates")
    .0;

    assert!(!created.job.enabled);
    assert_eq!(created.job.paused_reason.as_deref(), Some("created_paused"));

    let first = run_job_route(
        State(state.clone()),
        Path(created.job.job_id.to_string()),
        Some(Json(RunJobRequest { force: false })),
    )
    .await
    .expect("manual test run starts")
    .0;
    let second = run_job_route(
        State(state.clone()),
        Path(created.job.job_id.to_string()),
        Some(Json(RunJobRequest { force: false })),
    )
    .await
    .expect("overlap is retained")
    .0;
    let job_runtime_profile: String = sqlx::query_scalar(
        "select execution_profile from runtime_sessions where runtime_session_id = ?1",
    )
    .bind(
        first
            .runtime_session_id
            .as_ref()
            .expect("starting Job has a runtime session")
            .as_str(),
    )
    .fetch_one(&state.pool)
    .await
    .expect("Job runtime profile loads");
    let _ = update_job_route(
        State(state.clone()),
        Path(created.job.job_id.to_string()),
        Json(UpdateJobRequest {
            name: None,
            prompt: Some("Use a changed prompt".to_owned()),
            provider: None,
            schedule: None,
            clear_schedule: false,
            timezone: None,
            continue_after_error: None,
        }),
    )
    .await
    .expect("future configuration updates");

    assert_eq!(first.state, JobRunState::Starting);
    assert_eq!(job_runtime_profile, "exec_compatibility");
    assert_eq!(second.state, JobRunState::Skipped);
    assert_eq!(
        second.terminal_reason.expect("skip reason exists").code,
        "job.overlap_skipped"
    );
    let snapshot: JobRunConfigSnapshot =
        serde_json::from_value(first.config_snapshot.0).expect("snapshot decodes");
    assert_eq!(snapshot.prompt, "Run make c and summarize failures");
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn interrupted_job_turn_handoff_reuses_the_existing_turn() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let job = create_job_route(
        State(state.clone()),
        Json(CreateJobRequest {
            name: "Recovery".to_owned(),
            project_placement_id: detail.placement.project_placement_id,
            prompt: "Resume without duplication".to_owned(),
            provider: "codex".to_owned(),
            schedule: None,
            timezone: "UTC".to_owned(),
            continue_after_error: false,
        }),
    )
    .await
    .expect("Job creates")
    .0;
    let starting = run_job_route(
        State(state.clone()),
        Path(job.job.job_id.to_string()),
        Some(Json(RunJobRequest { force: false })),
    )
    .await
    .expect("run starts")
    .0;
    sqlx::query("update runtime_sessions set state = 'ready' where runtime_session_id = ?1")
        .bind(
            starting
                .runtime_session_id
                .as_ref()
                .expect("runtime exists")
                .as_str(),
        )
        .execute(&state.pool)
        .await
        .expect("runtime becomes ready");
    drive_job_runs(&state).await.expect("prompt dispatches");
    let command_count_before: i64 = sqlx::query_scalar(
        "select count(*) from commands where session_thread_id = ?1 and kind = 'SendTurn'",
    )
    .bind(
        starting
            .session_thread_id
            .as_ref()
            .expect("session exists")
            .as_str(),
    )
    .fetch_one(&state.pool)
    .await
    .expect("command count loads");
    sqlx::query("update job_runs set state = 'starting' where job_run_id = ?1")
        .bind(starting.job_run_id.as_str())
        .execute(&state.pool)
        .await
        .expect("restart gap is simulated");
    sqlx::query("update runtime_sessions set state = 'running' where runtime_session_id = ?1")
        .bind(
            starting
                .runtime_session_id
                .as_ref()
                .expect("runtime exists")
                .as_str(),
        )
        .execute(&state.pool)
        .await
        .expect("runtime is already running");

    drive_job_runs(&state)
        .await
        .expect("recovery links existing turn");
    let recovered = load_job_run(&state, &starting.job_run_id)
        .await
        .expect("run reloads");
    let command_count_after: i64 = sqlx::query_scalar(
        "select count(*) from commands where session_thread_id = ?1 and kind = 'SendTurn'",
    )
    .bind(
        starting
            .session_thread_id
            .as_ref()
            .expect("session exists")
            .as_str(),
    )
    .fetch_one(&state.pool)
    .await
    .expect("command count reloads");

    assert_eq!(recovered.state, JobRunState::Running);
    assert_eq!(command_count_before, command_count_after);
    let turn_id: String = sqlx::query_scalar("select turn_id from job_runs where job_run_id = ?1")
        .bind(starting.job_run_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("turn id loads");
    sqlx::query(
        "insert into messages (message_id, session_thread_id, turn_id, role, content, created_at, completed_at, source_event_id) values (?1, ?2, ?3, 'assistant', 'Recovered summary', ?4, ?4, null)",
    )
    .bind(MessageId::new().as_str())
    .bind(
        starting
            .session_thread_id
            .as_ref()
            .expect("session exists")
            .as_str(),
    )
    .bind(&turn_id)
    .bind(Utc::now())
    .execute(&state.pool)
    .await
    .expect("assistant summary stores");
    sqlx::query("update turns set state = 'completed', completed_at = ?1 where turn_id = ?2")
        .bind(Utc::now())
        .bind(turn_id)
        .execute(&state.pool)
        .await
        .expect("turn completes");
    drive_job_runs(&state).await.expect("completion projects");
    let succeeded = load_job_run(&state, &starting.job_run_id)
        .await
        .expect("completed run reloads");
    assert_eq!(succeeded.state, JobRunState::Succeeded);
    assert_eq!(succeeded.summary.as_deref(), Some("Recovered summary"));
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn due_job_occurrence_is_claimed_once_and_advances_schedule() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let created = create_job_route(
        State(state.clone()),
        Json(CreateJobRequest {
            name: "Frequent check".to_owned(),
            project_placement_id: detail.placement.project_placement_id,
            prompt: "Check status".to_owned(),
            provider: "codex".to_owned(),
            schedule: Some(JobSchedule::Interval { minutes: 15 }),
            timezone: "UTC".to_owned(),
            continue_after_error: true,
        }),
    )
    .await
    .expect("Job creates")
    .0;
    let _ = enable_job_route(State(state.clone()), Path(created.job.job_id.to_string()))
        .await
        .expect("Job enables");
    let due_at = Utc::now() - chrono::Duration::seconds(1);
    sqlx::query("update jobs set next_run_at = ?1 where job_id = ?2")
        .bind(due_at)
        .bind(created.job.job_id.as_str())
        .execute(&state.pool)
        .await
        .expect("Job becomes due");

    dispatch_due_jobs(&state)
        .await
        .expect("first tick succeeds");
    dispatch_due_jobs(&state)
        .await
        .expect("second tick succeeds");

    let scheduled_count: i64 = sqlx::query_scalar(
        "select count(*) from job_runs where job_id = ?1 and trigger_kind = 'scheduled'",
    )
    .bind(created.job.job_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("scheduled count loads");
    let next_run_at: DateTime<Utc> =
        sqlx::query_scalar("select next_run_at from jobs where job_id = ?1")
            .bind(created.job.job_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("next run loads");

    assert_eq!(scheduled_count, 1);
    assert!(next_run_at > Utc::now());
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn failed_job_run_pauses_default_schedule_but_continue_after_error_does_not() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let create = |name: &str, continue_after_error: bool| CreateJobRequest {
        name: name.to_owned(),
        project_placement_id: detail.placement.project_placement_id.clone(),
        prompt: "Run unattended".to_owned(),
        provider: "codex".to_owned(),
        schedule: Some(JobSchedule::Interval { minutes: 60 }),
        timezone: "UTC".to_owned(),
        continue_after_error,
    };
    let stops = create_job_route(State(state.clone()), Json(create("Stops", false)))
        .await
        .expect("stop-on-error Job creates")
        .0;
    let continues = create_job_route(State(state.clone()), Json(create("Continues", true)))
        .await
        .expect("continue Job creates")
        .0;
    let _ = enable_job_route(State(state.clone()), Path(stops.job.job_id.to_string()))
        .await
        .expect("stop-on-error Job enables");
    let _ = enable_job_route(State(state.clone()), Path(continues.job.job_id.to_string()))
        .await
        .expect("continue Job enables");
    sqlx::query(
        "update nodes set presence = 'offline', last_heartbeat_at = null where node_id = ?1",
    )
    .bind(node_id.as_str())
    .execute(&state.pool)
    .await
    .expect("node goes offline");

    let stopped_run = run_job_route(
        State(state.clone()),
        Path(stops.job.job_id.to_string()),
        Some(Json(RunJobRequest { force: false })),
    )
    .await
    .expect("failed run is retained")
    .0;
    let continued_run = run_job_route(
        State(state.clone()),
        Path(continues.job.job_id.to_string()),
        Some(Json(RunJobRequest { force: false })),
    )
    .await
    .expect("continued failed run is retained")
    .0;
    let stopped = load_job_detail(&state, &stops.job.job_id)
        .await
        .expect("stopped Job loads");
    let continued = load_job_detail(&state, &continues.job.job_id)
        .await
        .expect("continued Job loads");

    assert_eq!(stopped_run.state, JobRunState::Failed);
    assert_eq!(continued_run.state, JobRunState::Failed);
    assert!(!stopped.job.enabled);
    assert!(continued.job.enabled);
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn provider_quota_blocks_session_and_force_override_is_audited() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let unknown = provider_quota_status(&state, "codex")
        .await
        .expect("missing reliable quota is observable");
    assert_eq!(unknown.state, ProviderQuotaState::Unknown);
    sqlx::query(
        "insert into provider_quota_snapshots (provider, five_hour_remaining_percent, weekly_remaining_percent, observed_at, reliable, source) values ('codex', 5, 80, ?1, 1, 'test')",
    )
    .bind(Utc::now())
    .execute(&state.pool)
    .await
    .expect("quota snapshot stores");
    let request = |force| CreateSessionRequest {
        project_placement_id: detail.placement.project_placement_id.clone(),
        title: Some("Quota session".to_owned()),
        provider: "codex".to_owned(),
        execution_profile: Some(AgentExecutionProfile::ExecCompatibility),
        force,
    };

    let blocked = create_session(State(state.clone()), Json(request(false))).await;
    let _ = create_session(State(state.clone()), Json(request(true)))
        .await
        .expect("forced session starts");
    let audit_count: i64 = sqlx::query_scalar(
        "select count(*) from security_audit_events where kind = 'provider.quota_force_override'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("audit count loads");

    assert!(matches!(
        blocked,
        Err(AppError::BadRequest {
            code: "provider.quota_limited",
            ..
        })
    ));
    assert_eq!(audit_count, 1);
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[test]
fn calendar_job_schedule_uses_explicit_iana_timezone() {
    let after = DateTime::parse_from_rfc3339("2026-03-28T23:30:00Z")
        .expect("fixture parses")
        .with_timezone(&Utc);
    let next = next_job_run_at(
        &JobSchedule::Daily {
            hour: 3,
            minute: 30,
        },
        "Europe/Berlin",
        after,
    )
    .expect("next DST-aware occurrence computes");

    assert_eq!(next.to_rfc3339(), "2026-03-29T01:30:00+00:00");
}
