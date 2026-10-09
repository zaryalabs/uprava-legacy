use uprava_protocol::{
    ContributionRef, EffectiveContributionState, PluginCompatibilityState, PluginContribution,
    PluginDesiredState, PluginEffectiveState, PluginId, PluginManifest,
    UpdateContributionTargetPreferencesRequest,
};

use super::*;

fn assistant_renderer_resolution(
    snapshot: &uprava_protocol::EffectivePluginSnapshot,
) -> &uprava_protocol::ContributionTargetResolution {
    snapshot
        .resolutions
        .iter()
        .find(|resolution| {
            matches!(
                &resolution.target,
                uprava_protocol::ContributionTarget::VisualRenderer {
                    source_kind,
                    surface,
                    render_scope: uprava_protocol::VisualRenderScope::ContentEnhancement,
                    selector: None,
                } if source_kind == "chat.assistant_message" && surface == "session.timeline"
            )
        })
        .expect("assistant renderer target resolves")
}

#[tokio::test]
async fn bundled_plugins_should_bootstrap_idempotently() {
    let state = test_state().await;

    bootstrap_bundled_plugins(&state)
        .await
        .expect("second bootstrap succeeds");
    let plugins = list_plugins(&state).await.expect("plugins load");

    assert_eq!(plugins.items.len(), 8);
    let latest_migration: i64 = sqlx::query_scalar("select max(version) from schema_migrations")
        .fetch_one(&state.pool)
        .await
        .expect("migration version loads");
    assert_eq!(latest_migration, 19);
}

#[tokio::test]
async fn visual_artifact_plugins_publish_resolved_types_and_renderers() {
    let state = test_state().await;
    let snapshot = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");

    for artifact_type in [
        "uprava.diagram",
        "uprava.diff-report",
        "uprava.check-report",
        "uprava.trace-timeline",
        "uprava.causality-narrative",
    ] {
        let resolution = snapshot
            .resolutions
            .iter()
            .find(|resolution| {
                matches!(
                    &resolution.target,
                    uprava_protocol::ContributionTarget::ArtifactType {
                        artifact_type: candidate,
                    } if candidate == artifact_type
                )
            })
            .expect("artifact type resolves");
        assert_eq!(resolution.extension_point, "artifact.type");
        assert_eq!(resolution.contributions.len(), 1);
        assert_eq!(
            resolution.contributions[0].effective_state,
            EffectiveContributionState::Available
        );
    }
    assert!(snapshot.resolutions.iter().any(|resolution| {
        matches!(
            &resolution.target,
            uprava_protocol::ContributionTarget::VisualRenderer {
                source_kind,
                surface,
                render_scope: uprava_protocol::VisualRenderScope::InlineFragment,
                selector: Some(selector),
            } if source_kind == "markdown.code_fence"
                && surface == "session.timeline"
                && selector == "mermaid"
        )
    }));
}

#[tokio::test]
async fn bundled_upgrade_should_preserve_desired_state() {
    let state = test_state().await;
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../bundled-plugins/uprava.markdown/manifest.json"
    ))
    .expect("manifest parses");
    manifest.plugin_id = PluginId::from("uprava.upgrade-fixture");
    manifest.version = "1.0.0".to_owned();
    let PluginContribution::VisualRenderer {
        contribution_id,
        contribution,
        ..
    } = &mut manifest.contributions[0]
    else {
        panic!("fixture contains a visual renderer");
    };
    *contribution_id = "uprava.upgrade-fixture.chat".to_owned();
    contribution.renderer_id = "uprava.upgrade-fixture.chat".to_owned();
    contribution.implementation_id = "uprava.upgrade-fixture.v1".to_owned();
    register_bundled_plugin(&state, &manifest, PluginDesiredState::Enabled)
        .await
        .expect("old package registers");
    set_plugin_desired_state(&state, &manifest.plugin_id, PluginDesiredState::Disabled)
        .await
        .expect("fixture disables");
    manifest.version = "1.1.0".to_owned();

    register_bundled_plugin(&state, &manifest, PluginDesiredState::Enabled)
        .await
        .expect("new package registers");
    let installation = load_plugin_installation(&state, &manifest.plugin_id)
        .await
        .expect("upgraded installation loads");

    assert_eq!(installation.package.version, "1.1.0");
    assert_eq!(installation.desired_state, PluginDesiredState::Disabled);
}

#[tokio::test]
async fn markdown_plugin_should_bootstrap_enabled() {
    let state = test_state().await;
    let plugin = load_plugin_installation(&state, &PluginId::from("uprava.markdown"))
        .await
        .expect("Markdown plugin loads");

    assert_eq!(plugin.desired_state, PluginDesiredState::Enabled);
    assert_eq!(plugin.effective_state, PluginEffectiveState::Active);
    assert_eq!(
        plugin.compatibility.state,
        PluginCompatibilityState::Compatible
    );
    assert_eq!(plugin.granted_permissions, ["visual.renderer.contribute"]);
}

