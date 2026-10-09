//! Node protocol URLs, capability discovery and versioned state paths.

use super::*;

pub(crate) fn control_url(core_url: &Url) -> anyhow::Result<Url> {
    let mut url = core_url
        .join("/api/v1/node/control")
        .context("control URL should be valid")?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => anyhow::bail!("unsupported Core URL scheme `{other}`"),
    };
    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("failed to set control URL scheme"))?;
    Ok(url)
}

pub(crate) fn capabilities(config: &NodeConfig) -> Vec<CapabilitySummary> {
    let codex_available = command_available(&config.codex_binary);
    let codex_managed_available = codex_available
        && config.codex_managed_unavailable_reason.is_none()
        && config.codex_version.is_some();
    vec![
        CapabilitySummary {
            key: "provider.codex.version".to_owned(),
            value: CapabilityValue::Extension {
                name: "provider_version".to_owned(),
                value: JsonValue(serde_json::json!(config.codex_version)),
            },
        },
        CapabilitySummary {
            key: "provider.codex".to_owned(),
            value: CapabilityValue::Provider {
                available: codex_available,
                configured: true,
                mode: "exec".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: (!codex_available).then(|| "binary_not_found".to_owned()),
            },
        },
        CapabilitySummary {
            key: ProviderRuntimeCapability::CodexExec.as_str().to_owned(),
            value: CapabilityValue::Provider {
                available: codex_available,
                configured: true,
                mode: "exec_compatibility".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: (!codex_available).then(|| "binary_not_found".to_owned()),
            },
        },
        CapabilitySummary {
            key: ProviderRuntimeCapability::CodexManaged.as_str().to_owned(),
            value: CapabilityValue::Provider {
                available: codex_managed_available,
                configured: true,
                mode: "managed".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: config.codex_managed_unavailable_reason.clone(),
            },
        },
        CapabilitySummary {
            key: ProviderRuntimeCapability::CodexManagedApproval
                .as_str()
                .to_owned(),
            value: CapabilityValue::Provider {
                available: codex_managed_available,
                configured: true,
                mode: "managed".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: config.codex_managed_unavailable_reason.clone(),
            },
        },
        CapabilitySummary {
            key: ProviderRuntimeCapability::CodexManagedInterrupt
                .as_str()
                .to_owned(),
            value: CapabilityValue::Provider {
                available: codex_managed_available,
                configured: true,
                mode: "managed".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: config.codex_managed_unavailable_reason.clone(),
            },
        },
        CapabilitySummary {
            key: ProviderRuntimeCapability::CodexManagedResume
                .as_str()
                .to_owned(),
            value: CapabilityValue::Provider {
                available: codex_managed_available,
                configured: true,
                mode: "managed".to_owned(),
                timeout_seconds: Some(config.codex_timeout.as_secs()),
                unavailable_reason: config.codex_managed_unavailable_reason.clone(),
            },
        },
        CapabilitySummary {
            key: "workspace.validation".to_owned(),
            value: CapabilityValue::WorkspaceValidation {
                mode: "explicit_path".to_owned(),
            },
        },
        CapabilitySummary {
            key: "task_runtime.opensandbox.docker".to_owned(),
            value: CapabilityValue::Extension {
                name: "task_runtime".to_owned(),
                value: JsonValue(serde_json::json!({
                    "available": config.opensandbox_url.is_some(),
                    "configured": config.opensandbox_url.is_some(),
                    "backend": "opensandbox",
                    "mode": "docker",
                    "provider": "codex",
                    "runtime_image": config.task_runtime_image,
                    "auth_mode": "deferred_manual"
                })),
            },
        },
    ]
}

