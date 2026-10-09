use super::*;

#[test]
fn app_config_from_env_uses_documented_defaults() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);

    let config = AppConfig::from_env().expect("default core config parses");

    assert_eq!(config.bind_address, "127.0.0.1:8080");
    assert_eq!(
        config.database_url,
        "sqlite://.local/state/core/core.sqlite"
    );
    assert_eq!(config.profile, DeploymentProfile::ControlledDev);
    assert_eq!(
        config
            .allowed_origins
            .iter()
            .map(|origin| origin.to_str().expect("origin is utf8"))
            .collect::<Vec<_>>(),
        vec!["http://127.0.0.1:5173", "http://localhost:5173"]
    );
    assert_eq!(config.stale_after_seconds, 15);
    assert_eq!(config.offline_after_seconds, 45);
    assert_eq!(config.enrollment_ttl_seconds, 600);
    assert_eq!(config.runtime_expiry_seconds, 86_400);
    assert!(!config.auto_approve_enrollments);
    assert_eq!(config.auto_approve_node_name, None);
    assert_eq!(
        config.client_log_file,
        PathBuf::from(".local/logs/client.log")
    );
    assert!(config.web_auth_required);
    assert_eq!(config.web_session_ttl_seconds, 86_400);
    assert!(!config.cookie_secure);
    assert_eq!(config.core_shutdown_timeout_seconds, 5);
    assert_eq!(config.public_rate_window_seconds, 60);
    assert_eq!(config.public_global_rate_limit, 5_000);
    assert_eq!(config.public_peer_rate_limit, 600);
    assert_eq!(config.generated_ui_builder_url, "http://127.0.0.1:18082");
    assert_eq!(config.generated_ui_builder_timeout_seconds, 15);
}

#[test]
fn app_config_from_env_parses_overrides() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_CORE_BIND", "127.0.0.1:19080");
    std::env::set_var(
        "UPRAVA_ALLOWED_ORIGINS",
        "http://127.0.0.1:5173, http://localhost:4173",
    );
    std::env::set_var("UPRAVA_DATABASE_URL", "sqlite:///tmp/uprava-test.sqlite");
    std::env::set_var("UPRAVA_DEPLOYMENT_PROFILE", "controlled_dev");
    std::env::set_var("UPRAVA_HEARTBEAT_STALE_SECONDS", "3");
    std::env::set_var("UPRAVA_HEARTBEAT_OFFLINE_SECONDS", "9");
    std::env::set_var("UPRAVA_ENROLLMENT_TTL_SECONDS", "30");
    std::env::set_var("UPRAVA_RUNTIME_EXPIRY_SECONDS", "120");
    std::env::set_var("UPRAVA_CLIENT_LOG_FILE", "/tmp/uprava-client.jsonl");
    std::env::set_var("UPRAVA_WEB_SESSION_TTL_SECONDS", "3600");
    std::env::set_var("UPRAVA_COOKIE_SECURE", "true");
    std::env::set_var("UPRAVA_CORE_SHUTDOWN_TIMEOUT_SECONDS", "2");
    std::env::set_var("UPRAVA_AUTO_APPROVE_NODE_NAME", " Zarya Server ");
    std::env::set_var("UPRAVA_PUBLIC_RATE_WINDOW_SECONDS", "30");
    std::env::set_var("UPRAVA_PUBLIC_GLOBAL_RATE_LIMIT", "9000");
    std::env::set_var("UPRAVA_PUBLIC_PEER_RATE_LIMIT", "900");
    std::env::set_var(
        "UPRAVA_GENERATED_UI_BUILDER_URL",
        "http://builder.internal:19082",
    );
    std::env::set_var("UPRAVA_GENERATED_UI_BUILDER_TIMEOUT_SECONDS", "7");

    let config = AppConfig::from_env().expect("overridden core config parses");

    assert_eq!(config.bind_address, "127.0.0.1:19080");
    assert_eq!(config.database_url, "sqlite:///tmp/uprava-test.sqlite");
    assert_eq!(config.profile, DeploymentProfile::ControlledDev);
    assert_eq!(
        config
            .allowed_origins
            .iter()
            .map(|origin| origin.to_str().expect("origin is utf8"))
            .collect::<Vec<_>>(),
        vec!["http://127.0.0.1:5173", "http://localhost:4173"]
    );
    assert_eq!(config.stale_after_seconds, 3);
    assert_eq!(config.offline_after_seconds, 9);
    assert_eq!(config.enrollment_ttl_seconds, 30);
    assert_eq!(config.runtime_expiry_seconds, 120);
    assert!(!config.auto_approve_enrollments);
    assert_eq!(
        config.auto_approve_node_name.as_deref(),
        Some("Zarya Server")
    );
    assert_eq!(
        config.client_log_file,
        PathBuf::from("/tmp/uprava-client.jsonl")
    );
    assert!(config.web_auth_required);
    assert_eq!(config.web_session_ttl_seconds, 3600);
    assert!(config.cookie_secure);
    assert_eq!(config.core_shutdown_timeout_seconds, 2);
    assert_eq!(config.public_rate_window_seconds, 30);
    assert_eq!(config.public_global_rate_limit, 9_000);
    assert_eq!(config.public_peer_rate_limit, 900);
    assert_eq!(
        config.generated_ui_builder_url,
        "http://builder.internal:19082"
    );
    assert_eq!(config.generated_ui_builder_timeout_seconds, 7);
}

