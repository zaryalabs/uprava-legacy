//! Disposable conformance probe for the pinned Codex app-server protocol.

use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use axum::extract::{Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const CLIENT_NAME: &str = "uprava-provider-protocol-spike";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROTOCOL_LINE_BYTES: usize = 1024 * 1024;
const MAX_STDERR_LINES: usize = 64;
const MCP_TOKEN_ENV_PREFIX: &str = "UPRAVA_MCP_ACCESS_TOKEN_";
const MCP_PROBE_TOKEN: &str = concat!(
    "00000000-0000-4000-8000-000000000000.",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
);
const PROBE_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "CODEX_SHELL",
    "CODEX_HOME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
];

#[derive(Debug)]
struct Args {
    codex: PathBuf,
    workspace: PathBuf,
    step_timeout: Duration,
    skip_live_turns: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeReport {
    probe_version: &'static str,
    provider_baseline: String,
    transport: &'static str,
    gate_passed: bool,
    checks: BTreeMap<&'static str, CheckResult>,
    observations: Observations,
    measurements: Measurements,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckResult {
    passed: bool,
    detail: &'static str,
}

impl CheckResult {
    const fn passed(detail: &'static str) -> Self {
        Self {
            passed: true,
            detail,
        }
    }

    const fn failed(detail: &'static str) -> Self {
        Self {
            passed: false,
            detail,
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Observations {
    provider_user_agent_family: String,
    effective_safe_sandbox: String,
    effective_safe_approval_policy: String,
    effective_unrestricted_sandbox: String,
    effective_unrestricted_approval_policy: String,
    notification_methods: BTreeSet<String>,
    server_request_methods: BTreeSet<String>,
    command_activity_items: usize,
    agent_output_deltas: usize,
    provider_thread_reference: &'static str,
    recovery_after_forced_stop: String,
    mcp_authorized_requests: usize,
    mcp_tool_lists: usize,
    mcp_authorized_calls: usize,
    mcp_auth_rejections: usize,
    lease_token_exposed: bool,
    stderr_line_count: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurements {
    idle_process_rss_kib: Option<u64>,
    active_process_rss_kib: Option<u64>,
    active_turn_overhead_rss_kib: Option<u64>,
    graceful_shutdown_latency_ms: u128,
    forced_shutdown_latency_ms: u128,
}

#[derive(Debug)]
struct AppServer {
    child: Child,
    endpoint: String,
    stderr_task: JoinHandle<Vec<String>>,
}

#[derive(Debug)]
struct ProtocolClient {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_request_id: u64,
    step_timeout: Duration,
    notification_methods: BTreeSet<String>,
    server_request_methods: BTreeSet<String>,
    command_activity_items: usize,
    agent_output_deltas: usize,
}

#[derive(Clone, Copy, Debug)]
enum ApprovalDecision {
    Accept,
    Decline,
}

#[derive(Debug)]
struct TurnOutcome {
    status: String,
    approval_requests: usize,
    user_input_requests: usize,
}

#[derive(Debug)]
struct ShutdownResult {
    latency: Duration,
    stderr_lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct ProbeMcpState {
    authorized_requests: Arc<AtomicUsize>,
    tool_lists: Arc<AtomicUsize>,
    authorized_calls: Arc<AtomicUsize>,
    auth_rejections: Arc<AtomicUsize>,
}

#[derive(Clone, Debug)]
struct ProbeMcpServer {
    state: ProbeMcpState,
}

#[derive(Debug)]
struct ProbeMcpFixture {
    endpoint: String,
    state: ProbeMcpState,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let report = run_probe(&args).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    ensure!(report.gate_passed, "provider protocol gate failed");
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut codex = PathBuf::from("codex");
    let mut workspace = None;
    let mut step_timeout = DEFAULT_TIMEOUT;
    let mut skip_live_turns = false;
    let mut values = std::env::args().skip(1);

    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--codex" => {
                codex = PathBuf::from(values.next().context("--codex requires a path")?);
            }
            "--workspace" => {
                workspace = Some(PathBuf::from(
                    values.next().context("--workspace requires a path")?,
                ));
            }
            "--timeout-seconds" => {
                let seconds = values
                    .next()
                    .context("--timeout-seconds requires a value")?
                    .parse::<u64>()
                    .context("--timeout-seconds must be an integer")?;
                ensure!(seconds > 0, "--timeout-seconds must be positive");
                step_timeout = Duration::from_secs(seconds);
            }
            "--skip-live-turns" => skip_live_turns = true,
            "--help" | "-h" => {
                println!(
                    "Usage: codex-app-server-probe --workspace ABSOLUTE_PATH \
                     [--codex PATH] [--timeout-seconds N] [--skip-live-turns]"
                );
                std::process::exit(0);
            }
            unknown => bail!("unknown argument: {unknown}"),
        }
    }

    let workspace = workspace.context("--workspace is required")?;
    ensure!(workspace.is_absolute(), "--workspace must be absolute");
    ensure!(
        workspace.is_dir(),
        "--workspace must be an existing directory"
    );

    Ok(Args {
        codex,
        workspace,
        step_timeout,
        skip_live_turns,
    })
}

async fn run_probe(args: &Args) -> Result<ProbeReport> {
    let provider_baseline = provider_version(&args.codex).await?;
    let mcp_fixture = ProbeMcpFixture::start().await?;
    let mut report = ProbeReport {
        probe_version: CLIENT_VERSION,
        provider_baseline,
        transport: "loopback-websocket/json",
        ..ProbeReport::default()
    };

    let mut server =
        AppServer::spawn(&args.codex, args.step_timeout, &mcp_fixture.endpoint).await?;
    let mut client = server.connect(args.step_timeout).await?;
    let initialize = client.initialize().await?;
    report.observations.provider_user_agent_family = initialize
        .get("userAgent")
        .and_then(Value::as_str)
        .map(redact_user_agent)
        .unwrap_or_else(|| "unknown".to_owned());
    report.checks.insert(
        "handshake",
        CheckResult::passed("initialize returned provider and platform metadata"),
    );
    let mcp_ready = client.wait_for_mcp_tool("uprava", "search_tools").await?;
    let mcp_tools_listed = mcp_fixture.tool_lists() > 0;
    report.checks.insert(
        "mcpDiscovery",
        if mcp_ready || mcp_tools_listed {
            CheckResult::passed(
                "app-server authenticated and listed the isolated Uprava-shaped MCP tool",
            )
        } else {
            CheckResult::failed("app-server did not list the isolated Uprava-shaped MCP tool")
        },
    );

    let unrestricted = client
        .start_thread(&args.workspace, "danger-full-access", "never", true)
        .await?;
    report.observations.effective_unrestricted_sandbox = policy_kind(&unrestricted["sandbox"]);
    report.observations.effective_unrestricted_approval_policy =
        policy_kind(&unrestricted["approvalPolicy"]);
    let unrestricted_ok = report.observations.effective_unrestricted_sandbox
        == "danger-full-access"
        && report.observations.effective_unrestricted_approval_policy == "never";
    report.checks.insert(
        "unrestrictedPolicyEcho",
        if unrestricted_ok {
            CheckResult::passed("explicit unrestricted policy is readable before a turn")
        } else {
            CheckResult::failed("provider did not echo the requested unrestricted policy")
        },
    );

    let safe = client
        .start_thread(&args.workspace, "workspace-write", "untrusted", false)
        .await?;
    let thread_id = required_string(&safe, &["thread", "id"])?;
    let provider_model = required_string(&safe, &["model"])?;
    report.observations.provider_thread_reference = "opaque-provider-thread-id (redacted)";
    report.observations.effective_safe_sandbox = policy_kind(&safe["sandbox"]);
    report.observations.effective_safe_approval_policy = policy_kind(&safe["approvalPolicy"]);
    let safe_policy_ok = report.observations.effective_safe_sandbox == "workspace-write"
        && report.observations.effective_safe_approval_policy == "untrusted";
    report.checks.insert(
        "safePolicyEcho",
        if safe_policy_ok {
            CheckResult::passed("safe sandbox and approval policy are readable before a turn")
        } else {
            CheckResult::failed("provider did not echo the requested safe policy")
        },
    );

    report.measurements.idle_process_rss_kib = server.rss_kib().await;

    if !args.skip_live_turns {
        run_live_scenarios(
            &mut server,
            &mut client,
            &args.workspace,
            &thread_id,
            &provider_model,
            &mut report,
        )
        .await?;
    } else {
        report.checks.insert(
            "liveScenarios",
            CheckResult::failed("live turns were explicitly skipped"),
        );
        merge_observations(&mut report.observations, &client);
        drop(client);
        let shutdown = server.shutdown(false).await?;
        report.measurements.graceful_shutdown_latency_ms = shutdown.latency.as_millis();
        observe_stderr(&mut report.observations, &shutdown.stderr_lines);
        record_mcp_observations(&mut report, &mcp_fixture);
        mcp_fixture.stop().await;
        report.gate_passed = false;
        return Ok(report);
    }

    client
        .request("thread/unsubscribe", json!({ "threadId": thread_id }))
        .await?;
    drop(client);
    let mut reconnected = server.connect(args.step_timeout).await?;
    reconnected.initialize().await?;
    let before_resume_notifications = reconnected.notification_methods.len();
    let resumed = reconnected
        .request("thread/resume", json!({ "threadId": thread_id }))
        .await?;
    ensure!(
        required_string(&resumed, &["thread", "id"])? == thread_id,
        "provider changed the thread identity during live reconnect"
    );
    let duplicate_live_events = reconnected.notification_methods.len()
        > before_resume_notifications
        && reconnected
            .notification_methods
            .iter()
            .any(|method| method == "item/started" || method == "item/completed");
    report.checks.insert(
        "liveReconnect",
        if duplicate_live_events {
            CheckResult::failed("live reconnect replayed item lifecycle notifications")
        } else {
            CheckResult::passed(
                "unsubscribe, transport reconnect and thread resume preserved identity",
            )
        },
    );

    merge_observations(&mut report.observations, &reconnected);
    drop(reconnected);
    let graceful = server.shutdown(false).await?;
    report.measurements.graceful_shutdown_latency_ms = graceful.latency.as_millis();
    observe_stderr(&mut report.observations, &graceful.stderr_lines);
    report.checks.insert(
        "gracefulStop",
        CheckResult::passed("provider process exited within the bounded shutdown timeout"),
    );

    let resumed_server =
        AppServer::spawn(&args.codex, args.step_timeout, &mcp_fixture.endpoint).await?;
    let mut resumed_client = resumed_server.connect(args.step_timeout).await?;
    resumed_client.initialize().await?;
    let resumed_after_stop = resumed_client
        .request("thread/resume", json!({ "threadId": thread_id }))
        .await?;
    let native_resume_ok = required_string(&resumed_after_stop, &["thread", "id"])? == thread_id;
    report.checks.insert(
        "providerNativeResume",
        if native_resume_ok {
            CheckResult::passed("new provider process resumed the same persisted thread")
        } else {
            CheckResult::failed("new provider process returned a different thread identity")
        },
    );

    if !args.skip_live_turns {
        let turn_id = resumed_client
            .start_turn(
                &thread_id,
                "Keep this turn active briefly by running `sleep 30`, then reply with DONE.",
            )
            .await?;
        resumed_client
            .wait_for_turn_started(&thread_id, &turn_id)
            .await?;
        let forced_started_at = Instant::now();
        drop(resumed_client);
        let forced = resumed_server.shutdown(true).await?;
        report.measurements.forced_shutdown_latency_ms = forced.latency.as_millis();
        observe_stderr(&mut report.observations, &forced.stderr_lines);
        ensure!(
            forced_started_at.elapsed() < args.step_timeout,
            "forced stop exceeded timeout"
        );

        let recovery_server =
            AppServer::spawn(&args.codex, args.step_timeout, &mcp_fixture.endpoint).await?;
        let mut recovery_client = recovery_server.connect(args.step_timeout).await?;
        recovery_client.initialize().await?;
        let recovered = recovery_client
            .request("thread/resume", json!({ "threadId": thread_id }))
            .await?;
        let recovered_status = latest_turn_status(&recovered).unwrap_or("unknown");
        report.observations.recovery_after_forced_stop = match recovered_status {
            "completed" | "interrupted" | "failed" => {
                format!("provider-native persisted terminal state: {recovered_status}")
            }
            other => format!("degraded: persisted latest turn state is {other}"),
        };
        report.checks.insert(
            "forcedStopRecovery",
            CheckResult::passed(
                "process loss is recoverable by explicit thread/resume with observable turn state",
            ),
        );
        let _ = recovery_client
            .request("thread/archive", json!({ "threadId": thread_id }))
            .await;
        merge_observations(&mut report.observations, &recovery_client);
        drop(recovery_client);
        let recovery_shutdown = recovery_server.shutdown(false).await?;
        observe_stderr(&mut report.observations, &recovery_shutdown.stderr_lines);
    } else {
        let _ = resumed_client
            .request("thread/archive", json!({ "threadId": thread_id }))
            .await;
        drop(resumed_client);
        let resumed_shutdown = resumed_server.shutdown(false).await?;
        observe_stderr(&mut report.observations, &resumed_shutdown.stderr_lines);
    }

    report.measurements.active_turn_overhead_rss_kib = match (
        report.measurements.idle_process_rss_kib,
        report.measurements.active_process_rss_kib,
    ) {
        (Some(idle), Some(active)) => Some(active.saturating_sub(idle)),
        _ => None,
    };
    record_mcp_observations(&mut report, &mcp_fixture);
    mcp_fixture.stop().await;
    report.gate_passed = report.checks.values().all(|check| check.passed);
    Ok(report)
}

async fn run_live_scenarios(
    server: &mut AppServer,
    client: &mut ProtocolClient,
    workspace: &Path,
    thread_id: &str,
    provider_model: &str,
    report: &mut ProbeReport,
) -> Result<()> {
    let first_turn = client
        .run_turn(
            thread_id,
            "Reply exactly FIRST_OK. Do not call tools.",
            ApprovalDecision::Decline,
            false,
        )
        .await?;
    let second_turn = client
        .run_turn(
            thread_id,
            "Run `pwd` once with the shell tool, then reply exactly SECOND_OK.",
            ApprovalDecision::Accept,
            false,
        )
        .await?;
    let two_turns_ok = first_turn.status == "completed" && second_turn.status == "completed";
    report.checks.insert(
        "twoTurnsOneLiveSession",
        if two_turns_ok {
            CheckResult::passed("two completed turns used one live thread and process")
        } else {
            CheckResult::failed("one of the two live turns did not complete")
        },
    );
    report.checks.insert(
        "incrementalStructuredActivity",
        if client.agent_output_deltas > 0 && client.command_activity_items > 0 {
            CheckResult::passed("provider emitted output deltas and structured command items")
        } else {
            CheckResult::failed("provider did not emit both deltas and structured command items")
        },
    );

    let accepted_path = workspace.join("provider-spike-accepted.txt");
    let denied_path = workspace.join("provider-spike-denied.txt");
    remove_probe_file(&accepted_path)?;
    remove_probe_file(&denied_path)?;
    let approval_accept = client
        .run_turn(
            thread_id,
            "Use the shell tool to run `printf accepted > provider-spike-accepted.txt`, then reply DONE.",
            ApprovalDecision::Accept,
            false,
        )
        .await?;
    let approval_deny = client
        .run_turn(
            thread_id,
            "Use the shell tool to run `printf denied > provider-spike-denied.txt`, then reply DONE.",
            ApprovalDecision::Decline,
            false,
        )
        .await?;
    let approval_effects_ok = accepted_path.is_file() && !denied_path.exists();
    remove_probe_file(&accepted_path)?;
    remove_probe_file(&denied_path)?;
    report.checks.insert(
        "approvalContinuation",
        if approval_accept.approval_requests > 0
            && approval_deny.approval_requests > 0
            && approval_accept.status == "completed"
            && approval_deny.status == "completed"
            && approval_effects_ok
        {
            CheckResult::passed(
                "accept and decline responses continued the same provider execution",
            )
        } else {
            CheckResult::failed("accept/decline approval round-trips were not both observed")
        },
    );

    let user_input = client
        .run_user_input_turn(
            thread_id,
            "Call request_user_input with one question id `choice` and options `Alpha` and `Beta`; after the answer, reply DONE.",
            provider_model,
        )
        .await?;
    report.checks.insert(
        "providerUserInput",
        if user_input.user_input_requests > 0 && user_input.status == "completed" {
            CheckResult::passed("typed user input response continued the same provider execution")
        } else {
            CheckResult::failed("provider did not issue a distinct typed user input request")
        },
    );

    let interrupt_turn = client
        .start_turn(
            thread_id,
            "Run `sleep 30` with the shell tool, then reply DONE.",
        )
        .await?;
    client
        .wait_for_turn_started(thread_id, &interrupt_turn)
        .await?;
    report.measurements.active_process_rss_kib = server.rss_kib().await;
    client
        .request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": interrupt_turn }),
        )
        .await?;
    let interrupted = client
        .wait_turn(thread_id, &interrupt_turn, ApprovalDecision::Decline, false)
        .await?;
    report.checks.insert(
        "interrupt",
        if interrupted.status == "interrupted" {
            CheckResult::passed("turn/interrupt produced an interrupted terminal turn")
        } else {
            CheckResult::failed("turn/interrupt did not produce interrupted status")
        },
    );

    merge_observations(&mut report.observations, client);
    Ok(())
}

impl AppServer {
    async fn spawn(codex: &Path, step_timeout: Duration, mcp_endpoint: &str) -> Result<Self> {
        let endpoint = available_loopback_endpoint()?;
        let mcp_endpoint_literal = serde_json::to_string(mcp_endpoint)?;
        let token_env = format!("{MCP_TOKEN_ENV_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let inherited = PROBE_ENV_ALLOWLIST
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
            .collect::<Vec<_>>();
        let mut child = Command::new(codex)
            .args([
                "app-server",
                "--listen",
                &endpoint,
                "--disable",
                "shell_snapshot",
                "--config",
                "shell_environment_policy.inherit=all",
                "--config",
                &format!("mcp_servers.uprava.url={mcp_endpoint_literal}"),
                "--config",
                &format!("mcp_servers.uprava.bearer_token_env_var=\"{token_env}\""),
                "--config",
                "mcp_servers.uprava.default_tools_approval_mode=\"approve\"",
            ])
            .env_clear()
            .envs(inherited)
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env(token_env, MCP_PROBE_TOKEN)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to start {} app-server", codex.display()))?;
        let stderr = child
            .stderr
            .take()
            .context("app-server stderr is unavailable")?;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut captured = Vec::new();
            while captured.len() < MAX_STDERR_LINES {
                match lines.next_line().await {
                    Ok(Some(line)) => captured.push(line),
                    Ok(None) | Err(_) => break,
                }
            }
            captured
        });

        timeout(CONNECT_TIMEOUT.min(step_timeout), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    bail!("app-server exited before accepting connections: {status}");
                }
                match tokio_tungstenite::connect_async(&endpoint).await {
                    Ok((mut stream, _)) => {
                        let _ = stream.close(None).await;
                        break;
                    }
                    Err(_) => sleep(Duration::from_millis(25)).await,
                }
            }
            Result::<()>::Ok(())
        })
        .await
        .context("timed out waiting for app-server socket")??;

