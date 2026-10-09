//! Uprava MCP Streamable HTTP adapter over Core application services.

use std::sync::Arc;

use axum::http::request::Parts;
use axum::response::Response;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use uprava_protocol::{
    ExecuteToolRequest, InspectToolRequest, McpAccessLeaseClaims, SearchToolsRequest, ToolCallState,
};

use super::super::*;

pub(crate) const UPRAVA_MCP_PATH: &str = "/mcp";

#[derive(Clone)]
pub(crate) struct UpravaMcpServer {
    state: Arc<AppState>,
}

pub(crate) async fn require_mcp_lease(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let access_token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty());
    let Some(access_token) = access_token else {
        state
            .core_metrics
            .mcp_lease_rejections
            .fetch_add(1, Ordering::Relaxed);
        return AppError::auth("mcp_lease.missing", "Missing Uprava MCP lease").into_response();
    };
    match validate_mcp_access_lease(&state, access_token).await {
        Ok(claims) => {
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Err(error) => {
            state
                .core_metrics
                .mcp_lease_rejections
                .fetch_add(1, Ordering::Relaxed);
            let presented_lease_id = access_token
                .split_once('.')
                .map(|(lease_id, _)| lease_id)
                .unwrap_or("invalid");
            tracing::warn!(
                error_code = ?error.code,
                error_message = %error.message,
                presented_lease_id,
                "Uprava MCP lease rejected"
            );
            AppError::auth("mcp_lease.invalid", "Invalid or expired Uprava MCP lease")
                .into_response()
        }
    }
}

impl UpravaMcpServer {
    pub(crate) fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

impl ServerHandler for UpravaMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use search_tools, then inspect_tool, then execute_tool. Search never returns full tool schemas.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let _ = lease_claims(&context)?;
        Ok(ListToolsResult {
            tools: meta_tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let claims = lease_claims(&context)?;
        match request.name.as_ref() {
            "search_tools" => {
                let request: SearchToolsRequest = parse_arguments(request.arguments)?;
                require_lease_scope(&claims, &request.scope)?;
                match search_tools(&self.state, &request).await {
                    Ok(response) => structured(response),
                    Err(error) => Ok(application_error_result(error)),
                }
            }
            "inspect_tool" => {
                let request: InspectToolRequest = parse_arguments(request.arguments)?;
                require_lease_scope(&claims, &request.scope)?;
                match inspect_tool(&self.state, &request).await {
                    Ok(response) => structured(response),
                    Err(error) => Ok(application_error_result(error)),
                }
            }
            "execute_tool" => {
                let request: ExecuteToolRequest = parse_arguments(request.arguments)?;
                require_lease_scope(&claims, &request.scope)?;
                match execute_tool(&self.state, &request).await {
                    Ok(response) if response.state == ToolCallState::Completed => {
                        structured(response)
                    }
                    Ok(response) => serde_json::to_value(response)
                        .map(CallToolResult::structured_error)
                        .map_err(|error| {
                            McpError::internal_error(
                                "Failed to serialize tool response",
                                Some(json!({"error": error.to_string()})),
                            )
                        }),
                    Err(error) => Ok(application_error_result(error)),
                }
            }
            _ => Err(McpError::invalid_params(
                "Unknown Uprava MCP meta-tool",
                None,
            )),
        }
    }
}

fn lease_claims(context: &RequestContext<RoleServer>) -> Result<McpAccessLeaseClaims, McpError> {
    context
        .extensions
        .get::<Parts>()
        .and_then(|parts| parts.extensions.get::<McpAccessLeaseClaims>())
        .cloned()
        .ok_or_else(|| McpError::invalid_request("Missing validated Uprava MCP lease", None))
}

fn require_lease_scope(
    claims: &McpAccessLeaseClaims,
    scope: &uprava_protocol::ToolScope,
) -> Result<(), McpError> {
    if lease_scope_matches(claims, scope) {
        Ok(())
    } else {
        Err(McpError::invalid_params(
            "Tool scope does not match the MCP lease",
            None,
        ))
    }
}

fn parse_arguments<T: DeserializeOwned>(
    arguments: Option<Map<String, Value>>,
) -> Result<T, McpError> {
    serde_json::from_value(Value::Object(arguments.unwrap_or_default())).map_err(|error| {
        McpError::invalid_params(
            "Invalid Uprava MCP tool arguments",
            Some(json!({"error": error.to_string()})),
        )
    })
}

fn structured(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    serde_json::to_value(value)
        .map(CallToolResult::structured)
        .map_err(|error| {
            McpError::internal_error(
                "Failed to serialize tool response",
                Some(json!({"error": error.to_string()})),
            )
        })
}

fn application_error_result(error: AppError) -> CallToolResult {
    let (code, message, retryable) = match error {
        AppError::NotFound { code, message } | AppError::BadRequest { code, message } => {
            (code, message, false)
        }
        AppError::Auth { code, message } => (code, message, false),
        AppError::RateLimited { code, message } => (code, message, true),
        other => {
            tracing::error!(error = %other, "Uprava MCP application error");
            (
                "internal.error",
                "Core tool operation failed".to_owned(),
                true,
            )
        }
    };
    CallToolResult::structured_error(json!({
        "error_code": code,
        "message": message,
        "retryable": retryable,
    }))
}

fn meta_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "search_tools",
            "Search effective tools in the current scope without returning full tool schemas.",
            Arc::new(search_tools_schema()),
        ),
        Tool::new(
            "inspect_tool",
            "Inspect exactly one current tool definition and its effective availability.",
            Arc::new(inspect_tool_schema()),
        ),
        Tool::new(
            "execute_tool",
            "Execute one inspected tool after fresh scope, policy, schema, approval and availability checks.",
            Arc::new(execute_tool_schema()),
        ),
    ]
}