#[test]
fn app_config_rejects_non_http_generated_ui_builder_urls() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_GENERATED_UI_BUILDER_URL", "file:///tmp/builder");

    let error = AppConfig::from_env().expect_err("non-http builder URL should fail");

    assert!(matches!(
        error,
        ConfigError::InvalidGeneratedUiBuilderUrl(_)
    ));
}

#[test]
fn app_config_from_env_rejects_invalid_profile() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_DEPLOYMENT_PROFILE", "production");

    let error = AppConfig::from_env().expect_err("invalid profile should fail");

    assert!(matches!(error, ConfigError::InvalidProfile(profile) if profile == "production"));
}

#[test]
fn app_config_from_env_rejects_local_trusted_profile() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_DEPLOYMENT_PROFILE", "local_trusted");

    let error = AppConfig::from_env().expect_err("local_trusted profile should fail");

    assert!(matches!(error, ConfigError::InvalidProfile(profile) if profile == "local_trusted"));
}

#[test]
fn app_config_from_env_rejects_disabled_web_auth() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_WEB_AUTH", "disabled");

    let error = AppConfig::from_env().expect_err("disabled web auth should fail");

    assert!(matches!(error, ConfigError::InvalidWebAuthMode(mode) if mode == "disabled"));
}

#[test]
fn app_config_from_env_rejects_auto_approve_enrollments() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_AUTO_APPROVE_ENROLLMENTS", "yes");

    let error = AppConfig::from_env().expect_err("auto approval should fail");

    assert!(matches!(error, ConfigError::AutoApproveEnrollments));
}

#[test]
fn app_config_from_env_rejects_invalid_integer() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_HEARTBEAT_STALE_SECONDS", "fast");

    let error = AppConfig::from_env().expect_err("invalid integer should fail");

    assert!(matches!(
        error,
        ConfigError::InvalidInteger { name, .. }
            if name == "UPRAVA_HEARTBEAT_STALE_SECONDS"
    ));
}

#[test]
fn app_config_from_env_rejects_zero_rate_limit() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_PUBLIC_PEER_RATE_LIMIT", "0");

    let error = AppConfig::from_env().expect_err("zero rate limit should fail");

    assert!(matches!(
        error,
        ConfigError::NonPositiveInteger { name }
            if name == "UPRAVA_PUBLIC_PEER_RATE_LIMIT"
    ));
}