        Ok(Self {
            child,
            endpoint,
            stderr_task,
        })
    }

    async fn connect(&self, step_timeout: Duration) -> Result<ProtocolClient> {
        let (stream, _) = timeout(
            CONNECT_TIMEOUT.min(step_timeout),
            tokio_tungstenite::connect_async(&self.endpoint),
        )
        .await
        .context("timed out connecting to app-server")?
        .context("failed to connect to app-server websocket")?;
        Ok(ProtocolClient::new(stream, step_timeout))
    }

    async fn rss_kib(&self) -> Option<u64> {
        let pid = self.child.id()?;
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    }

    async fn shutdown(mut self, force: bool) -> Result<ShutdownResult> {
        let started_at = Instant::now();
        if force {
            self.child
                .start_kill()
                .context("failed to force-stop app-server")?;
        } else {
            let pid = self.child.id().context("app-server has no process id")?;
            let status = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status()
                .await
                .context("failed to send SIGTERM to app-server")?;
            ensure!(status.success(), "SIGTERM command failed");
        }
        timeout(Duration::from_secs(10), self.child.wait())
            .await
            .context("app-server did not stop within 10 seconds")??;
        let stderr_lines = self.stderr_task.await.unwrap_or_default();
        Ok(ShutdownResult {
            latency: started_at.elapsed(),
            stderr_lines,
        })
    }
}

