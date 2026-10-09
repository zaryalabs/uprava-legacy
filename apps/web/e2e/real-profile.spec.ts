import { expect, test, type APIRequestContext } from "@playwright/test";

const realApiEnabled = process.env.UPRAVA_E2E_REAL_API === "1";
const coreUrl = process.env.UPRAVA_E2E_CORE_URL ?? "http://127.0.0.1:8080";
const expectedNode = process.env.UPRAVA_E2E_EXPECTED_NODE ?? "Compose Node";
const workspacePath = process.env.UPRAVA_E2E_WORKSPACE_PATH ?? "/workspace";
const provider = process.env.UPRAVA_E2E_PROVIDER ?? "codex";
const webPassword =
  process.env.UPRAVA_E2E_WEB_PASSWORD ?? "uprava-smoke-password";
const sessionTitle =
  process.env.UPRAVA_E2E_SESSION_TITLE ?? "Playwright real profile";
const turnContent =
  process.env.UPRAVA_E2E_TURN_CONTENT ??
  "Reply exactly UPRAVA_CODEX_SMOKE_OK. Do not modify files.";
const expectedAssistantContent =
  process.env.UPRAVA_E2E_EXPECTED_ASSISTANT_CONTENT ?? "UPRAVA_CODEX_SMOKE_OK";
const turnTimeoutMs = Number(process.env.UPRAVA_E2E_TURN_TIMEOUT_MS ?? "30000");
const lifecycleEnabled = process.env.UPRAVA_E2E_LIFECYCLE === "1";
const execCompatibilityEnabled =
  process.env.UPRAVA_E2E_EXEC_COMPATIBILITY === "1";
const managedAcceptanceEnabled =
  process.env.UPRAVA_E2E_MANAGED_ACCEPTANCE === "1";
const testTimeoutMs = Number(
  process.env.UPRAVA_E2E_TEST_TIMEOUT_MS ??
    String(
      Math.max(
        30_000,
        turnTimeoutMs *
          (1 +
            Number(lifecycleEnabled) +
            Number(execCompatibilityEnabled) +
            5 * Number(managedAcceptanceEnabled)) +
          45_000,
      ),
    ),
);