#[test]
fn app_config_from_env_rejects_wildcard_cors_origin() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(CORE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_ALLOWED_ORIGINS", "*");

    let error = AppConfig::from_env().expect_err("wildcard origin should fail");

    assert!(matches!(error, ConfigError::WildcardOrigin));
}

#[tokio::test]
async fn migration_creates_baseline_schema_from_empty_database() {
    let state = test_state().await;

    let table_count: i64 = sqlx::query_scalar(
        r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name in (
                  'nodes',
                  'node_enrollments',
                  'node_capabilities',
                  'actors',
                  'projects',
                  'project_placements',
                  'session_threads',
                  'runtime_sessions',
                  'turns',
                  'approvals',
                  'messages',
                  'commands',
                  'events',
                  'warning_acknowledgements',
                  'event_publication_outbox',
                  'command_dispatch_outbox',
                  'scheduled_messages',
                  'jobs',
                  'job_runs',
                  'provider_quota_snapshots',
                  'deductions',
                  'causality_narratives',
                  'causality_narrative_versions',
                  'artifacts',
                  'artifact_versions',
                  'tool_sources',
                  'tool_definitions',
                  'integration_connections',
                  'mcp_dependency_instances',
                  'observed_capabilities',
                  'tool_calls',
                  'tool_call_events',
                  'session_tool_snapshots',
                  'mcp_access_leases',
                  'runtime_attempts',
                  'provider_interactions'
              )
            "#,
    )
    .fetch_one(&state.pool)
    .await
    .expect("baseline tables count loads");

    assert_eq!(table_count, 36);

    let applied_versions: Vec<i64> =
        sqlx::query_scalar("select version from schema_migrations order by version")
            .fetch_all(&state.pool)
            .await
            .expect("migration versions load");
    assert_eq!(
        applied_versions,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
    );

    let metadata: (String, i64) =
        sqlx::query_as("select slot, schema_version from core_schema_meta")
            .fetch_one(&state.pool)
            .await
            .expect("core schema metadata loads");
    assert_eq!(metadata, (CORE_STATE_SLOT.to_owned(), SCHEMA_VERSION));
}

#[tokio::test]
async fn migration_runner_is_idempotent_and_does_not_duplicate_versions() {
    let pool = memory_pool().await;
    let state = AppState::new(test_config(86_400), pool)
        .await
        .expect("state migrates");
    state
        .migrate()
        .await
        .expect("second migration run succeeds");

    let migration_count: i64 = sqlx::query_scalar("select count(*) from schema_migrations")
        .fetch_one(&state.pool)
        .await
        .expect("migration count loads");
    assert_eq!(migration_count, 19);
}

#[tokio::test]
async fn migration_upgrades_the_0_2_10_numbered_baseline() {
    let pool = memory_pool().await;
    sqlx::query(
        "create table schema_migrations (version integer primary key, checksum text not null, applied_at text not null)",
    )
    .execute(&pool)
    .await
    .expect("migration history table creates");
    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 11)
    {
        for statement in migration.statements {
            if let Err(error) = sqlx::query(statement).execute(&pool).await {
                assert!(
                    migration.ignore_duplicate_columns && is_duplicate_column_error(&error),
                    "0.2.10 migration {} failed: {error}",
                    migration.version
                );
            }
        }
        sqlx::query(
            "insert into schema_migrations (version, checksum, applied_at) values (?1, ?2, ?3)",
        )
        .bind(migration.version)
        .bind(migration.checksum())
        .bind(Utc::now())
        .execute(&pool)
        .await
        .expect("0.2.10 migration history records");
    }

    let state = AppState::new(test_config(86_400), pool)
        .await
        .expect("0.2.10 state upgrades");
    let latest_version: i64 = sqlx::query_scalar("select max(version) from schema_migrations")
        .fetch_one(&state.pool)
        .await
        .expect("latest migration loads");
    let tooling_table_count: i64 = sqlx::query_scalar(
        "select count(*) from sqlite_master where type = 'table' and name in ('tool_definitions', 'tool_calls', 'mcp_access_leases')",
    )
    .fetch_one(&state.pool)
    .await
    .expect("tooling tables count loads");

    assert_eq!(latest_version, 19);
    assert_eq!(tooling_table_count, 3);
}