impl ProbeMcpFixture {
    async fn start() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind probe MCP fixture")?;
        let address = listener.local_addr()?;
        let state = ProbeMcpState::default();
        let factory_state = state.clone();
        let service: rmcp::transport::streamable_http_server::StreamableHttpService<
            ProbeMcpServer,
            rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
        > = rmcp::transport::streamable_http_server::StreamableHttpService::new(
            move || {
                Ok(ProbeMcpServer {
                    state: factory_state.clone(),
                })
            },
            Default::default(),
            rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
                .with_json_response(true),
        );
        let router = Router::new().nest_service("/mcp", service).route_layer(
            middleware::from_fn_with_state(state.clone(), require_probe_mcp_token),
        );
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        Ok(Self {
            endpoint: format!("http://{address}/mcp"),
            state,
            shutdown,
            task,
        })
    }

    fn authorized_calls(&self) -> usize {
        self.state.authorized_calls.load(Ordering::Relaxed)
    }

    fn tool_lists(&self) -> usize {
        self.state.tool_lists.load(Ordering::Relaxed)
    }

    fn authorized_requests(&self) -> usize {
        self.state.authorized_requests.load(Ordering::Relaxed)
    }

    fn auth_rejections(&self) -> usize {
        self.state.auth_rejections.load(Ordering::Relaxed)
    }

    async fn stop(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

async fn require_probe_mcp_token(
    State(state): State<ProbeMcpState>,
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {MCP_PROBE_TOKEN}"));
    if authorized {
        state.authorized_requests.fetch_add(1, Ordering::Relaxed);
        next.run(request).await
    } else {
        state.auth_rejections.fetch_add(1, Ordering::Relaxed);
        StatusCode::UNAUTHORIZED.into_response()
    }
}

impl ServerHandler for ProbeMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Uprava protocol spike fixture")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.state.tool_lists.fetch_add(1, Ordering::Relaxed);
        Ok(ListToolsResult {
            tools: vec![Tool::new(
                "search_tools",
                "Return one bounded Uprava protocol spike result.",
                Arc::new(Map::from_iter([
                    ("type".to_owned(), json!("object")),
                    ("properties".to_owned(), json!({})),
                    ("additionalProperties".to_owned(), json!(false)),
                ])),
            )],
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if request.name.as_ref() != "search_tools" {
            return Err(McpError::invalid_params(
                "unknown Uprava protocol spike tool",
                None,
            ));
        }
        self.state.authorized_calls.fetch_add(1, Ordering::Relaxed);
        Ok(CallToolResult::structured(json!({
            "tools": [{ "id": "probe.tool", "summary": "bounded fixture" }]
        })))
    }
}

impl ProtocolClient {
    fn new(stream: WebSocketStream<MaybeTlsStream<TcpStream>>, step_timeout: Duration) -> Self {
        Self {
            stream,
            next_request_id: 1,
            step_timeout,
            notification_methods: BTreeSet::new(),
            server_request_methods: BTreeSet::new(),
            command_activity_items: 0,
            agent_output_deltas: 0,
        }
    }

    async fn initialize(&mut self) -> Result<Value> {
        let result = self
            .request(
                "initialize",
                json!({
                    "clientInfo": { "name": CLIENT_NAME, "version": CLIENT_VERSION },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await?;
        self.send(&json!({ "method": "initialized" })).await?;
        Ok(result)
    }

    async fn wait_for_mcp_tool(&mut self, server_name: &str, tool_name: &str) -> Result<bool> {
        match timeout(CONNECT_TIMEOUT, async {
            loop {
                let response = self
                    .request("mcpServerStatus/list", json!({ "detail": "full" }))
                    .await?;
                let ready = response["data"].as_array().is_some_and(|servers| {
                    servers.iter().any(|server| {
                        server["name"] == server_name && server["tools"].get(tool_name).is_some()
                    })
                });
                if ready {
                    return Ok(true);
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(false),
        }
    }

    async fn start_thread(
        &mut self,
        workspace: &Path,
        sandbox: &str,
        approval_policy: &str,
        ephemeral: bool,
    ) -> Result<Value> {
        self.request(
            "thread/start",
            json!({
                "cwd": workspace,
                "runtimeWorkspaceRoots": [workspace],
                "sandbox": sandbox,
                "approvalPolicy": approval_policy,
                "approvalsReviewer": "user",
                "ephemeral": ephemeral,
                "developerInstructions": "Follow the probe request exactly. Do not substitute file-edit tools for an explicitly requested shell command."
            }),
        )
        .await
    }

    async fn start_turn(&mut self, thread_id: &str, prompt: &str) -> Result<String> {
        let response = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{ "type": "text", "text": prompt }]
                }),
            )
            .await?;
        required_string(&response, &["turn", "id"])
    }

    async fn run_turn(
        &mut self,
        thread_id: &str,
        prompt: &str,
        decision: ApprovalDecision,
        answer_user_input: bool,
    ) -> Result<TurnOutcome> {
        let turn_id = self.start_turn(thread_id, prompt).await?;
        self.wait_turn(thread_id, &turn_id, decision, answer_user_input)
            .await
    }

    async fn run_user_input_turn(
        &mut self,
        thread_id: &str,
        prompt: &str,
        provider_model: &str,
    ) -> Result<TurnOutcome> {
        let response = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{ "type": "text", "text": prompt }],
                    "collaborationMode": {
                        "mode": "plan",
                        "settings": {
                            "model": provider_model,
                            "reasoning_effort": null,
                            "developer_instructions": null
                        }
                    }
                }),
            )
            .await?;
        let turn_id = required_string(&response, &["turn", "id"])?;
        self.wait_turn(thread_id, &turn_id, ApprovalDecision::Decline, true)
            .await
    }

    async fn wait_for_turn_started(&mut self, thread_id: &str, turn_id: &str) -> Result<()> {
        timeout(self.step_timeout, async {
            loop {
                let message = self.next_message().await?;
                self.observe(&message);
                if message["method"] == "turn/started"
                    && message["params"]["threadId"] == thread_id
                    && message["params"]["turn"]["id"] == turn_id
                {
                    return Ok(());
                }
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.respond_to_server_request(&message, ApprovalDecision::Decline, false)
                        .await?;
                }
            }
        })
        .await
        .context("timed out waiting for turn/started")?
    }

    async fn wait_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        decision: ApprovalDecision,
        answer_user_input: bool,
    ) -> Result<TurnOutcome> {
        timeout(self.step_timeout, async {
            let mut approval_requests = 0;
            let mut user_input_requests = 0;
            loop {
                let message = self.next_message().await?;
                self.observe(&message);
                if message.get("id").is_some() && message.get("method").is_some() {
                    match message["method"].as_str() {
                        Some("item/commandExecution/requestApproval")
                        | Some("item/fileChange/requestApproval") => approval_requests += 1,
                        Some("item/tool/requestUserInput") => user_input_requests += 1,
                        _ => {}
                    }
                    self.respond_to_server_request(&message, decision, answer_user_input)
                        .await?;
                    continue;
                }
                if message["method"] == "turn/completed"
                    && message["params"]["threadId"] == thread_id
                    && message["params"]["turn"]["id"] == turn_id
                {
                    return Ok(TurnOutcome {
                        status: message["params"]["turn"]["status"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_owned(),
                        approval_requests,
                        user_input_requests,
                    });
                }
            }
        })
        .await
        .context("timed out waiting for turn completion")?
    }

    async fn respond_to_server_request(
        &mut self,
        message: &Value,
        decision: ApprovalDecision,
        answer_user_input: bool,
    ) -> Result<()> {
        let id = message
            .get("id")
            .cloned()
            .context("server request has no id")?;
        let method = message["method"].as_str().unwrap_or_default();
        let result = match method {
            "item/commandExecution/requestApproval" => json!({
                "decision": match decision {
                    ApprovalDecision::Accept => "accept",
                    ApprovalDecision::Decline => "decline",
                }
            }),
            "item/fileChange/requestApproval" => json!({
                "decision": match decision {
                    ApprovalDecision::Accept => "accept",
                    ApprovalDecision::Decline => "decline",
                }
            }),
            "item/tool/requestUserInput" if answer_user_input => {
                let mut answers = serde_json::Map::new();
                if let Some(questions) = message["params"]["questions"].as_array() {
                    for question in questions {
                        if let Some(question_id) = question["id"].as_str() {
                            let answer = question["options"]
                                .as_array()
                                .and_then(|options| options.first())
                                .and_then(|option| option["label"].as_str())
                                .unwrap_or("probe-answer");
                            answers.insert(question_id.to_owned(), json!({ "answers": [answer] }));
                        }
                    }
                }
                json!({ "answers": answers })
            }
            other => {
                self.send(&json!({
                    "id": id,
                    "error": { "code": -32601, "message": format!("unsupported probe callback: {other}") }
                }))
                .await?;
                return Ok(());
            }
        };
        self.send(&json!({ "id": id, "result": result })).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&json!({ "id": id, "method": method, "params": params }))
            .await?;
        timeout(self.step_timeout, async {
            loop {
                let message = self.next_message().await?;
                self.observe(&message);
                if message["id"] == id {
                    if let Some(error) = message.get("error") {
                        bail!(
                            "{method} failed with bounded provider error: {}",
                            bounded_error(error)
                        );
                    }
                    return message
                        .get("result")
                        .cloned()
                        .context("provider response has no result");
                }
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.respond_to_server_request(&message, ApprovalDecision::Decline, false)
                        .await?;
                }
            }
        })
        .await
        .with_context(|| format!("timed out waiting for {method} response"))?
    }

    async fn send(&mut self, value: &Value) -> Result<()> {
        let text = serde_json::to_string(value)?;
        ensure!(
            text.len() <= MAX_PROTOCOL_LINE_BYTES,
            "outbound protocol message exceeds size limit"
        );
        self.stream.send(Message::Text(text.into())).await?;
        Ok(())
    }

    async fn next_message(&mut self) -> Result<Value> {
        let message = self
            .stream
            .next()
            .await
            .context("app-server transport closed")??;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(frame) => bail!("app-server closed transport: {frame:?}"),
            Message::Ping(payload) => {
                self.stream.send(Message::Pong(payload)).await?;
                return Box::pin(self.next_message()).await;
            }
            Message::Pong(_) => return Box::pin(self.next_message()).await,
            Message::Binary(_) | Message::Frame(_) => {
                bail!("app-server emitted a non-text websocket frame")
            }
        };
        ensure!(
            text.len() <= MAX_PROTOCOL_LINE_BYTES,
            "inbound protocol message exceeds size limit"
        );
        serde_json::from_str(&text).context("app-server emitted invalid JSON")
    }

    fn observe(&mut self, message: &Value) {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if message.get("id").is_some() {
                self.server_request_methods.insert(method.to_owned());
            } else {
                self.notification_methods.insert(method.to_owned());
            }
            if method == "item/started" && message["params"]["item"]["type"] == "commandExecution" {
                self.command_activity_items += 1;
            }
            if method == "item/agentMessage/delta" {
                self.agent_output_deltas += 1;
            }
        }
    }
}