pub(crate) fn command_available(binary: &str) -> bool {
    let path = Path::new(binary);
    if path.components().count() > 1 || path.is_absolute() {
        return path.is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    command_available_in_search_path(binary, &paths)
}

pub(crate) fn command_available_in_search_path(
    binary: &str,
    search_path: &std::ffi::OsStr,
) -> bool {
    std::env::split_paths(search_path).any(|directory| directory.join(binary).is_file())
}

pub(crate) fn validate_workspace(path: &Path) -> WorkspaceSnapshot {
    let display_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_owned();
    let (state, resource_badges, git_snapshot) = if !path.exists() {
        (
            PlacementState::Missing,
            vec![ResourceBadge {
                kind: "missing_workspace".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Workspace missing".to_owned(),
            }],
            None,
        )
    } else if !path.is_dir() {
        (
            PlacementState::Missing,
            vec![ResourceBadge {
                kind: "workspace_not_directory".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Workspace is not a directory".to_owned(),
            }],
            None,
        )
    } else if std::fs::metadata(path)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false)
    {
        (
            PlacementState::ReadOnly,
            vec![ResourceBadge {
                kind: "read_only_workspace".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Read-only workspace".to_owned(),
            }],
            None,
        )
    } else {
        let git_snapshot = git_workspace_snapshot(path);
        (
            PlacementState::Validated,
            resource_warnings_for_snapshot(git_snapshot.as_ref()),
            git_snapshot,
        )
    };

    WorkspaceSnapshot {
        display_name,
        workspace_path: path.display().to_string(),
        state,
        resource_badges,
        git_snapshot,
        last_validated_at: chrono::Utc::now(),
    }
}

pub(crate) fn git_workspace_snapshot(path: &Path) -> Option<GitWorkspaceSnapshot> {
    let generated_at = Utc::now();
    let (stdout, output_truncated, success) = capped_git_status(path)?;
    if !success {
        return Some(empty_git_snapshot(
            GitRepositoryState::NotRepository,
            generated_at,
        ));
    }
    let status = String::from_utf8_lossy(&stdout);
    let mut snapshot = parse_git_porcelain_v2(&status, generated_at);
    snapshot.truncated |= output_truncated;
    let git_dir = git_path_output(path, &["rev-parse", "--path-format=absolute", "--git-dir"]);
    let common_dir = git_path_output(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    snapshot.worktree_kind = match (&git_dir, &common_dir) {
        (Some(git_dir), Some(common_dir)) if git_dir == common_dir => {
            Some(GitWorktreeKind::Primary)
        }
        (Some(_), Some(_)) => Some(GitWorktreeKind::Linked),
        _ => None,
    };
    snapshot.operation = git_dir.as_deref().and_then(git_operation);
    snapshot.repo_id = git_repository_id(path);
    Some(snapshot)
}

fn capped_git_status(path: &Path) -> Option<(Vec<u8>, bool, bool)> {
    let mut child = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain=v2", "--branch", "-z"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let mut captured = Vec::with_capacity(MAX_GIT_STATUS_BYTES);
    let mut buffer = [0_u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let read = stdout.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        let remaining = MAX_GIT_STATUS_BYTES.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    let status = child.wait().ok()?;
    Some((captured, truncated, status.success()))
}

fn empty_git_snapshot(
    state: GitRepositoryState,
    generated_at: DateTime<Utc>,
) -> GitWorkspaceSnapshot {
    GitWorkspaceSnapshot {
        state,
        repo_id: None,
        head_state: None,
        branch: None,
        commit: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        worktree_kind: None,
        operation: None,
        changed_files: vec![],
        staged_count: 0,
        unstaged_count: 0,
        untracked_count: 0,
        conflicted_count: 0,
        truncated: false,
        generated_at,
    }
}

pub(crate) fn parse_git_porcelain_v2(
    status: &str,
    generated_at: DateTime<Utc>,
) -> GitWorkspaceSnapshot {
    let mut snapshot = empty_git_snapshot(GitRepositoryState::Ready, generated_at);
    let records: Vec<&str> = status.split('\0').collect();
    let mut index = 0usize;
    while index < records.len() {
        let record = records[index];
        if let Some(value) = record.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                snapshot.commit = Some(value.to_owned());
            }
        } else if let Some(value) = record.strip_prefix("# branch.head ") {
            if value == "(detached)" {
                snapshot.head_state = Some(GitHeadState::Detached);
            } else {
                snapshot.head_state = Some(GitHeadState::Branch);
                snapshot.branch = Some(value.to_owned());
            }
        } else if let Some(value) = record.strip_prefix("# branch.upstream ") {
            snapshot.upstream = Some(value.to_owned());
        } else if let Some(value) = record.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(ahead) = part.strip_prefix('+') {
                    snapshot.ahead = ahead.parse().unwrap_or_default();
                } else if let Some(behind) = part.strip_prefix('-') {
                    snapshot.behind = behind.parse().unwrap_or_default();
                }
            }
        } else if let Some(rest) = record.strip_prefix("1 ") {
            if let Some(change) = parse_ordinary_git_change(rest) {
                push_git_change(&mut snapshot, change);
            }
        } else if let Some(rest) = record.strip_prefix("2 ") {
            if let Some(mut change) = parse_renamed_git_change(rest) {
                index += 1;
                change.previous_path = records.get(index).map(|path| (*path).to_owned());
                push_git_change(&mut snapshot, change);
            }
        } else if let Some(rest) = record.strip_prefix("u ") {
            if let Some(path) = rest.splitn(11, ' ').nth(10) {
                push_git_change(
                    &mut snapshot,
                    GitChangedFile {
                        path: path.to_owned(),
                        previous_path: None,
                        index_status: Some(GitChangeKind::Unmerged),
                        worktree_status: Some(GitChangeKind::Unmerged),
                        conflicted: true,
                        binary: false,
                    },
                );
            }
        } else if let Some(path) = record.strip_prefix("? ") {
            push_git_change(
                &mut snapshot,
                GitChangedFile {
                    path: path.to_owned(),
                    previous_path: None,
                    index_status: None,
                    worktree_status: Some(GitChangeKind::Untracked),
                    conflicted: false,
                    binary: false,
                },
            );
        }
        index += 1;
    }
    if snapshot.head_state == Some(GitHeadState::Branch) && snapshot.commit.is_none() {
        snapshot.head_state = Some(GitHeadState::Unborn);
    }
    snapshot
}