#[tokio::test]
async fn migration_18_keeps_existing_sessions_in_exec_compatibility() {
    let pool = memory_pool().await;
    sqlx::query(
        "create table schema_migrations (version integer primary key, checksum text not null, applied_at text not null)",
    )
    .execute(&pool)
    .await
    .expect("migration history table creates");
    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 17)
    {
        for statement in migration.statements {
            if let Err(error) = sqlx::query(statement).execute(&pool).await {
                assert!(
                    migration.ignore_duplicate_columns && is_duplicate_column_error(&error),
                    "migration {} failed: {error}",
                    migration.version
                );
            }
        }
        sqlx::query(
            "insert into schema_migrations (version, checksum, applied_at) values (?1, ?2, ?3)",
        )
        .bind(migration.version)
        .bind(migration.checksum())
        .bind(Utc::now())
        .execute(&pool)
        .await
        .expect("migration history records");
    }
    let now = Utc::now();
    sqlx::query("insert into nodes (node_id, display_name, presence, sleep_hint, daemon_version, active_runtime_count, capabilities_json, diagnostics, created_at, updated_at) values ('node-old', 'Old node', 'reachable', 'awake', '0.2.20', 0, '[]', '', ?1, ?1)")
        .bind(now)
        .execute(&pool)
        .await
        .expect("old node inserts");
    sqlx::query("insert into project_placements (project_placement_id, node_id, display_name, workspace_path, state, resource_badges_json, created_at, updated_at) values ('placement-old', 'node-old', 'Old workspace', '/tmp/old', 'validated', '[]', ?1, ?1)")
        .bind(now)
        .execute(&pool)
        .await
        .expect("old placement inserts");
    sqlx::query("insert into session_threads (session_thread_id, project_placement_id, runtime_session_id, title, state, provider, created_at, updated_at) values ('session-old', 'placement-old', 'runtime-old', 'Old session', 'active', 'codex', ?1, ?1)")
        .bind(now)
        .execute(&pool)
        .await
        .expect("old session inserts");
    sqlx::query("insert into runtime_sessions (runtime_session_id, session_thread_id, provider, state, resume_supported, created_at, updated_at) values ('runtime-old', 'session-old', 'codex', 'ready', 1, ?1, ?1)")
        .bind(now)
        .execute(&pool)
        .await
        .expect("old runtime inserts");

    let state = AppState::build_uninitialized(test_config(86_400), pool);
    state.migrate().await.expect("0.2.20 state upgrades");
    let profile: String = sqlx::query_scalar(
        "select execution_profile from runtime_sessions where runtime_session_id = 'runtime-old'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("execution profile loads");

    assert_eq!(profile, "exec_compatibility");
}

#[tokio::test]
async fn migration_runner_rejects_changed_applied_migration() {
    let state = test_state().await;
    sqlx::query("update schema_migrations set checksum = 'tampered' where version = 1")
        .execute(&state.pool)
        .await
        .expect("migration metadata updates");

    let error = state
        .migrate()
        .await
        .expect_err("tampered migration must be rejected");
    assert!(error.to_string().contains("checksum mismatch"));
}