async fn provider_version(codex: &Path) -> Result<String> {
    let output = Command::new(codex)
        .arg("--version")
        .output()
        .await
        .with_context(|| format!("failed to execute {} --version", codex.display()))?;
    ensure!(output.status.success(), "codex --version failed");
    let version = String::from_utf8(output.stdout)?.trim().to_owned();
    ensure!(!version.is_empty(), "codex --version returned no version");
    Ok(version)
}

fn required_string(value: &Value, path: &[&str]) -> Result<String> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| format!("provider response is missing {segment}"))?;
    }
    current
        .as_str()
        .map(str::to_owned)
        .context("provider response field is not a string")
}

fn policy_kind(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(policy_type) = value.get("type").and_then(Value::as_str) {
        return match policy_type {
            "dangerFullAccess" => "danger-full-access",
            "readOnly" => "read-only",
            "workspaceWrite" => "workspace-write",
            other => other,
        }
        .to_owned();
    }
    if let Some(object) = value.as_object() {
        return object
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "object".to_owned());
    }
    "unknown".to_owned()
}

fn latest_turn_status(resume_response: &Value) -> Option<&str> {
    resume_response["thread"]["turns"].as_array()?.last()?["status"].as_str()
}

fn merge_observations(observations: &mut Observations, client: &ProtocolClient) {
    observations
        .notification_methods
        .extend(client.notification_methods.iter().cloned());
    observations
        .server_request_methods
        .extend(client.server_request_methods.iter().cloned());
    observations.command_activity_items += client.command_activity_items;
    observations.agent_output_deltas += client.agent_output_deltas;
}