test.describe("real local profile", () => {
  test.skip(
    !realApiEnabled,
    "set UPRAVA_E2E_REAL_API=1 to run against a live Core/Web/Node profile",
  );

  test("creates a provider session through Core and renders it in Web", async ({
    page,
  }) => {
    test.setTimeout(testTimeoutMs);
    const request = page.context().request;
    const csrfToken = await authenticate(request);
    const placementId = await waitForPlacementId(request);
    const session = await createProviderSession(
      request,
      placementId,
      csrfToken,
    );
    expect(runtimeProfile(session)).toBe("managed");
    expect(runtimePolicyField(session, "sandbox_mode")).toBe("workspace-write");
    expect(runtimePolicyField(session, "approval_mode")).toBe("untrusted");
    const sessionId = isRecord(session.session)
      ? stringField(session.session, "session_thread_id")
      : "";
    expect(sessionId).not.toBe("");
    const sessionUrl = `${coreUrl}/api/v1/sessions/${encodeURIComponent(sessionId)}`;

    await waitForRuntimeState(request, sessionUrl, "ready");
    await postJson(
      request,
      `${sessionUrl}/turns`,
      { content: turnContent },
      csrfToken,
    );
    await waitForAssistantContent(
      request,
      sessionUrl,
      expectedAssistantContent,
    );
    await waitForRuntimeState(request, sessionUrl, "ready");

    if (managedAcceptanceEnabled) {
      const runtimeId = runtimeIdFromDetail(session);
      expect(runtimeId).not.toBe("");
      await exerciseManagedAcceptance(
        request,
        sessionUrl,
        sessionId,
        runtimeId,
        csrfToken,
      );
    }

    await page.goto(`/sessions/${sessionId}`);
    await expect
      .poll(() => new URL(page.url()).pathname)
      .toBe(
        `/workspaces/${encodeURIComponent(placementId)}/agent/${encodeURIComponent(sessionId)}`,
      );
    await expect(
      page.getByRole("heading", { name: sessionTitle }),
    ).toBeVisible();
    const main = page.getByRole("main");
    await expect(
      main.getByText(expectedAssistantContent, { exact: true }),
    ).toBeVisible();
    await main.getByText("Session details").click();
    await expect(
      main.getByRole("heading", { name: "Evidence Projection" }),
    ).toBeVisible();
    await expect(
      main
        .getByRole("article", { name: sessionTitle })
        .getByText("SESSION / managed / ready", { exact: true }),
    ).toBeVisible();

    if (lifecycleEnabled) {
      await page.getByRole("button", { name: "Detach" }).click();
      await waitForSessionState(request, sessionUrl, "detached");
      await expect(page.getByRole("button", { name: "Attach" })).toBeEnabled();
      await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();

      await page.getByRole("button", { name: "Attach" }).click();
      await waitForSessionState(request, sessionUrl, "active");
      await expect(page.getByRole("button", { name: "Detach" })).toBeEnabled();

      await page.getByRole("button", { name: "Stop" }).click();
      await waitForRuntimeState(request, sessionUrl, "stopped");
      await waitForSessionState(request, sessionUrl, "stopped");
      await expect(page.getByRole("button", { name: "Resume" })).toBeEnabled();

      await page.getByRole("button", { name: "Resume" }).click();
      await waitForRuntimeState(request, sessionUrl, "ready");
      await waitForSessionState(request, sessionUrl, "active");
      await expect(page.getByRole("button", { name: "Detach" })).toBeEnabled();

      const postResumeTurn = `${turnContent} after browser resume`;
      const postResumeAssistant =
        process.env.UPRAVA_E2E_POST_RESUME_EXPECTED_ASSISTANT_CONTENT ??
        expectedAssistantContent;
      await page.getByPlaceholder("Send a turn").fill(postResumeTurn);
      await page.getByRole("button", { name: "Send" }).click();
      await waitForAssistantContent(request, sessionUrl, postResumeAssistant);
      await waitForRuntimeState(request, sessionUrl, "ready");
      await expect(
        main.getByText(postResumeAssistant, { exact: true }),
      ).toBeVisible();

      await page.reload();
      await expect(
        page.getByRole("heading", { name: sessionTitle }),
      ).toBeVisible();
      await expect(
        page.getByRole("main").getByText(postResumeAssistant, { exact: true }),
      ).toBeVisible();
      await page.getByRole("main").getByText("Session details").click();
      await expect(
        page
          .getByRole("main")
          .getByRole("heading", { name: "Evidence Projection" }),
      ).toBeVisible();
    }

    if (execCompatibilityEnabled) {
      const compatibility = await createProviderSession(
        request,
        placementId,
        csrfToken,
        "exec_compatibility",
        `${sessionTitle} · Exec compatibility`,
      );
      expect(runtimeProfile(compatibility)).toBe("exec_compatibility");
      const compatibilityId = isRecord(compatibility.session)
        ? stringField(compatibility.session, "session_thread_id")
        : "";
      expect(compatibilityId).not.toBe("");
      const compatibilityUrl = `${coreUrl}/api/v1/sessions/${encodeURIComponent(compatibilityId)}`;
      await waitForRuntimeState(request, compatibilityUrl, "ready");
      await postJson(
        request,
        `${compatibilityUrl}/turns`,
        { content: turnContent },
        csrfToken,
      );
      await waitForAssistantContent(
        request,
        compatibilityUrl,
        expectedAssistantContent,
      );
      await waitForRuntimeState(request, compatibilityUrl, "ready");
    }
  });
});