fn parse_ordinary_git_change(record: &str) -> Option<GitChangedFile> {
    let mut fields = record.splitn(9, ' ');
    let xy = fields.next()?;
    for _ in 0..6 {
        fields.next()?;
    }
    let path = fields.next()?;
    Some(git_changed_file(path, None, xy))
}

fn parse_renamed_git_change(record: &str) -> Option<GitChangedFile> {
    let mut fields = record.splitn(10, ' ');
    let xy = fields.next()?;
    for _ in 0..7 {
        fields.next()?;
    }
    let path = fields.next()?;
    Some(git_changed_file(path, None, xy))
}

fn git_changed_file(path: &str, previous_path: Option<String>, xy: &str) -> GitChangedFile {
    let mut chars = xy.chars();
    let index_status = chars.next().and_then(git_change_kind);
    let worktree_status = chars.next().and_then(git_change_kind);
    let conflicted = matches!(index_status, Some(GitChangeKind::Unmerged))
        || matches!(worktree_status, Some(GitChangeKind::Unmerged));
    GitChangedFile {
        path: path.to_owned(),
        previous_path,
        index_status,
        worktree_status,
        conflicted,
        binary: false,
    }
}

fn git_change_kind(status: char) -> Option<GitChangeKind> {
    match status {
        '.' => None,
        'A' => Some(GitChangeKind::Added),
        'M' => Some(GitChangeKind::Modified),
        'D' => Some(GitChangeKind::Deleted),
        'R' => Some(GitChangeKind::Renamed),
        'C' => Some(GitChangeKind::Copied),
        'T' => Some(GitChangeKind::TypeChanged),
        'U' => Some(GitChangeKind::Unmerged),
        '?' => Some(GitChangeKind::Untracked),
        _ => Some(GitChangeKind::Unknown),
    }
}

fn push_git_change(snapshot: &mut GitWorkspaceSnapshot, change: GitChangedFile) {
    if change.index_status.is_some() {
        snapshot.staged_count += 1;
    }
    if change.worktree_status.is_some() {
        snapshot.unstaged_count += 1;
    }
    if change.worktree_status == Some(GitChangeKind::Untracked) {
        snapshot.untracked_count += 1;
    }
    if change.conflicted {
        snapshot.conflicted_count += 1;
    }
    if snapshot.changed_files.len() < MAX_GIT_CHANGED_FILES {
        snapshot.changed_files.push(change);
    } else {
        snapshot.truncated = true;
    }
}