#[tokio::test]
async fn dark_theme_should_bootstrap_disabled() {
    let state = test_state().await;
    let plugin = load_plugin_installation(&state, &PluginId::from("uprava.theme-dark"))
        .await
        .expect("theme plugin loads");

    assert_eq!(plugin.desired_state, PluginDesiredState::Disabled);
    assert_eq!(plugin.effective_state, PluginEffectiveState::Disabled);
}

#[tokio::test]
async fn exclusive_renderer_target_should_have_stable_visible_conflict() {
    let state = test_state().await;
    let snapshot = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");
    let resolution = assistant_renderer_resolution(&snapshot);

    assert!(resolution.conflict);
    assert_eq!(
        resolution.contributions[0].plugin_id,
        "uprava.markdown".into()
    );
    assert_eq!(
        resolution.contributions[1].plugin_id,
        "uprava.plain-text".into()
    );
}

#[tokio::test]
async fn contribution_preferences_should_reorder_and_disable_candidates() {
    let state = test_state().await;
    let snapshot = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");
    let resolution = assistant_renderer_resolution(&snapshot);
    let markdown = ContributionRef {
        plugin_id: PluginId::from("uprava.markdown"),
        contribution_id: "uprava.markdown.chat".to_owned(),
    };
    let plain_text = ContributionRef {
        plugin_id: PluginId::from("uprava.plain-text"),
        contribution_id: "uprava.plain-text.chat".to_owned(),
    };

    update_contribution_target_preferences(
        &state,
        &resolution.target_id,
        UpdateContributionTargetPreferencesRequest {
            expected_revision: 0,
            ordered_contributions: vec![plain_text, markdown.clone()],
            disabled_contributions: vec![markdown],
        },
    )
    .await
    .expect("preferences update");
    let updated = effective_plugin_snapshot(&state)
        .await
        .expect("updated snapshot loads");
    let updated = assistant_renderer_resolution(&updated);

    assert_eq!(updated.revision, 1);
    assert_eq!(
        updated.contributions[0].plugin_id,
        PluginId::from("uprava.plain-text")
    );
    assert_eq!(
        updated.contributions[1].effective_state,
        EffectiveContributionState::Disabled
    );
    assert!(!updated.conflict);
}

#[tokio::test]
async fn contribution_preferences_should_reject_stale_revision() {
    let state = test_state().await;
    let snapshot = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");
    let target_id = assistant_renderer_resolution(&snapshot).target_id.clone();
    let request = UpdateContributionTargetPreferencesRequest {
        expected_revision: 0,
        ordered_contributions: Vec::new(),
        disabled_contributions: Vec::new(),
    };
    update_contribution_target_preferences(&state, &target_id, request.clone())
        .await
        .expect("first update succeeds");

    let error = update_contribution_target_preferences(&state, &target_id, request)
        .await
        .expect_err("stale update fails");

    assert!(matches!(error, AppError::Conflict { .. }));
}

#[tokio::test]
async fn plugin_enable_and_disable_should_change_effective_snapshot() {
    let state = test_state().await;
    let plugin_id = PluginId::from("uprava.theme-dark");

    set_plugin_desired_state(&state, &plugin_id, PluginDesiredState::Enabled)
        .await
        .expect("plugin enables");
    let enabled = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");
    assert!(enabled.contributions.iter().any(|contribution| {
        contribution.plugin_id == plugin_id && contribution.extension_point == "ui.theme"
    }));

    set_plugin_desired_state(&state, &plugin_id, PluginDesiredState::Disabled)
        .await
        .expect("plugin disables");
    let disabled = effective_plugin_snapshot(&state)
        .await
        .expect("snapshot loads");
    assert!(!disabled
        .contributions
        .iter()
        .any(|contribution| contribution.plugin_id == plugin_id));
}

#[tokio::test]
async fn plugin_desired_state_should_survive_core_restart() {
    let pool = memory_pool().await;
    let config = test_config(86_400);
    let state = AppState::new(config.clone(), pool.clone())
        .await
        .expect("state starts");
    let plugin_id = PluginId::from("uprava.theme-dark");
    set_plugin_desired_state(&state, &plugin_id, PluginDesiredState::Enabled)
        .await
        .expect("plugin enables");
    drop(state);

    let restarted = AppState::new(config, pool).await.expect("state restarts");
    let plugin = load_plugin_installation(&restarted, &plugin_id)
        .await
        .expect("plugin loads");

    assert_eq!(plugin.desired_state, PluginDesiredState::Enabled);
    assert_eq!(plugin.effective_state, PluginEffectiveState::Active);
}

#[tokio::test]
async fn plugin_lifecycle_should_write_bounded_audit_events() {
    let state = test_state().await;
    let plugin_id = PluginId::from("uprava.theme-dark");

    set_plugin_desired_state(&state, &plugin_id, PluginDesiredState::Enabled)
        .await
        .expect("plugin enables");
    let metadata: String = sqlx::query_scalar(
        "select metadata_json from security_audit_events where kind = 'plugin.enabled' order by happened_at desc limit 1",
    )
    .fetch_one(&state.pool)
    .await
    .expect("audit event loads");

    assert!(metadata.contains("uprava.theme-dark"));
    assert!(!metadata.contains("semantic_tokens"));
}