fn scope_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "actor_ref": { "type": "object" },
            "node_id": { "type": ["string", "null"] },
            "project_id": { "type": ["string", "null"] },
            "project_placement_id": { "type": ["string", "null"] },
            "session_thread_id": { "type": ["string", "null"] }
        },
        "required": ["actor_ref", "node_id", "project_id", "project_placement_id", "session_thread_id"],
        "additionalProperties": false
    })
}

fn search_tools_schema() -> Map<String, Value> {
    object_schema(
        json!({
            "scope": scope_schema(),
            "query": { "type": "string", "maxLength": 512 },
            "filters": {
                "type": "object",
                "properties": {
                    "source_kinds": { "type": "array", "items": { "enum": ["uprava_native", "external_mcp", "plugin"] } },
                    "risk_levels": { "type": "array", "items": { "type": "string" } },
                    "availability_states": { "type": "array", "items": { "enum": ["available", "unavailable", "degraded", "approval_required"] } }
                },
                "additionalProperties": false
            },
            "cursor": { "type": ["string", "null"] },
            "limit": { "type": ["integer", "null"], "minimum": 1, "maximum": 25 }
        }),
        &["scope", "query"],
    )
}

fn inspect_tool_schema() -> Map<String, Value> {
    object_schema(
        json!({
            "scope": scope_schema(),
            "tool_id": { "type": "string", "minLength": 1 }
        }),
        &["scope", "tool_id"],
    )
}

fn execute_tool_schema() -> Map<String, Value> {
    object_schema(
        json!({
            "scope": scope_schema(),
            "tool_id": { "type": "string", "minLength": 1 },
            "arguments": { "type": "object" }
        }),
        &["scope", "tool_id", "arguments"],
    )
}

fn object_schema(properties: Value, required: &[&str]) -> Map<String, Value> {
    let Value::Object(properties) = properties else {
        return Map::new();
    };
    let Value::Object(schema) = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    }) else {
        return Map::new();
    };
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_visible_surface_contains_only_three_meta_tools() {
        let names = meta_tools()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["search_tools", "inspect_tool", "execute_tool"]);
    }
}