fn git_path_output(path: &Path, args: &[&str]) -> Option<PathBuf> {
    let output = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

fn git_repository_id(path: &Path) -> Option<String> {
    let remote = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let identity = remote.filter(|value| !value.is_empty()).or_else(|| {
        git_path_output(path, &["rev-parse", "--show-toplevel"])
            .map(|value| value.display().to_string())
    })?;
    let normalized = redact_git_credentials(&identity);
    Some(format!(
        "sha256:{:x}",
        Sha256::digest(normalized.as_bytes())
    ))
}

fn redact_git_credentials(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_owned();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}{}", &rest[authority_end..])
}

fn git_operation(git_dir: &Path) -> Option<GitOperation> {
    [
        ("MERGE_HEAD", GitOperation::Merge),
        ("rebase-merge", GitOperation::Rebase),
        ("rebase-apply", GitOperation::Rebase),
        ("CHERRY_PICK_HEAD", GitOperation::CherryPick),
        ("REVERT_HEAD", GitOperation::Revert),
        ("BISECT_LOG", GitOperation::Bisect),
    ]
    .into_iter()
    .find_map(|(marker, operation)| git_dir.join(marker).exists().then_some(operation))
}

pub(crate) fn resource_warnings_for_snapshot(
    snapshot: Option<&GitWorkspaceSnapshot>,
) -> Vec<ResourceBadge> {
    let Some(snapshot) = snapshot else {
        return vec![ResourceBadge {
            kind: "git_snapshot_unavailable".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Git snapshot unavailable".to_owned(),
        }];
    };
    if snapshot.state == GitRepositoryState::NotRepository {
        return vec![];
    }
    if snapshot.state == GitRepositoryState::Unavailable {
        return vec![ResourceBadge {
            kind: "git_snapshot_unavailable".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Git snapshot unavailable".to_owned(),
        }];
    }
    let mut badges = vec![ResourceBadge {
        kind: "git_workspace".to_owned(),
        severity: WarningSeverity::Info,
        label: "Git workspace".to_owned(),
    }];
    if let Some(branch) = &snapshot.branch {
        badges.push(ResourceBadge {
            kind: "git_branch".to_owned(),
            severity: WarningSeverity::Info,
            label: format!("Git branch: {branch}"),
        });
    }
    let dirty_count = snapshot.changed_files.len();
    if dirty_count > 0 {
        badges.push(ResourceBadge {
            kind: "dirty_workspace".to_owned(),
            severity: WarningSeverity::Warning,
            label: dirty_workspace_label(dirty_count, snapshot.untracked_count as usize),
        });
    }
    if snapshot.conflicted_count > 0 {
        badges.push(ResourceBadge {
            kind: "git_conflicts".to_owned(),
            severity: WarningSeverity::HardBlock,
            label: format!("{} conflicted path(s)", snapshot.conflicted_count),
        });
    }
    if snapshot.head_state == Some(GitHeadState::Detached) {
        badges.push(ResourceBadge {
            kind: "detached_head".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Git HEAD is detached".to_owned(),
        });
    }
    if let Some(operation) = snapshot.operation {
        badges.push(ResourceBadge {
            kind: "git_operation_in_progress".to_owned(),
            severity: WarningSeverity::Warning,
            label: format!("Git {operation:?} is in progress"),
        });
    }
    if snapshot.behind > 0 {
        badges.push(ResourceBadge {
            kind: "branch_behind".to_owned(),
            severity: WarningSeverity::Warning,
            label: format!("Branch is behind upstream by {} commit(s)", snapshot.behind),
        });
    }
    if snapshot.ahead > 0 {
        badges.push(ResourceBadge {
            kind: "branch_ahead".to_owned(),
            severity: WarningSeverity::Info,
            label: format!(
                "Branch is ahead of upstream by {} commit(s)",
                snapshot.ahead
            ),
        });
    }
    badges
}