#[tokio::test]
async fn migration_adds_core_identity_constraints() {
    let state = test_state().await;
    let index_count: i64 = sqlx::query_scalar(
            "select count(*) from sqlite_master where type = 'index' and name in ('project_placements_identity_idx', 'runtime_sessions_session_thread_idx', 'approvals_turn_idx', 'messages_source_event_idx')",
        )
        .fetch_one(&state.pool)
        .await
        .expect("invariant indexes load");
    assert_eq!(index_count, 4);

    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let duplicate = sqlx::query(
        r#"
            insert into runtime_sessions (
                runtime_session_id, session_thread_id, provider, state,
                resume_supported, degraded_reason, created_at, updated_at
            ) values (?1, ?2, 'codex', 'starting', 1, null, ?3, ?3)
            "#,
    )
    .bind("duplicate-runtime-session")
    .bind(detail.session.session_thread_id.as_str())
    .bind(Utc::now())
    .execute(&state.pool)
    .await;
    assert!(duplicate.is_err());

    let duplicate_placement = sqlx::query(
        r#"
            insert into project_placements (
                project_placement_id, project_id, node_id, display_name, workspace_path,
                state, resource_badges_json, last_validated_at, created_at, updated_at
            ) values (?1, ?2, ?3, 'duplicate', ?4, 'pending', '[]', null, ?5, ?5)
            "#,
    )
    .bind("duplicate-placement")
    .bind("different-project")
    .bind(detail.placement.node_id.as_str())
    .bind(detail.placement.workspace_path.as_str())
    .bind(Utc::now())
    .execute(&state.pool)
    .await;
    assert!(duplicate_placement.is_err());

    let now = Utc::now();
    for approval_id in ["approval-1", "approval-2"] {
        let result = sqlx::query(
            r#"
                insert into approvals (
                    approval_id, session_thread_id, turn_id, state,
                    request_payload_json, created_at, updated_at
                ) values (?1, ?2, 'turn-unique', 'requested', '{}', ?3, ?3)
                "#,
        )
        .bind(approval_id)
        .bind(detail.session.session_thread_id.as_str())
        .bind(now)
        .execute(&state.pool)
        .await;
        if approval_id == "approval-1" {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }

    for message_id in ["message-1", "message-2"] {
        let result = sqlx::query(
            r#"
                insert into messages (
                    message_id, session_thread_id, role, content, source_event_id, created_at
                ) values (?1, ?2, 'assistant', 'duplicate source', 'event-unique', ?3)
                "#,
        )
        .bind(message_id)
        .bind(detail.session.session_thread_id.as_str())
        .bind(now)
        .execute(&state.pool)
        .await;
        if message_id == "message-1" {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn migration_rejects_mismatched_state_slot_before_schema_writes() {
    let pool = memory_pool().await;
    sqlx::query(
            "create table core_schema_meta (slot text primary key, schema_version integer not null, created_at text not null, updated_at text not null)",
        )
        .execute(&pool)
        .await
        .expect("schema metadata table creates");
    sqlx::query(
            "insert into core_schema_meta (slot, schema_version, created_at, updated_at) values ('0.1.8', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("legacy metadata inserts");

    let error = match AppState::new(test_config(86_400), pool.clone()).await {
        Ok(_) => panic!("mismatched state slot must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("expected slot 0.2.0"));
    let migration_table_count: i64 = sqlx::query_scalar(
        "select count(*) from sqlite_master where type = 'table' and name = 'schema_migrations'",
    )
    .fetch_one(&pool)
    .await
    .expect("migration table count loads");
    assert_eq!(migration_table_count, 0);
}

#[tokio::test]
async fn migration_rejects_partial_unversioned_schema_before_writes() {
    let pool = memory_pool().await;
    sqlx::query("create table legacy_partial_state (value text)")
        .execute(&pool)
        .await
        .expect("partial legacy table creates");

    let error = match AppState::new(test_config(86_400), pool.clone()).await {
        Ok(_) => panic!("partial unversioned state must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("retained 0.1.x Core state"));
    let migration_table_count: i64 = sqlx::query_scalar(
        "select count(*) from sqlite_master where type = 'table' and name = 'schema_migrations'",
    )
    .fetch_one(&pool)
    .await
    .expect("migration table count loads");
    assert_eq!(migration_table_count, 0);
}

#[tokio::test]
async fn migration_rejects_multiple_core_schema_metadata_rows() {
    let pool = memory_pool().await;
    sqlx::query(
            "create table core_schema_meta (slot text primary key, schema_version integer not null, created_at text not null, updated_at text not null)",
        )
        .execute(&pool)
        .await
        .expect("schema metadata table creates");
    sqlx::query(
            "insert into core_schema_meta (slot, schema_version, created_at, updated_at) values ('0.2.0', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("first metadata row inserts");
    sqlx::query(
            "insert into core_schema_meta (slot, schema_version, created_at, updated_at) values ('0.2.0-duplicate', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("second metadata row inserts");

    let error = match AppState::new(test_config(86_400), pool).await {
        Ok(_) => panic!("multiple metadata rows must be rejected"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("exactly one Core schema metadata"));
}

#[tokio::test]
async fn migration_concurrent_file_backed_starts_share_one_numbered_history() {
    let db_path = std::env::temp_dir().join(format!("uprava-migrations-{}.sqlite", Uuid::new_v4()));
    remove_sqlite_file_set(&db_path);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_path)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5))
                .create_if_missing(true),
        )
        .await
        .expect("migration pool opens");
    let config = test_config(86_400);
    let first_state = AppState::build_uninitialized(config.clone(), pool.clone());
    let second_state = AppState::build_uninitialized(config, pool.clone());
    let (first, second) = tokio::join!(first_state.migrate(), second_state.migrate(),);
    first.expect("first migration succeeds");
    second.expect("second migration succeeds");
    let count: i64 = sqlx::query_scalar("select count(*) from schema_migrations")
        .fetch_one(&pool)
        .await
        .expect("migration count loads");
    assert_eq!(count, 19);
    drop(first_state);
    drop(second_state);
    pool.close().await;
    remove_sqlite_file_set(&db_path);
}

#[tokio::test]
async fn migration_rejects_unversioned_previous_dev_nodes_table_without_mutation() {
    let pool = memory_pool().await;
    sqlx::query(
        r#"
            create table nodes (
                node_id text primary key,
                display_name text not null,
                presence text not null,
                sleep_hint text not null,
                last_heartbeat_at text,
                daemon_version text not null,
                active_runtime_count integer not null default 0,
                capabilities_json text not null,
                diagnostics text not null,
                created_at text not null,
                updated_at text not null
            )
            "#,
    )
    .execute(&pool)
    .await
    .expect("legacy nodes table creates");

    let error = match AppState::new(test_config(86_400), pool.clone()).await {
        Ok(_) => panic!("unversioned state must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("retained 0.1.x Core state"));
    let column_count: i64 = sqlx::query_scalar(
        "select count(*) from pragma_table_info('nodes') where name = 'credential_hash'",
    )
    .fetch_one(&pool)
    .await
    .expect("nodes columns load");

    assert_eq!(column_count, 0);
}

#[tokio::test]
async fn core_state_survives_sqlite_reopen() {
    let db_path = std::env::temp_dir().join(format!("uprava-core-{}.sqlite", Uuid::new_v4()));
    remove_sqlite_file_set(&db_path);
    let state = AppState::new(test_config(86_400), sqlite_file_pool(&db_path).await)
        .await
        .expect("file state migrates");
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let placement_id = detail.placement.project_placement_id.clone();
    let session_id = detail.session.session_thread_id.clone();
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
    state.pool.close().await;
    drop(state);

    let reopened = AppState::new(test_config(86_400), sqlite_file_pool(&db_path).await)
        .await
        .expect("file state remigrates");
    let inventory = load_inventory(&reopened)
        .await
        .expect("inventory loads after reopen");
    let node_persisted = inventory.nodes.iter().any(|node| node.node_id == node_id);
    let placement_persisted = inventory
        .placements
        .iter()
        .any(|placement| placement.project_placement_id == placement_id);
    let session_persisted = inventory
        .sessions
        .iter()
        .any(|session| session.session_thread_id == session_id);
    reopened.pool.close().await;
    drop(reopened);
    remove_sqlite_file_set(&db_path);

    assert!(node_persisted);
    assert!(placement_persisted);
    assert!(session_persisted);
}

#[tokio::test]
async fn session_history_and_read_models_survive_sqlite_reopen() {
    let db_path = std::env::temp_dir().join(format!("uprava-core-{}.sqlite", Uuid::new_v4()));
    remove_sqlite_file_set(&db_path);
    let state = AppState::new(test_config(86_400), sqlite_file_pool(&db_path).await)
        .await
        .expect("file state migrates");
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let session_id = detail.session.session_thread_id.clone();
    let event_id = EventId::from("persisted-provider-completed-1");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            event_id.as_str(),
            1,
            EventKind::ProviderMessageCompleted,
            json!({ "content": "persisted assistant" }),
        ),
    )
    .await
    .expect("provider event accepts");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
    state.pool.close().await;
    drop(state);

    let reopened = AppState::new(test_config(86_400), sqlite_file_pool(&db_path).await)
        .await
        .expect("file state remigrates");
    let detail = load_session_detail(&reopened, &session_id)
        .await
        .expect("session detail reloads after reopen");
    let evidence_projection = build_session_evidence_projection(&reopened, &session_id)
        .await
        .expect("evidence projection rebuilds after reopen");
    let projection = build_agent_projection(&reopened, &session_id)
        .await
        .expect("agent projection rebuilds after reopen");
    reopened.pool.close().await;
    drop(reopened);
    remove_sqlite_file_set(&db_path);

    let assistant_message = detail
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message.content == "persisted assistant"
                && message.source_event_id.as_ref() == Some(&event_id)
        })
        .expect("assistant message persisted");
    assert!(detail.events.iter().any(|event| event.event_id == event_id));
    assert!(evidence_projection
        .root
        .children
        .iter()
        .any(|node| matches!(
            &node.primary_ref,
            UpravaRef::Message { message_id } if message_id == &assistant_message.message_id
        )));
    assert!(evidence_projection
        .root
        .children
        .iter()
        .any(|node| matches!(
            &node.primary_ref,
            UpravaRef::Event {
                event_id: artifact_event_id,
                ..
            } if artifact_event_id == &event_id
        )));
    assert!(projection
        .recent_message_refs
        .iter()
        .any(|reference| matches!(
                reference,
                UpravaRef::Message { message_id } if message_id == &assistant_message.message_id
        )));
    assert!(projection
        .evidence_projection_summary
        .contains("1 messages, 1 events"));
}

#[tokio::test]
async fn trace_event_log_and_reference_resolution_preserve_raw_causality() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let cause_ref = UpravaRef::Command {
        command_id: CommandId::from("command-cause-1"),
    };
    for (seq, event_id, summary) in [
        (1, "activity-1", "read source file"),
        (2, "activity-2", "compare command output"),
    ] {
        let mut event = node_event_fixture(
            &detail,
            node_id.clone(),
            event_id,
            seq,
            EventKind::ProviderActivity,
            json!({ "summary": summary, "provider": "codex" }),
        );
        event.cause_refs = vec![cause_ref.clone()];
        accept_node_event(&state, event)
            .await
            .expect("provider activity accepts");
    }

    let trace = build_session_trace_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("trace builds");
    let activity = trace
        .steps
        .iter()
        .find(|step| step.title == "Provider activity")
        .expect("provider activity is grouped");
    assert_eq!(activity.precision, TracePrecision::Coarse);
    assert_eq!(activity.links.raw_refs.len(), 2);
    assert!(activity.links.cause_refs.contains(&cause_ref));

    let first_page = load_event_log_page(
        &state,
        EventLogQuery {
            session_thread_id: Some(detail.session.session_thread_id.to_string()),
            placement_id: None,
            kind: Some("provider.activity".to_owned()),
            cursor: None,
            limit: Some(1),
        },
    )
    .await
    .expect("first event page loads");
    assert_eq!(first_page.events.len(), 1);
    let cursor = first_page.next_cursor.expect("more events remain");
    let second_page = load_event_log_page(
        &state,
        EventLogQuery {
            session_thread_id: Some(detail.session.session_thread_id.to_string()),
            placement_id: None,
            kind: Some("provider.activity".to_owned()),
            cursor: Some(cursor),
            limit: Some(1),
        },
    )
    .await
    .expect("second event page loads");
    assert_eq!(second_page.events.len(), 1);
    assert_ne!(
        first_page.events[0].event_id,
        second_page.events[0].event_id
    );

    let resolution = resolve_reference(&state, event_ref(&first_page.events[0]))
        .await
        .expect("event reference resolves");
    assert_eq!(resolution.status, ReferenceResolutionStatus::Resolved);
    assert_eq!(resolution.links.cause_refs, vec![cause_ref]);
    assert!(resolution.raw_payload.is_some());

    let missing = resolve_reference(
        &state,
        UpravaRef::Event {
            event_id: EventId::from("missing-event"),
            scope_ref: Box::new(ScopeRef::Session {
                session_thread_id: detail.session.session_thread_id.clone(),
            }),
            seq: 999,
        },
    )
    .await
    .expect("missing references resolve to an explicit state");
    assert_eq!(missing.status, ReferenceResolutionStatus::Missing);
    assert_eq!(
        missing.unavailable_reason.as_deref(),
        Some("Event not found")
    );
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn deduction_rejects_foreign_scope_and_records_bounded_package() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;

    let invalid = create_deduction(
        &state,
        detail.session.session_thread_id.clone(),
        CreateDeductionRequest {
            scope_ref: UpravaRef::Message {
                message_id: MessageId::from("foreign-message"),
            },
            question: Some("Why?".to_owned()),
        },
        CorrelationId::from("deduction-invalid-scope"),
    )
    .await
    .expect_err("foreign scope is rejected");
    assert!(matches!(
        invalid,
        AppError::BadRequest {
            code: "deduction.scope_invalid",
            ..
        }
    ));

    let scope_ref = UpravaRef::Session {
        session_thread_id: detail.session.session_thread_id.clone(),
    };
    let accepted = create_deduction(
        &state,
        detail.session.session_thread_id.clone(),
        CreateDeductionRequest {
            scope_ref: scope_ref.clone(),
            question: None,
        },
        CorrelationId::from("deduction-valid"),
    )
    .await
    .expect("deduction records");
    let command_json: String =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(accepted.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("deduction command loads");
    let command: CommandEnvelope =
        serde_json::from_str(&command_json).expect("deduction command decodes");
    let CommandPayload::RequestDeduction { package } = command.payload else {
        panic!("deduction command carries an evidence package");
    };
    assert_eq!(package.scope_ref, scope_ref);
    assert!(!package.evidence_snapshot_hash.is_empty());
    assert!(package.allowed_refs.contains(&package.scope_ref));
    assert_eq!(package.session_thread_id, detail.session.session_thread_id);

    let record = load_deduction_record(&state, &accepted.deduction_id)
        .await
        .expect("deduction record loads");
    assert_eq!(record.state, DeductionState::Requested);

    let cancelled = cancel_deduction(
        &state,
        &accepted.deduction_id,
        CorrelationId::from("deduction-cancel"),
    )
    .await
    .expect("active deduction cancels");
    let cancelled_record = load_deduction_record(&state, &accepted.deduction_id)
        .await
        .expect("cancelled deduction reloads");
    assert_eq!(cancelled_record.state, DeductionState::Cancelled);
    let cancel_kind: String = sqlx::query_scalar("select kind from commands where command_id = ?1")
        .bind(cancelled.command_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("cancel command loads");
    assert_eq!(cancel_kind, "CancelDeduction");
    std::fs::remove_dir_all(workspace_path).expect("workspace dir removes");
}