async function authenticate(request: APIRequestContext) {
  const status = await getJson(request, `${coreUrl}/api/v1/auth/status`);
  if (!isRecord(status) || status.auth_required !== true) {
    return "";
  }
  if (status.authenticated === true) {
    const csrfToken = await csrfTokenFromStorage(request);
    if (csrfToken) {
      return csrfToken;
    }
  }
  const authPath =
    status.setup_required === true
      ? `${coreUrl}/api/v1/auth/setup`
      : `${coreUrl}/api/v1/auth/login`;
  const auth = await postJson(request, authPath, { password: webPassword });
  const csrfToken = isRecord(auth) ? stringField(auth, "csrf_token") : "";
  expect(csrfToken).not.toBe("");
  return csrfToken;
}

async function csrfTokenFromStorage(request: APIRequestContext) {
  const storage = await request.storageState();
  return (
    storage.cookies.find((cookie) => cookie.name === "uprava_csrf")?.value ?? ""
  );
}

async function waitForPlacementId(request: APIRequestContext) {
  let lastInventory: unknown = null;
  await expect
    .poll(
      async () => {
        const response = await request.get(`${coreUrl}/api/v1/inventory`);
        if (!response.ok()) return "";
        lastInventory = await response.json();
        return placementIdFromInventory(lastInventory);
      },
      {
        timeout: 60_000,
        intervals: [500, 1_000, 2_000],
        message: `waiting for ${expectedNode} validated placement at ${workspacePath}`,
      },
    )
    .not.toBe("");

  return placementIdFromInventory(lastInventory);
}

function placementIdFromInventory(value: unknown) {
  if (
    !isRecord(value) ||
    !Array.isArray(value.nodes) ||
    !Array.isArray(value.placements)
  ) {
    return "";
  }
  const node = value.nodes.find(
    (candidate) =>
      isRecord(candidate) &&
      stringField(candidate, "display_name") === expectedNode,
  );
  if (!isRecord(node)) return "";
  const nodeId = stringField(node, "node_id");
  const placement = value.placements.find(
    (candidate) =>
      isRecord(candidate) &&
      stringField(candidate, "node_id") === nodeId &&
      stringField(candidate, "workspace_path") === workspacePath &&
      stringField(candidate, "state") === "validated",
  );
  return isRecord(placement)
    ? stringField(placement, "project_placement_id")
    : "";
}

async function createProviderSession(
  request: APIRequestContext,
  placementId: string,
  csrfToken: string,
  executionProfile?: "managed" | "exec_compatibility",
  title = sessionTitle,
) {
  return postJson(
    request,
    `${coreUrl}/api/v1/sessions`,
    {
      project_placement_id: placementId,
      title,
      provider,
      ...(executionProfile ? { execution_profile: executionProfile } : {}),
    },
    csrfToken,
  );
}

function runtimeProfile(value: unknown) {
  if (
    !isRecord(value) ||
    !isRecord(value.session) ||
    !isRecord(value.session.runtime)
  ) {
    return "";
  }
  return stringField(value.session.runtime, "execution_profile");
}

function runtimePolicyField(value: unknown, field: string) {
  if (
    !isRecord(value) ||
    !isRecord(value.session) ||
    !isRecord(value.session.runtime) ||
    !isRecord(value.session.runtime.effective_policy)
  ) {
    return "";
  }
  return stringField(value.session.runtime.effective_policy, field);
}

function runtimeIdFromDetail(value: unknown) {
  if (
    !isRecord(value) ||
    !isRecord(value.session) ||
    !isRecord(value.session.runtime)
  ) {
    return "";
  }
  return stringField(value.session.runtime, "runtime_session_id");
}