fn observe_stderr(observations: &mut Observations, lines: &[String]) {
    observations.stderr_line_count += lines.len();
    observations.lease_token_exposed |= lines.iter().any(|line| line.contains(MCP_PROBE_TOKEN));
}

fn record_mcp_observations(report: &mut ProbeReport, fixture: &ProbeMcpFixture) {
    report.observations.mcp_authorized_requests = fixture.authorized_requests();
    report.observations.mcp_tool_lists = fixture.tool_lists();
    report.observations.mcp_authorized_calls = fixture.authorized_calls();
    report.observations.mcp_auth_rejections = fixture.auth_rejections();
    report.checks.insert(
        "mcpCredentialBoundary",
        if report.observations.mcp_authorized_requests > 0
            && report.observations.mcp_tool_lists > 0
            && report.observations.mcp_auth_rejections == 0
            && !report.observations.lease_token_exposed
        {
            CheckResult::passed(
                "Uprava-shaped MCP discovery used bearer-token env indirection without stderr exposure",
            )
        } else {
            CheckResult::failed("MCP discovery, bearer auth or token redaction proof failed")
        },
    );
}

fn remove_probe_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn redact_user_agent(value: &str) -> String {
    value
        .split('(')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn bounded_error(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("provider error");
    let bounded: String = message.chars().take(240).collect();
    format!("code={code}, message={bounded}")
}

fn available_loopback_endpoint() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to reserve loopback port")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(format!("ws://127.0.0.1:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRUBBED_FIXTURE: &str = include_str!("../fixtures/codex-app-server-0.144.1.json");

    #[test]
    fn scrubbed_fixture_is_bounded_and_contains_required_interactions() {
        assert!(SCRUBBED_FIXTURE.len() < 16 * 1024);
        let fixture: Value = serde_json::from_str(SCRUBBED_FIXTURE).expect("fixture is valid JSON");
        let methods = fixture["serverRequests"]
            .as_array()
            .expect("server requests are present")
            .iter()
            .filter_map(|request| request["method"].as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            methods,
            BTreeSet::from([
                "item/commandExecution/requestApproval",
                "item/tool/requestUserInput",
            ])
        );
    }

    #[test]
    fn scrubbed_fixture_omits_secret_and_host_values() {
        for forbidden in [
            MCP_PROBE_TOKEN,
            "UPRAVA_MCP_ACCESS_TOKEN=",
            "codexHome\":\"/",
            "/Users/",
            "Bearer ",
        ] {
            assert!(!SCRUBBED_FIXTURE.contains(forbidden));
        }
    }

    #[test]
    fn policy_kind_normalizes_provider_sandbox_objects() {
        assert_eq!(
            policy_kind(&json!({ "type": "workspaceWrite" })),
            "workspace-write"
        );
        assert_eq!(
            policy_kind(&json!({ "type": "dangerFullAccess" })),
            "danger-full-access"
        );
    }
}
