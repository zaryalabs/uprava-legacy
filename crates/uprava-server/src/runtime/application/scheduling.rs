//! Scheduled-message, provider-quota and background-job orchestration.

use super::super::*;

pub(crate) async fn create_scheduled_message_route(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
    Json(request): Json<CreateScheduledMessageRequest>,
) -> Result<Json<ScheduledSessionMessage>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    load_session_detail(&state, &session_id).await?;
    validate_scheduled_message_input(&request.content, request.due_at, &request.timezone)?;
    let now = Utc::now();
    let scheduled_message_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        insert into scheduled_messages (
            scheduled_message_id, session_thread_id, content, due_at, timezone, state,
            created_at, updated_at, sending_at, sent_at, cancelled_at, command_id, turn_id,
            failure_code, failure_message
        ) values (?1, ?2, ?3, ?4, ?5, 'scheduled', ?6, ?6, null, null, null, null, null, null, null)
        "#,
    )
    .bind(&scheduled_message_id)
    .bind(session_id.as_str())
    .bind(request.content.trim())
    .bind(request.due_at)
    .bind(request.timezone.trim())
    .bind(now)
    .execute(&state.pool)
    .await?;
    load_scheduled_message(&state, &session_id, &scheduled_message_id)
        .await
        .map(Json)
}

pub(crate) async fn update_scheduled_message_route(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, scheduled_message_id)): Path<(String, String)>,
    Json(request): Json<UpdateScheduledMessageRequest>,
) -> Result<Json<ScheduledSessionMessage>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    let existing = load_scheduled_message(&state, &session_id, &scheduled_message_id).await?;
    if !matches!(
        existing.state,
        ScheduledMessageState::Scheduled | ScheduledMessageState::Failed
    ) {
        return Err(AppError::bad_request(
            "scheduled_message.not_editable",
            "Only scheduled or failed messages can be edited or rescheduled",
        ));
    }
    let content = request.content.unwrap_or(existing.content);
    let due_at = request.due_at.unwrap_or(existing.due_at);
    let timezone = request.timezone.unwrap_or(existing.timezone);
    validate_scheduled_message_input(&content, due_at, &timezone)?;
    let now = Utc::now();
    sqlx::query(
        r#"
        update scheduled_messages
        set content = ?1, due_at = ?2, timezone = ?3, state = 'scheduled', updated_at = ?4,
            sending_at = null, sent_at = null, cancelled_at = null, command_id = null, turn_id = null,
            failure_code = null, failure_message = null
        where scheduled_message_id = ?5 and session_thread_id = ?6
          and state in ('scheduled', 'failed')
        "#,
    )
    .bind(content.trim())
    .bind(due_at)
    .bind(timezone.trim())
    .bind(now)
    .bind(&scheduled_message_id)
    .bind(session_id.as_str())
    .execute(&state.pool)
    .await?;
    load_scheduled_message(&state, &session_id, &scheduled_message_id)
        .await
        .map(Json)
}

pub(crate) async fn cancel_scheduled_message_route(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, scheduled_message_id)): Path<(String, String)>,
) -> Result<Json<ScheduledSessionMessage>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    let now = Utc::now();
    let result = sqlx::query(
        "update scheduled_messages set state = 'cancelled', cancelled_at = ?1, updated_at = ?1 where scheduled_message_id = ?2 and session_thread_id = ?3 and state = 'scheduled'",
    )
    .bind(now)
    .bind(&scheduled_message_id)
    .bind(session_id.as_str())
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "scheduled_message.not_cancellable",
            "Only scheduled messages can be cancelled",
        ));
    }
    load_scheduled_message(&state, &session_id, &scheduled_message_id)
        .await
        .map(Json)
}

pub(crate) async fn send_scheduled_message_now_route(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, scheduled_message_id)): Path<(String, String)>,
) -> Result<Json<ScheduledSessionMessage>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    claim_scheduled_message(&state, &session_id, &scheduled_message_id, "scheduled").await?;
    dispatch_scheduled_message(&state, &session_id, &scheduled_message_id).await?;
    load_scheduled_message(&state, &session_id, &scheduled_message_id)
        .await
        .map(Json)
}

pub(crate) async fn retry_scheduled_message_route(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, scheduled_message_id)): Path<(String, String)>,
) -> Result<Json<ScheduledSessionMessage>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    claim_scheduled_message(&state, &session_id, &scheduled_message_id, "failed").await?;
    dispatch_scheduled_message(&state, &session_id, &scheduled_message_id).await?;
    load_scheduled_message(&state, &session_id, &scheduled_message_id)
        .await
        .map(Json)
}

pub(crate) fn validate_scheduled_message_input(
    content: &str,
    due_at: DateTime<Utc>,
    timezone: &str,
) -> Result<(), AppError> {
    if content.trim().is_empty() {
        return Err(AppError::bad_request(
            "validation.empty_scheduled_message",
            "Scheduled message content cannot be empty",
        ));
    }
    if content.chars().count() > MAX_SCHEDULED_MESSAGE_CONTENT_CHARS {
        return Err(AppError::bad_request(
            "validation.scheduled_message_too_large",
            "Scheduled message content exceeds the allowed size",
        ));
    }
    if due_at <= Utc::now() {
        return Err(AppError::bad_request(
            "validation.scheduled_message_not_future",
            "Scheduled message time must be in the future",
        ));
    }
    let timezone = timezone.trim();
    if timezone.is_empty()
        || timezone.chars().count() > MAX_SCHEDULED_MESSAGE_TIMEZONE_CHARS
        || !timezone.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '-' | '+')
        })
    {
        return Err(AppError::bad_request(
            "validation.scheduled_message_timezone",
            "Scheduled message timezone must be a valid explicit timezone name",
        ));
    }
    Ok(())
}

pub(crate) fn spawn_scheduled_message_dispatcher(state: std::sync::Weak<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SCHEDULED_MESSAGE_TICK).await;
            let Some(state) = state.upgrade() else {
                break;
            };
            if let Err(error) = dispatch_due_scheduled_messages(&state).await {
                tracing::error!(error = %error, "scheduled message dispatcher tick failed");
            }
        }
    });
}