#[cfg(test)]
pub(crate) fn resource_warnings(path: &Path) -> Vec<ResourceBadge> {
    let mut badges = Vec::new();
    if !path.join(".git").exists() {
        return badges;
    }

    badges.push(ResourceBadge {
        kind: "git_workspace".to_owned(),
        severity: WarningSeverity::Info,
        label: "Git workspace".to_owned(),
    });
    if let Some(status) = git_status_snapshot(path) {
        badges.extend(git_status_badges(&status));
    } else {
        badges.push(ResourceBadge {
            kind: "git_snapshot_unavailable".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Git snapshot unavailable".to_owned(),
        });
    }
    badges
}

#[cfg(test)]
pub(crate) fn git_status_snapshot(path: &Path) -> Option<String> {
    let output = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("--branch")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
pub(crate) fn git_status_badges(status: &str) -> Vec<ResourceBadge> {
    let mut branch = None;
    let mut dirty_count = 0usize;
    let mut untracked_count = 0usize;
    let mut ahead_count = 0usize;
    let mut behind_count = 0usize;

    for line in status.lines() {
        if let Some(summary) = line.strip_prefix("## ") {
            branch = git_branch_label(summary);
            ahead_count = git_tracking_count(summary, "ahead");
            behind_count = git_tracking_count(summary, "behind");
        } else if !line.trim().is_empty() {
            dirty_count += 1;
            if line.starts_with("?? ") {
                untracked_count += 1;
            }
        }
    }

    let mut badges = Vec::new();
    if let Some(branch) = branch {
        badges.push(ResourceBadge {
            kind: "git_branch".to_owned(),
            severity: WarningSeverity::Info,
            label: format!("Git branch: {branch}"),
        });
    }
    if dirty_count > 0 {
        badges.push(ResourceBadge {
            kind: "dirty_workspace".to_owned(),
            severity: WarningSeverity::Warning,
            label: dirty_workspace_label(dirty_count, untracked_count),
        });
    }
    if behind_count > 0 {
        badges.push(ResourceBadge {
            kind: "branch_behind".to_owned(),
            severity: WarningSeverity::Warning,
            label: format!("Branch is behind upstream by {behind_count} commit(s)"),
        });
    }
    if ahead_count > 0 {
        badges.push(ResourceBadge {
            kind: "branch_ahead".to_owned(),
            severity: WarningSeverity::Info,
            label: format!("Branch is ahead of upstream by {ahead_count} commit(s)"),
        });
    }
    badges
}

#[cfg(test)]
pub(crate) fn git_branch_label(summary: &str) -> Option<String> {
    let without_upstream = summary.split("...").next().unwrap_or(summary);
    let without_tracking = without_upstream
        .split(" [")
        .next()
        .unwrap_or(without_upstream)
        .trim();
    (!without_tracking.is_empty()).then(|| without_tracking.to_owned())
}

#[cfg(test)]
pub(crate) fn git_tracking_count(summary: &str, key: &str) -> usize {
    let Some((_, tracking)) = summary.split_once('[') else {
        return 0;
    };
    let tracking = tracking.strip_suffix(']').unwrap_or(tracking);
    tracking
        .split(',')
        .map(str::trim)
        .find_map(|part| {
            part.strip_prefix(key)
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0)
}

pub(crate) fn dirty_workspace_label(dirty_count: usize, untracked_count: usize) -> String {
    if untracked_count == 0 {
        return format!("Git workspace has {dirty_count} changed path(s)");
    }
    format!(
        "Git workspace has {dirty_count} changed path(s), including {untracked_count} untracked"
    )
}

pub(crate) fn default_state_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".local")
        .join("share")
        .join("uprava-node")
        .join(NODE_STATE_SLOT)
        .join("node.sqlite")
}

pub(crate) fn is_sqlite_state_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "sqlite")
}

pub(crate) fn legacy_state_path(path: &Path) -> Option<PathBuf> {
    let slot_dir = path.parent()?;
    if slot_dir.file_name()?.to_string_lossy() != NODE_STATE_SLOT {
        return None;
    }
    Some(slot_dir.parent()?.join("node.json"))
}

pub(crate) fn is_versioned_state_path(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name.to_string_lossy() == NODE_STATE_SLOT)
}