async function exerciseManagedAcceptance(
  request: APIRequestContext,
  sessionUrl: string,
  sessionId: string,
  runtimeId: string,
  csrfToken: string,
) {
  await postJson(
    request,
    `${sessionUrl}/turns`,
    {
      content:
        "Use the Uprava MCP search_tools tool with query `workspace`. Then reply exactly UPRAVA_MCP_OK.",
    },
    csrfToken,
  );
  await waitForAssistantContent(request, sessionUrl, "UPRAVA_MCP_OK");
  await waitForRuntimeState(request, sessionUrl, "ready");
  await expect
    .poll(
      async () => {
        const detail = await getJson(request, sessionUrl);
        return JSON.stringify(detail).includes("search_tools");
      },
      { timeout: turnTimeoutMs, intervals: [500, 1_000, 2_000] },
    )
    .toBe(true);

  await postJson(
    request,
    `${sessionUrl}/turns`,
    {
      content:
        "Use the shell tool to run `printf accepted > uprava-acceptance-approved.txt`. After the tool attempt completes, reply exactly UPRAVA_APPROVAL_ALLOWED.",
    },
    csrfToken,
  );
  const approved = await waitForPendingInteraction(
    request,
    sessionUrl,
    "approval",
  );
  await postJson(
    request,
    `${coreUrl}/api/v1/sessions/${encodeURIComponent(sessionId)}/provider-interactions/${encodeURIComponent(approved)}/approval`,
    { approved: true, message: "host acceptance approve" },
    csrfToken,
  );
  await waitForAssistantContent(request, sessionUrl, "UPRAVA_APPROVAL_ALLOWED");
  await waitForRuntimeState(request, sessionUrl, "ready");

  await postJson(
    request,
    `${sessionUrl}/turns`,
    {
      content:
        "Use the shell tool to run `printf denied > uprava-acceptance-denied.txt`. After the tool attempt completes, reply exactly UPRAVA_APPROVAL_DENIED.",
    },
    csrfToken,
  );
  const denied = await waitForPendingInteraction(
    request,
    sessionUrl,
    "approval",
  );
  await postJson(
    request,
    `${coreUrl}/api/v1/sessions/${encodeURIComponent(sessionId)}/provider-interactions/${encodeURIComponent(denied)}/approval`,
    { approved: false, message: "host acceptance deny" },
    csrfToken,
  );
  await waitForAssistantContent(request, sessionUrl, "UPRAVA_APPROVAL_DENIED");
  await waitForRuntimeState(request, sessionUrl, "ready");

  await postJson(
    request,
    `${sessionUrl}/turns`,
    {
      content:
        "Use the request_user_input tool to ask one question with id `acceptance` and options Alpha and Beta. After the answer, reply exactly UPRAVA_INPUT_OK.",
      collaboration_mode: "plan",
    },
    csrfToken,
  );
  const input = await waitForPendingInteraction(
    request,
    sessionUrl,
    "user_input",
  );
  await postJson(
    request,
    `${coreUrl}/api/v1/sessions/${encodeURIComponent(sessionId)}/provider-interactions/${encodeURIComponent(input)}/input`,
    { answers: ["Alpha"] },
    csrfToken,
  );
  await waitForAssistantContent(request, sessionUrl, "UPRAVA_INPUT_OK");
  await waitForRuntimeState(request, sessionUrl, "ready");

  await postJson(
    request,
    `${sessionUrl}/turns`,
    {
      content:
        "Run the shell command `sleep 30`, wait for it, then reply UPRAVA_INTERRUPT_MISSED.",
    },
    csrfToken,
  );
  const sleepApproval = await waitForPendingInteraction(
    request,
    sessionUrl,
    "approval",
  );
  await postJson(
    request,
    `${coreUrl}/api/v1/sessions/${encodeURIComponent(sessionId)}/provider-interactions/${encodeURIComponent(sleepApproval)}/approval`,
    { approved: true, message: "host acceptance interrupt" },
    csrfToken,
  );
  await waitForRuntimeState(request, sessionUrl, "running");
  await postJson(
    request,
    `${coreUrl}/api/v1/runtime-sessions/${encodeURIComponent(runtimeId)}/interrupt`,
    undefined,
    csrfToken,
  );
  await waitForRuntimeState(request, sessionUrl, "ready");
  await expect
    .poll(
      async () =>
        JSON.stringify(await getJson(request, sessionUrl)).includes(
          "turn.interrupted",
        ),
      { timeout: turnTimeoutMs, intervals: [500, 1_000, 2_000] },
    )
    .toBe(true);
}