pub(crate) async fn recover_interrupted_scheduled_messages(
    state: &AppState,
) -> Result<(), AppError> {
    let now = Utc::now();
    sqlx::query(
        "update scheduled_messages set state = 'failed', updated_at = ?1, failure_code = 'scheduled_message.dispatch_interrupted', failure_message = 'Core restarted while this delayed message was being dispatched; retry or reschedule it explicitly' where state = 'sending'",
    )
    .bind(now)
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn dispatch_due_scheduled_messages(state: &AppState) -> Result<(), AppError> {
    let due_ids: Vec<(String, String)> = sqlx::query_as(
        "select scheduled_message_id, session_thread_id from scheduled_messages where state = 'scheduled' and due_at <= ?1 order by due_at asc, scheduled_message_id asc",
    )
    .bind(Utc::now())
    .fetch_all(&state.pool)
    .await?;
    for (scheduled_message_id, session_thread_id) in due_ids {
        let session_id = SessionThreadId::from(session_thread_id);
        if claim_scheduled_message(state, &session_id, &scheduled_message_id, "scheduled")
            .await
            .is_ok()
        {
            dispatch_scheduled_message(state, &session_id, &scheduled_message_id).await?;
        }
    }
    Ok(())
}

pub(crate) async fn claim_scheduled_message(
    state: &AppState,
    session_id: &SessionThreadId,
    scheduled_message_id: &str,
    expected_state: &str,
) -> Result<(), AppError> {
    let now = Utc::now();
    let result = sqlx::query(
        "update scheduled_messages set state = 'sending', sending_at = ?1, updated_at = ?1, failure_code = null, failure_message = null where scheduled_message_id = ?2 and session_thread_id = ?3 and state = ?4",
    )
    .bind(now)
    .bind(scheduled_message_id)
    .bind(session_id.as_str())
    .bind(expected_state)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 1 {
        return Ok(());
    }
    Err(AppError::bad_request(
        "scheduled_message.not_sendable",
        "Scheduled message is no longer available to send",
    ))
}

pub(crate) async fn dispatch_scheduled_message(
    state: &AppState,
    session_id: &SessionThreadId,
    scheduled_message_id: &str,
) -> Result<(), AppError> {
    let scheduled = load_scheduled_message(state, session_id, scheduled_message_id).await?;
    if scheduled.state != ScheduledMessageState::Sending {
        return Ok(());
    }
    match submit_turn_with_correlation(
        state,
        session_id.clone(),
        scheduled.content,
        None,
        CorrelationId::new(),
    )
    .await
    {
        Ok(accepted) => {
            let turn_id: Option<String> =
                sqlx::query_scalar("select turn_id from turns where command_id = ?1")
                    .bind(accepted.command_id.as_str())
                    .fetch_optional(&state.pool)
                    .await?;
            let now = Utc::now();
            sqlx::query(
                "update scheduled_messages set state = 'sent', sent_at = ?1, updated_at = ?1, command_id = ?2, turn_id = ?3 where scheduled_message_id = ?4 and session_thread_id = ?5 and state = 'sending'",
            )
            .bind(now)
            .bind(accepted.command_id.as_str())
            .bind(turn_id)
            .bind(scheduled_message_id)
            .bind(session_id.as_str())
            .execute(&state.pool)
            .await?;
        }
        Err(error) => {
            let (code, message) = scheduled_message_failure(&error);
            let now = Utc::now();
            sqlx::query(
                "update scheduled_messages set state = 'failed', updated_at = ?1, failure_code = ?2, failure_message = ?3 where scheduled_message_id = ?4 and session_thread_id = ?5 and state = 'sending'",
            )
            .bind(now)
            .bind(code)
            .bind(message)
            .bind(scheduled_message_id)
            .bind(session_id.as_str())
            .execute(&state.pool)
            .await?;
        }
    }
    Ok(())
}

pub(crate) fn scheduled_message_failure(error: &AppError) -> (String, String) {
    match error {
        AppError::NotFound { code, message }
        | AppError::BadRequest { code, message }
        | AppError::Conflict { code, message }
        | AppError::Auth { code, message }
        | AppError::RateLimited { code, message }
        | AppError::Internal { code, message } => ((*code).to_owned(), message.clone()),
        AppError::Database(_) => (
            "internal.database".to_owned(),
            "Core database operation failed".to_owned(),
        ),
        AppError::Serialization(_) => (
            "internal.serialization".to_owned(),
            "Core serialization failed".to_owned(),
        ),
        AppError::Io(_) => (
            "internal.io".to_owned(),
            "Core IO operation failed".to_owned(),
        ),
        AppError::TaskJoin(_) => (
            "internal.task_join".to_owned(),
            "Core background task failed".to_owned(),
        ),
    }
}

pub(crate) async fn load_scheduled_message(
    state: &AppState,
    session_id: &SessionThreadId,
    scheduled_message_id: &str,
) -> Result<ScheduledSessionMessage, AppError> {
    let row = sqlx::query(
        r#"
        select scheduled_message_id, session_thread_id, content, due_at, timezone, state,
               created_at, updated_at, sending_at, sent_at, cancelled_at, command_id, turn_id,
               failure_code, failure_message
        from scheduled_messages
        where scheduled_message_id = ?1 and session_thread_id = ?2
        "#,
    )
    .bind(scheduled_message_id)
    .bind(session_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found("scheduled_message.not_found", "Scheduled message not found")
    })?;
    row_to_scheduled_message(row)
}

pub(crate) async fn load_scheduled_messages(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<Vec<ScheduledSessionMessage>, AppError> {
    let rows = sqlx::query(
        r#"
        select scheduled_message_id, session_thread_id, content, due_at, timezone, state,
               created_at, updated_at, sending_at, sent_at, cancelled_at, command_id, turn_id,
               failure_code, failure_message
        from scheduled_messages
        where session_thread_id = ?1
        order by due_at desc, created_at desc
        "#,
    )
    .bind(session_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter().map(row_to_scheduled_message).collect()
}

pub(crate) fn row_to_scheduled_message(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ScheduledSessionMessage, AppError> {
    let failure_code: Option<String> = row.try_get("failure_code")?;
    let failure_message: Option<String> = row.try_get("failure_message")?;
    Ok(ScheduledSessionMessage {
        scheduled_message_id: row.try_get("scheduled_message_id")?,
        session_thread_id: SessionThreadId::from(row.try_get::<String, _>("session_thread_id")?),
        content: row.try_get("content")?,
        due_at: row.try_get("due_at")?,
        timezone: row.try_get("timezone")?,
        state: parse_scheduled_message_state(row.try_get::<String, _>("state")?.as_str()),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        sending_at: row.try_get("sending_at")?,
        sent_at: row.try_get("sent_at")?,
        cancelled_at: row.try_get("cancelled_at")?,
        command_id: row
            .try_get::<Option<String>, _>("command_id")?
            .map(CommandId::from),
        turn_id: row
            .try_get::<Option<String>, _>("turn_id")?
            .map(TurnId::from),
        failure: failure_code
            .zip(failure_message)
            .map(|(code, message)| ScheduledMessageFailure { code, message }),
    })
}

pub(crate) async fn provider_quota_route(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
) -> Result<Json<ProviderQuotaStatus>, AppError> {
    provider_quota_status(&state, provider.trim())
        .await
        .map(Json)
}

pub(crate) async fn provider_quota_status(
    state: &AppState,
    provider: &str,
) -> Result<ProviderQuotaStatus, AppError> {
    if provider.is_empty() {
        return Err(AppError::bad_request(
            "validation.provider_required",
            "Provider is required",
        ));
    }
    let snapshot: Option<(Option<i64>, Option<i64>, DateTime<Utc>)> = sqlx::query_as(
        "select five_hour_remaining_percent, weekly_remaining_percent, observed_at from provider_quota_snapshots where provider = ?1 and reliable = 1",
    )
    .bind(provider)
    .fetch_optional(&state.pool)
    .await?;
    let Some((five_hour, weekly, observed_at)) = snapshot else {
        return Ok(ProviderQuotaStatus {
            provider: provider.to_owned(),
            state: ProviderQuotaState::Unknown,
            five_hour_remaining_percent: None,
            weekly_remaining_percent: None,
            observed_at: None,
            unavailable_reason: Some("provider_has_no_machine_readable_quota_source".to_owned()),
        });
    };
    let snapshot_age = Utc::now().signed_duration_since(observed_at).num_seconds();
    if !(0..=PROVIDER_QUOTA_FRESH_SECONDS).contains(&snapshot_age) {
        return Ok(ProviderQuotaStatus {
            provider: provider.to_owned(),
            state: ProviderQuotaState::Unknown,
            five_hour_remaining_percent: five_hour.and_then(|value| u8::try_from(value).ok()),
            weekly_remaining_percent: weekly.and_then(|value| u8::try_from(value).ok()),
            observed_at: Some(observed_at),
            unavailable_reason: Some("provider_quota_snapshot_stale".to_owned()),
        });
    }
    let five_hour = five_hour.and_then(|value| u8::try_from(value).ok());
    let weekly = weekly.and_then(|value| u8::try_from(value).ok());
    let limited = five_hour.is_some_and(|value| value <= PROVIDER_QUOTA_BLOCK_PERCENT)
        || weekly.is_some_and(|value| value <= PROVIDER_QUOTA_BLOCK_PERCENT);
    Ok(ProviderQuotaStatus {
        provider: provider.to_owned(),
        state: if limited {
            ProviderQuotaState::Limited
        } else {
            ProviderQuotaState::Available
        },
        five_hour_remaining_percent: five_hour,
        weekly_remaining_percent: weekly,
        observed_at: Some(observed_at),
        unavailable_reason: None,
    })
}

pub(crate) async fn ensure_provider_quota_admission(
    state: &AppState,
    provider: &str,
    force: bool,
    operation: &str,
) -> Result<(), AppError> {
    let status = provider_quota_status(state, provider).await?;
    if status.state != ProviderQuotaState::Limited {
        return Ok(());
    }
    if !force {
        return Err(AppError::bad_request(
            "provider.quota_limited",
            "Provider quota has 5% or less remaining; explicitly force the start to continue",
        ));
    }
    sqlx::query(
        "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, 'provider.quota_force_override', null, ?2, 'allowed', ?3, ?4)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(operation)
    .bind(serde_json::to_string(&status)?)
    .bind(Utc::now())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) fn validate_job_input(
    name: &str,
    prompt: &str,
    provider: &str,
    schedule: Option<&JobSchedule>,
    timezone: &str,
) -> Result<(), AppError> {
    if name.trim().is_empty() || name.chars().count() > MAX_JOB_NAME_CHARS {
        return Err(AppError::bad_request(
            "validation.job_name",
            "Job name is required and must fit the allowed size",
        ));
    }
    if prompt.trim().is_empty() || prompt.chars().count() > MAX_JOB_PROMPT_CHARS {
        return Err(AppError::bad_request(
            "validation.job_prompt",
            "Job prompt is required and must fit the allowed size",
        ));
    }
    if provider.trim().is_empty() {
        return Err(AppError::bad_request(
            "validation.provider_required",
            "Provider is required",
        ));
    }
    validate_job_timezone(timezone)?;
    if let Some(schedule) = schedule {
        match schedule {
            JobSchedule::Interval { minutes } if *minutes == 0 => {
                return Err(AppError::bad_request(
                    "validation.job_interval",
                    "Job interval must be at least one minute",
                ));
            }
            JobSchedule::Daily { hour, minute } if *hour > 23 || *minute > 59 => {
                return Err(AppError::bad_request(
                    "validation.job_schedule_time",
                    "Daily Job time is outside the valid clock range",
                ));
            }
            JobSchedule::Weekly {
                weekday,
                hour,
                minute,
            } if !(1..=7).contains(weekday) || *hour > 23 || *minute > 59 => {
                return Err(AppError::bad_request(
                    "validation.job_schedule_time",
                    "Weekly Job weekday or time is invalid",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn validate_job_timezone(timezone: &str) -> Result<(), AppError> {
    let timezone = timezone.trim();
    if timezone.is_empty() || timezone.chars().count() > MAX_JOB_TIMEZONE_CHARS {
        return Err(AppError::bad_request(
            "validation.job_timezone",
            "Job timezone must be an explicit IANA timezone",
        ));
    }
    jiff::tz::db().get(timezone).map_err(|_| {
        AppError::bad_request(
            "validation.job_timezone",
            "Job timezone must be an explicit IANA timezone",
        )
    })?;
    Ok(())
}

pub(crate) fn next_job_run_at(
    schedule: &JobSchedule,
    timezone: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, AppError> {
    if let JobSchedule::Interval { minutes } = schedule {
        return after
            .checked_add_signed(ChronoDuration::minutes(i64::from(*minutes)))
            .ok_or_else(|| {
                AppError::bad_request("job.schedule_overflow", "Job schedule overflow")
            });
    }
    let timestamp = Timestamp::new(
        after.timestamp(),
        i32::try_from(after.nanosecond()).unwrap_or_default(),
    )
    .map_err(|_| AppError::bad_request("job.schedule_time", "Job schedule time is invalid"))?;
    let zoned = timestamp.in_tz(timezone).map_err(|_| {
        AppError::bad_request("job.schedule_timezone", "Job timezone is unavailable")
    })?;
    let (hour, minute, wanted_weekday) = match schedule {
        JobSchedule::Daily { hour, minute } => (*hour, *minute, None),
        JobSchedule::Weekly {
            weekday,
            hour,
            minute,
        } => (
            *hour,
            *minute,
            Some(
                Weekday::from_monday_one_offset(i8::try_from(*weekday).unwrap_or_default())
                    .map_err(|_| {
                        AppError::bad_request("job.schedule_weekday", "Job weekday is invalid")
                    })?,
            ),
        ),
        JobSchedule::Interval { .. } => unreachable!(),
    };
    let mut date = zoned.date();
    for _ in 0..=7 {
        if wanted_weekday.is_none_or(|weekday| date.weekday() == weekday) {
            let candidate = date
                .at(
                    i8::try_from(hour).unwrap_or_default(),
                    i8::try_from(minute).unwrap_or_default(),
                    0,
                    0,
                )
                .in_tz(timezone)
                .map_err(|_| {
                    AppError::bad_request("job.schedule_time", "Job schedule time is invalid")
                })?;
            if candidate.timestamp() > timestamp {
                return DateTime::from_timestamp(
                    candidate.timestamp().as_second(),
                    u32::try_from(candidate.timestamp().subsec_nanosecond()).unwrap_or_default(),
                )
                .ok_or_else(|| {
                    AppError::bad_request("job.schedule_time", "Job schedule time is invalid")
                });
            }
        }
        date = date
            .tomorrow()
            .map_err(|_| AppError::bad_request("job.schedule_overflow", "Job schedule overflow"))?;
    }
    Err(AppError::bad_request(
        "job.schedule_unresolvable",
        "Job schedule has no next occurrence",
    ))
}

pub(crate) async fn list_jobs_route(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<JobSummary>>, AppError> {
    let ids: Vec<String> = sqlx::query_scalar("select job_id from jobs order by updated_at desc")
        .fetch_all(&state.pool)
        .await?;
    let mut jobs = Vec::with_capacity(ids.len());
    for id in ids {
        jobs.push(load_job_detail(&state, &JobId::from(id)).await?.job);
    }
    Ok(Json(jobs))
}

pub(crate) async fn create_job_route(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateJobRequest>,
) -> Result<Json<JobDetail>, AppError> {
    load_placement(&state, &request.project_placement_id).await?;
    validate_job_input(
        &request.name,
        &request.prompt,
        &request.provider,
        request.schedule.as_ref(),
        &request.timezone,
    )?;
    let job_id = JobId::new();
    let now = Utc::now();
    sqlx::query(
        r#"
        insert into jobs (
            job_id, project_placement_id, name, prompt, provider, schedule_json,
            timezone, enabled, overlap_policy, continue_after_error, next_run_at,
            paused_reason, created_at, updated_at
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 'skip', ?8, null, 'created_paused', ?9, ?9)
        "#,
    )
    .bind(job_id.as_str())
    .bind(request.project_placement_id.as_str())
    .bind(request.name.trim())
    .bind(request.prompt.trim())
    .bind(request.provider.trim())
    .bind(
        request
            .schedule
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(request.timezone.trim())
    .bind(request.continue_after_error)
    .bind(now)
    .execute(&state.pool)
    .await?;
    load_job_detail(&state, &job_id).await.map(Json)
}

pub(crate) async fn job_detail_route(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobDetail>, AppError> {
    load_job_detail(&state, &JobId::from(job_id))
        .await
        .map(Json)
}

pub(crate) async fn update_job_route(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(request): Json<UpdateJobRequest>,
) -> Result<Json<JobDetail>, AppError> {
    let job_id = JobId::from(job_id);
    let existing = load_job_detail(&state, &job_id).await?;
    let name = request.name.unwrap_or(existing.job.name);
    let prompt = request.prompt.unwrap_or(existing.prompt);
    let provider = request.provider.unwrap_or(existing.job.provider);
    let schedule = if request.clear_schedule {
        None
    } else {
        request.schedule.or(existing.job.schedule)
    };
    let timezone = request.timezone.unwrap_or(existing.job.timezone);
    let continue_after_error = request
        .continue_after_error
        .unwrap_or(existing.job.continue_after_error);
    validate_job_input(&name, &prompt, &provider, schedule.as_ref(), &timezone)?;
    let enabled = existing.job.enabled && schedule.is_some();
    let next_run_at = if enabled {
        schedule
            .as_ref()
            .map(|schedule| next_job_run_at(schedule, &timezone, Utc::now()))
            .transpose()?
    } else {
        None
    };
    let paused_reason = (!enabled && existing.job.enabled).then_some("schedule_removed");
    sqlx::query(
        "update jobs set name = ?1, prompt = ?2, provider = ?3, schedule_json = ?4, timezone = ?5, continue_after_error = ?6, enabled = ?7, next_run_at = ?8, paused_reason = coalesce(?9, paused_reason), updated_at = ?10 where job_id = ?11",
    )
    .bind(name.trim())
    .bind(prompt.trim())
    .bind(provider.trim())
    .bind(schedule.as_ref().map(serde_json::to_string).transpose()?)
    .bind(timezone.trim())
    .bind(continue_after_error)
    .bind(enabled)
    .bind(next_run_at)
    .bind(paused_reason)
    .bind(Utc::now())
    .bind(job_id.as_str())
    .execute(&state.pool)
    .await?;
    load_job_detail(&state, &job_id).await.map(Json)
}

pub(crate) async fn enable_job_route(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobDetail>, AppError> {
    let job_id = JobId::from(job_id);
    let job = load_job_detail(&state, &job_id).await?;
    let schedule = job.job.schedule.as_ref().ok_or_else(|| {
        AppError::bad_request(
            "job.schedule_required",
            "A schedule is required before enabling a Job",
        )
    })?;
    let next_run_at = next_job_run_at(schedule, &job.job.timezone, Utc::now())?;
    sqlx::query(
        "update jobs set enabled = 1, next_run_at = ?1, paused_reason = null, updated_at = ?2 where job_id = ?3",
    )
    .bind(next_run_at)
    .bind(Utc::now())
    .bind(job_id.as_str())
    .execute(&state.pool)
    .await?;
    load_job_detail(&state, &job_id).await.map(Json)
}

pub(crate) async fn disable_job_route(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobDetail>, AppError> {
    let job_id = JobId::from(job_id);
    let result = sqlx::query(
        "update jobs set enabled = 0, next_run_at = null, paused_reason = 'paused_by_user', updated_at = ?1 where job_id = ?2",
    )
    .bind(Utc::now())
    .bind(job_id.as_str())
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("job.not_found", "Job not found"));
    }
    load_job_detail(&state, &job_id).await.map(Json)
}

pub(crate) async fn run_job_route(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    request: Option<Json<RunJobRequest>>,
) -> Result<Json<JobRunSummary>, AppError> {
    let job_id = JobId::from(job_id);
    let job = load_job_detail(&state, &job_id).await?;
    let force = request
        .map(|Json(request)| request.force)
        .unwrap_or_default();
    let run = insert_job_run(&state, &job, JobRunTrigger::Manual, None, force).await?;
    drive_job_runs(&state).await?;
    load_job_run(&state, &run.job_run_id).await.map(Json)
}

pub(crate) async fn job_run_detail_route(
    State(state): State<Arc<AppState>>,
    Path(job_run_id): Path<String>,
) -> Result<Json<JobRunSummary>, AppError> {
    load_job_run(&state, &JobRunId::from(job_run_id))
        .await
        .map(Json)
}

pub(crate) async fn cancel_job_run_route(
    State(state): State<Arc<AppState>>,
    Path(job_run_id): Path<String>,
) -> Result<Json<JobRunSummary>, AppError> {
    let job_run_id = JobRunId::from(job_run_id);
    let run = load_job_run(&state, &job_run_id).await?;
    let result = sqlx::query(
        "update job_runs set state = 'cancelled', finished_at = ?1, terminal_code = 'job.cancelled_by_user', terminal_message = 'Job Run was cancelled by the user' where job_run_id = ?2 and state in ('queued', 'starting', 'running')",
    )
    .bind(Utc::now())
    .bind(job_run_id.as_str())
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::bad_request(
            "job_run.not_cancellable",
            "Only an active Job Run can be cancelled",
        ));
    }
    if let Some(runtime_session_id) = run.runtime_session_id {
        let _ = lifecycle_command(
            &state,
            runtime_session_id,
            CommandKind::StopRuntime,
            CorrelationId::new(),
        )
        .await;
    }
    load_job_run(&state, &job_run_id).await.map(Json)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JobRunConfigSnapshot {
    pub(crate) name: String,
    pub(crate) project_placement_id: ProjectPlacementId,
    pub(crate) prompt: String,
    pub(crate) provider: String,
    pub(crate) schedule: Option<JobSchedule>,
    pub(crate) timezone: String,
    pub(crate) overlap_policy: JobOverlapPolicy,
    pub(crate) continue_after_error: bool,
}

pub(crate) fn snapshot_for_job(job: &JobDetail) -> JobRunConfigSnapshot {
    JobRunConfigSnapshot {
        name: job.job.name.clone(),
        project_placement_id: job.job.project_placement_id.clone(),
        prompt: job.prompt.clone(),
        provider: job.job.provider.clone(),
        schedule: job.job.schedule.clone(),
        timezone: job.job.timezone.clone(),
        overlap_policy: job.job.overlap_policy,
        continue_after_error: job.job.continue_after_error,
    }
}

pub(crate) async fn load_job_detail(
    state: &AppState,
    job_id: &JobId,
) -> Result<JobDetail, AppError> {
    let row = sqlx::query(
        r#"
        select jobs.job_id, jobs.project_placement_id, jobs.name, jobs.prompt,
               jobs.provider, jobs.schedule_json, jobs.timezone, jobs.enabled,
               jobs.overlap_policy, jobs.continue_after_error, jobs.next_run_at,
               jobs.paused_reason, jobs.created_at, jobs.updated_at,
               project_placements.display_name as placement_name
        from jobs
        join project_placements on project_placements.project_placement_id = jobs.project_placement_id
        where jobs.job_id = ?1
        "#,
    )
    .bind(job_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("job.not_found", "Job not found"))?;
    let schedule_json: Option<String> = row.try_get("schedule_json")?;
    let schedule = schedule_json
        .map(|value| serde_json::from_str::<JobSchedule>(&value))
        .transpose()?;
    let runs = load_job_runs(state, job_id).await?;
    let summary = JobSummary {
        job_id: JobId::from(row.try_get::<String, _>("job_id")?),
        name: row.try_get("name")?,
        project_placement_id: ProjectPlacementId::from(
            row.try_get::<String, _>("project_placement_id")?,
        ),
        placement_name: row.try_get("placement_name")?,
        provider: row.try_get("provider")?,
        enabled: row.try_get("enabled")?,
        schedule,
        timezone: row.try_get("timezone")?,
        overlap_policy: JobOverlapPolicy::Skip,
        continue_after_error: row.try_get("continue_after_error")?,
        next_run_at: row.try_get("next_run_at")?,
        paused_reason: row.try_get("paused_reason")?,
        latest_run: runs.first().cloned(),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    };
    Ok(JobDetail {
        job: summary,
        prompt: row.try_get("prompt")?,
        runs,
    })
}

pub(crate) async fn load_job_runs(
    state: &AppState,
    job_id: &JobId,
) -> Result<Vec<JobRunSummary>, AppError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "select job_run_id from job_runs where job_id = ?1 order by queued_at desc, job_run_id desc",
    )
    .bind(job_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    let mut runs = Vec::with_capacity(ids.len());
    for id in ids {
        runs.push(load_job_run(state, &JobRunId::from(id)).await?);
    }
    Ok(runs)
}

pub(crate) async fn load_job_run(
    state: &AppState,
    job_run_id: &JobRunId,
) -> Result<JobRunSummary, AppError> {
    let row = sqlx::query(
        r#"
        select job_run_id, job_id, trigger_kind, state, scheduled_for, queued_at,
               started_at, finished_at, session_thread_id, runtime_session_id,
               summary, terminal_code, terminal_message, config_snapshot_json, force
        from job_runs where job_run_id = ?1
        "#,
    )
    .bind(job_run_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("job_run.not_found", "Job Run not found"))?;
    let terminal_code: Option<String> = row.try_get("terminal_code")?;
    let terminal_message: Option<String> = row.try_get("terminal_message")?;
    Ok(JobRunSummary {
        job_run_id: JobRunId::from(row.try_get::<String, _>("job_run_id")?),
        job_id: JobId::from(row.try_get::<String, _>("job_id")?),
        trigger: match row.try_get::<String, _>("trigger_kind")?.as_str() {
            "scheduled" => JobRunTrigger::Scheduled,
            _ => JobRunTrigger::Manual,
        },
        state: parse_job_run_state(&row.try_get::<String, _>("state")?),
        scheduled_for: row.try_get("scheduled_for")?,
        queued_at: row.try_get("queued_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        session_thread_id: row
            .try_get::<Option<String>, _>("session_thread_id")?
            .map(SessionThreadId::from),
        runtime_session_id: row
            .try_get::<Option<String>, _>("runtime_session_id")?
            .map(RuntimeSessionId::from),
        summary: row.try_get("summary")?,
        terminal_reason: terminal_code
            .zip(terminal_message)
            .map(|(code, message)| ScheduledMessageFailure { code, message }),
        config_snapshot: JsonValue(serde_json::from_str(
            &row.try_get::<String, _>("config_snapshot_json")?,
        )?),
        force: row.try_get("force")?,
    })
}

pub(crate) fn parse_job_run_state(value: &str) -> JobRunState {
    match value {
        "queued" => JobRunState::Queued,
        "starting" => JobRunState::Starting,
        "running" => JobRunState::Running,
        "succeeded" => JobRunState::Succeeded,
        "failed" => JobRunState::Failed,
        "cancelled" => JobRunState::Cancelled,
        "timed_out" => JobRunState::TimedOut,
        "skipped" => JobRunState::Skipped,
        _ => JobRunState::Failed,
    }
}

pub(crate) async fn insert_job_run(
    state: &AppState,
    job: &JobDetail,
    trigger: JobRunTrigger,
    scheduled_for: Option<DateTime<Utc>>,
    force: bool,
) -> Result<JobRunSummary, AppError> {
    let job_run_id = JobRunId::new();
    let now = Utc::now();
    let mut connection = state.pool.acquire().await?;
    sqlx::query("begin immediate")
        .execute(&mut *connection)
        .await?;
    let active_count: i64 = sqlx::query_scalar(
        "select count(*) from job_runs where job_id = ?1 and state in ('queued', 'starting', 'running')",
    )
    .bind(job.job.job_id.as_str())
    .fetch_one(&mut *connection)
    .await?;
    let (run_state, terminal_code, terminal_message) = if active_count > 0 {
        (
            "skipped",
            Some("job.overlap_skipped"),
            Some("Another run of this Job is already active"),
        )
    } else {
        ("queued", None, None)
    };
    let insert_result = sqlx::query(
        r#"
        insert into job_runs (
            job_run_id, job_id, trigger_kind, state, scheduled_for, queued_at,
            started_at, finished_at, session_thread_id, runtime_session_id,
            command_id, turn_id, summary, terminal_code, terminal_message,
            config_snapshot_json, force
        ) values (?1, ?2, ?3, ?4, ?5, ?6, null, ?7, null, null, null, null, null, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(job_run_id.as_str())
    .bind(job.job.job_id.as_str())
    .bind(match trigger {
        JobRunTrigger::Manual => "manual",
        JobRunTrigger::Scheduled => "scheduled",
    })
    .bind(run_state)
    .bind(scheduled_for)
    .bind(now)
    .bind((run_state == "skipped").then_some(now))
    .bind(terminal_code)
    .bind(terminal_message)
    .bind(serde_json::to_string(&snapshot_for_job(job))?)
    .bind(force)
    .execute(&mut *connection)
    .await;
    match insert_result {
        Ok(_) => {
            sqlx::query("commit").execute(&mut *connection).await?;
        }
        Err(error) => {
            let _ = sqlx::query("rollback").execute(&mut *connection).await;
            return Err(error.into());
        }
    }
    drop(connection);
    load_job_run(state, &job_run_id).await
}

pub(crate) fn spawn_job_scheduler(state: std::sync::Weak<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(JOB_SCHEDULER_TICK).await;
            let Some(state) = state.upgrade() else {
                break;
            };
            if let Err(error) = dispatch_due_jobs(&state).await {
                tracing::error!(error = %error, "Job scheduler tick failed");
            }
            if let Err(error) = drive_job_runs(&state).await {
                tracing::error!(error = %error, "Job Run driver tick failed");
            }
        }
    });
}

pub(crate) async fn recover_job_runs(state: &AppState) -> Result<(), AppError> {
    sqlx::query(
        "update job_runs set state = 'queued', started_at = null where state = 'starting' and session_thread_id is null",
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn dispatch_due_jobs(state: &AppState) -> Result<(), AppError> {
    let due_job_ids: Vec<String> = sqlx::query_scalar(
        "select job_id from jobs where enabled = 1 and next_run_at is not null and next_run_at <= ?1 order by next_run_at, job_id",
    )
    .bind(Utc::now())
    .fetch_all(&state.pool)
    .await?;
    for job_id in due_job_ids {
        claim_due_job(state, &JobId::from(job_id)).await?;
    }
    Ok(())
}

pub(crate) async fn claim_due_job(state: &AppState, job_id: &JobId) -> Result<(), AppError> {
    let job = load_job_detail(state, job_id).await?;
    if !job.job.enabled {
        return Ok(());
    }
    let Some(due_at) = job.job.next_run_at else {
        return Ok(());
    };
    if due_at > Utc::now() {
        return Ok(());
    }
    let Some(schedule) = job.job.schedule.as_ref() else {
        return Ok(());
    };
    let next_run_at = next_job_run_at(schedule, &job.job.timezone, Utc::now())?;
    let now = Utc::now();
    let job_run_id = JobRunId::new();
    let mut connection = state.pool.acquire().await?;
    sqlx::query("begin immediate")
        .execute(&mut *connection)
        .await?;
    let result = async {
        let active_count: i64 = sqlx::query_scalar(
            "select count(*) from job_runs where job_id = ?1 and state in ('queued', 'starting', 'running')",
        )
        .bind(job_id.as_str())
        .fetch_one(&mut *connection)
        .await?;
        let run_state = if active_count > 0 {
            "skipped"
        } else {
            "queued"
        };
        let updated = sqlx::query(
            "update jobs set next_run_at = ?1, updated_at = ?2 where job_id = ?3 and enabled = 1 and next_run_at = ?4",
        )
        .bind(next_run_at)
        .bind(now)
        .bind(job_id.as_str())
        .bind(due_at)
        .execute(&mut *connection)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok::<bool, AppError>(false);
        }
        sqlx::query(
            r#"
            insert into job_runs (
                job_run_id, job_id, trigger_kind, state, scheduled_for, queued_at,
                started_at, finished_at, session_thread_id, runtime_session_id,
                command_id, turn_id, summary, terminal_code, terminal_message,
                config_snapshot_json, force
            ) values (?1, ?2, 'scheduled', ?3, ?4, ?5, null, ?6, null, null, null, null, null, ?7, ?8, ?9, 0)
            "#,
        )
        .bind(job_run_id.as_str())
        .bind(job_id.as_str())
        .bind(run_state)
        .bind(due_at)
        .bind(now)
        .bind((run_state == "skipped").then_some(now))
        .bind((run_state == "skipped").then_some("job.overlap_skipped"))
        .bind((run_state == "skipped").then_some("Another run of this Job is already active"))
        .bind(serde_json::to_string(&snapshot_for_job(&job))?)
        .execute(&mut *connection)
        .await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(_) => {
            sqlx::query("commit").execute(&mut *connection).await?;
        }
        Err(error) => {
            let _ = sqlx::query("rollback").execute(&mut *connection).await;
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) async fn drive_job_runs(state: &AppState) -> Result<(), AppError> {
    let run_ids: Vec<String> = sqlx::query_scalar(
        "select job_run_id from job_runs where state in ('queued', 'starting', 'running') order by queued_at, job_run_id",
    )
    .fetch_all(&state.pool)
    .await?;
    for run_id in run_ids {
        let run_id = JobRunId::from(run_id);
        let run = load_job_run(state, &run_id).await?;
        match run.state {
            JobRunState::Queued => {
                if let Err(error) = start_job_run(state, &run).await {
                    let (code, message) = scheduled_message_failure(&error);
                    fail_job_run(state, &run, &code, &message, false).await?;
                }
            }
            JobRunState::Starting => drive_starting_job_run(state, &run).await?,
            JobRunState::Running => drive_running_job_run(state, &run).await?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) async fn start_job_run(state: &AppState, run: &JobRunSummary) -> Result<(), AppError> {
    let snapshot: JobRunConfigSnapshot = serde_json::from_value(run.config_snapshot.0.clone())?;
    let placement = load_placement(state, &snapshot.project_placement_id).await?;
    ensure_node_commandable(state, &placement.node_id).await?;
    ensure_placement_startable(&placement)?;
    let execution_profile = AgentExecutionProfile::ExecCompatibility;
    let provider_capabilities = ensure_node_supports_execution_profile(
        state,
        &placement.node_id,
        &snapshot.provider,
        execution_profile,
    )
    .await?;
    ensure_provider_quota_admission(state, &snapshot.provider, run.force, "job.run").await?;
    let provider_version =
        provider_version_for_node(state, &placement.node_id, &snapshot.provider).await?;
    let effective_policy = resolve_effective_runtime_policy(
        &snapshot.provider,
        provider_version,
        execution_profile,
        &placement.workspace_path,
        provider_capabilities,
    );
    let effective_policy_hash = effective_policy.policy_hash()?;
    let effective_policy_json = serde_json::to_string(&effective_policy)?;

    let now = Utc::now();
    let session_thread_id = SessionThreadId::new();
    let runtime_session_id = RuntimeSessionId::new();
    let command = CommandEnvelope {
        command_id: CommandId::new(),
        kind: CommandKind::StartRuntime,
        target: CommandTarget::SessionRuntime {
            node_id: placement.node_id.clone(),
            project_placement_id: snapshot.project_placement_id.clone(),
            session_thread_id: session_thread_id.clone(),
            runtime_session_id: runtime_session_id.clone(),
        },
        actor_ref: ActorRef::System,
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: now,
        correlation_id: CorrelationId::new(),
        payload: CommandPayload::StartRuntime {
            provider: snapshot.provider.clone(),
            workspace_path: placement.workspace_path,
            execution_profile,
            effective_policy: Some(effective_policy),
            effective_policy_hash: Some(effective_policy_hash.clone()),
        },
    };
    let mut transaction = state.pool.begin().await?;
    let claimed = sqlx::query(
        "update job_runs set state = 'starting', started_at = ?1 where job_run_id = ?2 and state = 'queued'",
    )
    .bind(now)
    .bind(run.job_run_id.as_str())
    .execute(&mut *transaction)
    .await?;
    if claimed.rows_affected() == 0 {
        transaction.rollback().await?;
        return Ok(());
    }
    sqlx::query(
        "insert into session_threads (session_thread_id, project_placement_id, runtime_session_id, title, state, provider, created_at, updated_at) values (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?6)",
    )
    .bind(session_thread_id.as_str())
    .bind(snapshot.project_placement_id.as_str())
    .bind(runtime_session_id.as_str())
    .bind(format!("{} · Job Run", snapshot.name))
    .bind(&snapshot.provider)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "insert into runtime_sessions (runtime_session_id, session_thread_id, provider, state, resume_supported, provider_resume_ref_json, degraded_reason, last_runtime_step_at, execution_profile, effective_policy_json, effective_policy_hash, recovery_status, created_at, updated_at) values (?1, ?2, ?3, 'starting', 1, null, null, ?4, 'exec_compatibility', ?5, ?6, 'not_required', ?4, ?4)",
    )
    .bind(runtime_session_id.as_str())
    .bind(session_thread_id.as_str())
    .bind(&snapshot.provider)
    .bind(now)
    .bind(effective_policy_json)
    .bind(effective_policy_hash.as_str())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "update job_runs set session_thread_id = ?1, runtime_session_id = ?2 where job_run_id = ?3 and state = 'starting'",
    )
    .bind(session_thread_id.as_str())
    .bind(runtime_session_id.as_str())
    .bind(run.job_run_id.as_str())
    .execute(&mut *transaction)
    .await?;
    record_command_on_connection(&mut transaction, &command).await?;
    transaction.commit().await?;
    dispatch_pending_commands(state, &placement.node_id).await
}

pub(crate) async fn drive_starting_job_run(
    state: &AppState,
    run: &JobRunSummary,
) -> Result<(), AppError> {
    let Some(session_id) = run.session_thread_id.as_ref() else {
        return Ok(());
    };
    let snapshot: JobRunConfigSnapshot = serde_json::from_value(run.config_snapshot.0.clone())?;
    let detail = load_session_detail(state, session_id).await?;
    if detail.session.runtime.state == RuntimeSessionState::Error {
        return fail_job_run(
            state,
            run,
            "job.runtime_start_failed",
            detail
                .session
                .runtime
                .degraded_reason
                .as_deref()
                .unwrap_or("Provider runtime failed to start"),
            false,
        )
        .await;
    }
    let existing_turn: Option<(String, String)> = sqlx::query_as(
        "select turn_id, command_id from turns where session_thread_id = ?1 order by turn_index limit 1",
    )
    .bind(session_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    if let Some((turn_id, command_id)) = existing_turn {
        sqlx::query(
            "update job_runs set state = 'running', turn_id = ?1, command_id = ?2 where job_run_id = ?3 and state = 'starting'",
        )
        .bind(turn_id)
        .bind(command_id)
        .bind(run.job_run_id.as_str())
        .execute(&state.pool)
        .await?;
        return Ok(());
    }
    if !matches!(detail.session.runtime.state, RuntimeSessionState::Ready) {
        return Ok(());
    }
    let (turn_id, command_id) = {
        let accepted = submit_turn_for_actor(
            state,
            session_id.clone(),
            snapshot.prompt,
            None,
            CorrelationId::new(),
            ActorRef::System,
        )
        .await?;
        let turn_id: String = sqlx::query_scalar("select turn_id from turns where command_id = ?1")
            .bind(accepted.command_id.as_str())
            .fetch_one(&state.pool)
            .await?;
        (turn_id, accepted.command_id.to_string())
    };
    sqlx::query(
        "update job_runs set state = 'running', turn_id = ?1, command_id = ?2 where job_run_id = ?3 and state = 'starting'",
    )
    .bind(turn_id)
    .bind(command_id)
    .bind(run.job_run_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn drive_running_job_run(
    state: &AppState,
    run: &JobRunSummary,
) -> Result<(), AppError> {
    let Some(session_id) = run.session_thread_id.as_ref() else {
        return fail_job_run(
            state,
            run,
            "job.session_missing",
            "Job Run session is missing",
            false,
        )
        .await;
    };
    let turn: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "select state, completed_at from turns where turn_id = (select turn_id from job_runs where job_run_id = ?1)",
    )
    .bind(run.job_run_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    match turn.as_ref().map(|(state, _)| state.as_str()) {
        Some("completed") => {
            let summary: Option<String> = sqlx::query_scalar(
                "select content from messages where session_thread_id = ?1 and role = 'assistant' order by created_at desc limit 1",
            )
            .bind(session_id.as_str())
            .fetch_optional(&state.pool)
            .await?;
            sqlx::query(
                "update job_runs set state = 'succeeded', finished_at = ?1, summary = ?2 where job_run_id = ?3 and state = 'running'",
            )
            .bind(Utc::now())
            .bind(summary)
            .bind(run.job_run_id.as_str())
            .execute(&state.pool)
            .await?;
        }
        Some("failed") | Some("interrupted") => {
            fail_job_run(
                state,
                run,
                "job.turn_failed",
                "Provider turn failed or was interrupted",
                false,
            )
            .await?;
        }
        _ => {
            let runtime_state: Option<String> = sqlx::query_scalar(
                "select state from runtime_sessions where session_thread_id = ?1",
            )
            .bind(session_id.as_str())
            .fetch_optional(&state.pool)
            .await?;
            if matches!(runtime_state.as_deref(), Some("error") | Some("expired")) {
                let timed_out = runtime_state.as_deref() == Some("expired");
                fail_job_run(
                    state,
                    run,
                    if timed_out {
                        "job.timed_out"
                    } else {
                        "job.runtime_failed"
                    },
                    if timed_out {
                        "Provider runtime timed out"
                    } else {
                        "Provider runtime failed"
                    },
                    timed_out,
                )
                .await?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn fail_job_run(
    state: &AppState,
    run: &JobRunSummary,
    code: &str,
    message: &str,
    timed_out: bool,
) -> Result<(), AppError> {
    let state_value = if timed_out { "timed_out" } else { "failed" };
    let result = sqlx::query(
        "update job_runs set state = ?1, finished_at = ?2, terminal_code = ?3, terminal_message = ?4 where job_run_id = ?5 and state in ('queued', 'starting', 'running')",
    )
    .bind(state_value)
    .bind(Utc::now())
    .bind(code)
    .bind(message)
    .bind(run.job_run_id.as_str())
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(());
    }
    let snapshot: JobRunConfigSnapshot = serde_json::from_value(run.config_snapshot.0.clone())?;
    if !snapshot.continue_after_error {
        sqlx::query(
            "update jobs set enabled = 0, next_run_at = null, paused_reason = ?1, updated_at = ?2 where job_id = ?3",
        )
        .bind(code)
        .bind(Utc::now())
        .bind(run.job_id.as_str())
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}