async function waitForPendingInteraction(
  request: APIRequestContext,
  sessionUrl: string,
  kind: "approval" | "user_input",
) {
  let interactionId = "";
  await expect
    .poll(
      async () => {
        const detail = await getJson(request, sessionUrl);
        interactionId = pendingInteractionId(detail, kind);
        return interactionId;
      },
      { timeout: turnTimeoutMs, intervals: [250, 500, 1_000] },
    )
    .not.toBe("");
  return interactionId;
}

function pendingInteractionId(value: unknown, kind: string) {
  if (!isRecord(value) || !Array.isArray(value.pending_interactions)) {
    return "";
  }
  const interaction = value.pending_interactions.find(
    (candidate) =>
      isRecord(candidate) &&
      stringField(candidate, "kind") === kind &&
      stringField(candidate, "state") === "requested",
  );
  return isRecord(interaction)
    ? stringField(interaction, "provider_interaction_id")
    : "";
}

async function waitForRuntimeState(
  request: APIRequestContext,
  sessionUrl: string,
  expectedState: string,
) {
  await expect
    .poll(
      async () => {
        const detail = await getJson(request, sessionUrl);
        return runtimeState(detail);
      },
      { timeout: 30_000, intervals: [500, 1_000, 2_000] },
    )
    .toBe(expectedState);
}

async function waitForSessionState(
  request: APIRequestContext,
  sessionUrl: string,
  expectedState: string,
) {
  await expect
    .poll(
      async () => {
        const detail = await getJson(request, sessionUrl);
        return sessionState(detail);
      },
      { timeout: 30_000, intervals: [500, 1_000, 2_000] },
    )
    .toBe(expectedState);
}

async function waitForAssistantContent(
  request: APIRequestContext,
  sessionUrl: string,
  expectedContent: string,
) {
  await expect
    .poll(
      async () => {
        const detail = await getJson(request, sessionUrl);
        return hasAssistantContent(detail, expectedContent);
      },
      { timeout: turnTimeoutMs, intervals: [500, 1_000, 2_000] },
    )
    .toBe(true);
}

async function getJson(request: APIRequestContext, url: string) {
  const response = await request.get(url);
  expect(response.ok()).toBe(true);
  return response.json() as Promise<unknown>;
}

async function postJson(
  request: APIRequestContext,
  url: string,
  body: unknown,
  csrfToken = "",
) {
  const response = await request.post(url, {
    data: body,
    headers: csrfToken ? { "x-uprava-csrf": csrfToken } : undefined,
  });
  expect(response.ok()).toBe(true);
  return response.json() as Promise<Record<string, unknown>>;
}

function runtimeState(value: unknown) {
  if (
    !isRecord(value) ||
    !isRecord(value.session) ||
    !isRecord(value.session.runtime)
  ) {
    return "";
  }
  return stringField(value.session.runtime, "state");
}

function sessionState(value: unknown) {
  if (!isRecord(value) || !isRecord(value.session)) {
    return "";
  }
  return stringField(value.session, "state");
}

function hasAssistantContent(value: unknown, expectedContent: string) {
  if (!isRecord(value) || !Array.isArray(value.messages)) return false;
  return value.messages.some(
    (message) =>
      isRecord(message) &&
      stringField(message, "role") === "assistant" &&
      stringField(message, "content").includes(expectedContent),
  );
}

function stringField(value: Record<string, unknown>, field: string) {
  const fieldValue = value[field];
  return typeof fieldValue === "string" ? fieldValue : "";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
