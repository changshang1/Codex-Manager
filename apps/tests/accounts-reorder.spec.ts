import { expect, test, type Page } from "@playwright/test";

const SETTINGS_SNAPSHOT = {
  updateAutoCheck: true,
  closeToTrayOnClose: false,
  closeToTraySupported: false,
  lowTransparency: false,
  lightweightModeOnCloseToTray: false,
  codexCliGuideDismissed: true,
  webAccessPasswordConfigured: false,
  locale: "zh-CN",
  localeOptions: ["zh-CN", "en"],
  serviceAddr: "localhost:48760",
  serviceListenMode: "loopback",
  serviceListenModeOptions: ["loopback", "all_interfaces"],
  routeStrategy: "ordered",
  routeStrategyOptions: ["ordered", "balanced"],
  freeAccountMaxModel: "auto",
  freeAccountMaxModelOptions: ["auto", "gpt-5"],
  modelForwardRules: "",
  accountMaxInflight: 1,
  gatewayOriginator: "codex-cli",
  gatewayOriginatorDefault: "codex-cli",
  gatewayUserAgentVersion: "1.0.0",
  gatewayUserAgentVersionDefault: "1.0.0",
  gatewayResidencyRequirement: "",
  gatewayResidencyRequirementOptions: ["", "us"],
  pluginMarketMode: "builtin",
  pluginMarketSourceUrl: "",
  upstreamProxyUrl: "",
  upstreamStreamTimeoutMs: 600000,
  upstreamTotalTimeoutMs: 0,
  sseKeepaliveIntervalMs: 15000,
  backgroundTasks: {
    usagePollingEnabled: true,
    usagePollIntervalSecs: 600,
    gatewayKeepaliveEnabled: true,
    gatewayKeepaliveIntervalSecs: 180,
    tokenRefreshPollingEnabled: true,
    tokenRefreshPollIntervalSecs: 60,
    usageRefreshWorkers: 4,
    httpWorkerFactor: 4,
    httpWorkerMin: 8,
    httpStreamWorkerFactor: 1,
    httpStreamWorkerMin: 2,
  },
  envOverrides: {},
  envOverrideCatalog: [],
  envOverrideReservedKeys: [],
  envOverrideUnsupportedKeys: [],
  theme: "tech",
  appearancePreset: "classic",
};

const ACCOUNT_ITEMS = [
  { id: "acct-1", label: "first@example.com", plan_type: "plus", status: "active", sort: 0 },
  { id: "acct-2", label: "second@example.com", plan_type: "free", status: "active", sort: 5 },
  { id: "acct-3", label: "third@example.com", plan_type: "pro", status: "active", sort: 10 },
];

async function installAccountRpcMock(
  page: Page,
  { failSingleSortUpdate = false } = {},
) {
  const sortUpdatePayloads: Record<string, unknown>[][] = [];
  const singleSortUpdatePayloads: Record<string, unknown>[] = [];
  let accountItems = ACCOUNT_ITEMS.map((account) => ({ ...account }));

  await page.route("**/api/runtime**", async (route) => {
    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        mode: "web-gateway",
        rpcBaseUrl: "/api/rpc",
        canManageService: false,
        canSelfUpdate: false,
        canCloseToTray: false,
        canOpenLocalDir: false,
        canUseBrowserFileImport: true,
        canUseBrowserDownloadExport: true,
      }),
    });
  });

  await page.route("**/api/rpc**", async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const params = payload?.params ?? {};
    const id = payload?.id ?? 1;

    const ok = (result: unknown) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          result,
        }),
      });
    const fail = (message: string) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          error: { code: -32000, message },
        }),
      });

    if (method === "appSettings/get") {
      await ok(SETTINGS_SNAPSHOT);
      return;
    }
    if (method === "initialize") {
      await ok({
        version: "0.3.1",
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "/tmp/.codex",
        platformFamily: "unix",
        platformOs: "macos",
      });
      return;
    }
    if (method === "accountManager/session/current") {
      await ok({
        mode: "none",
        currentUser: null,
        role: "system_admin",
        permissions: ["system:admin"],
        distributionEnabled: false,
      });
      return;
    }
    if (method === "account/list") {
      await ok({
        items: accountItems,
        total: accountItems.length,
        page: 1,
        pageSize: 20,
      });
      return;
    }
    if (method === "account/usage/list") {
      await ok([]);
      return;
    }
    if (method === "account/updateSorts") {
      const updates = params?.updates;
      sortUpdatePayloads.push(Array.isArray(updates) ? updates : []);
      const nextSortById = new Map(
        (Array.isArray(updates) ? updates : []).map((update) => [
          String(update?.accountId || ""),
          Number(update?.sort),
        ]),
      );
      accountItems = accountItems
        .map((account) => ({
          ...account,
          sort: nextSortById.has(account.id)
            ? Number(nextSortById.get(account.id))
            : account.sort,
        }))
        .sort((left, right) => left.sort - right.sort);
      await ok({});
      return;
    }
    if (method === "account/update") {
      const update = {
        accountId: String(params?.accountId || params?.account_id || ""),
        sort: Number(params?.sort),
      };
      singleSortUpdatePayloads.push(update);
      if (failSingleSortUpdate) {
        await fail("sort update failed");
        return;
      }
      accountItems = accountItems
        .map((account) =>
          account.id === update.accountId
            ? { ...account, sort: update.sort }
            : account,
        )
        .sort((left, right) => left.sort - right.sort);
      await ok({});
      return;
    }

    await route.fulfill({
      status: 500,
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        error: {
          code: -32000,
          message: `Unhandled RPC method in test: ${method}`,
        },
      }),
    });
  });

  return { sortUpdatePayloads, singleSortUpdatePayloads };
}

test("account row menu moves an account to the top of the pool", async ({
  page,
}) => {
  const { sortUpdatePayloads } = await installAccountRpcMock(page);

  await page.goto("/accounts/");

  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(ACCOUNT_ITEMS.length);

  // 关闭后的菜单仍留在 DOM 中，只断言当前可见的那一份。
  const openMenuItem = (name: string) =>
    page.getByRole("menuitem", { name }).filter({ visible: true });

  const actionCells = page
    .getByRole("table", { name: "账号操作" })
    .getByRole("cell");
  await expect(actionCells).toHaveCount(ACCOUNT_ITEMS.length);
  await actionCells.last().getByLabel("更多账号操作").click();

  const moveToTopItem = openMenuItem("移到顶部");
  await expect(moveToTopItem).toBeVisible();
  await expect(openMenuItem("移到底部")).toHaveAttribute(
    "aria-disabled",
    "true",
  );

  await moveToTopItem.click();

  await expect.poll(() => sortUpdatePayloads.length).toBe(1);
  expect(sortUpdatePayloads[0]).toEqual([
    { accountId: "acct-3", sort: 0 },
    { accountId: "acct-1", sort: 5 },
    { accountId: "acct-2", sort: 10 },
  ]);

  // 等待排序落库并关闭上一个菜单，避免重排进行中把菜单项判成禁用。
  await expect(page.getByText("账号顺序已调整（3 项）")).toBeVisible();
  await expect(openMenuItem("移到顶部")).toHaveCount(0);

  await actionCells.first().getByLabel("更多账号操作").click();
  await expect(openMenuItem("移到顶部")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(openMenuItem("移到底部")).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("account sort can be edited directly in the list", async ({ page }) => {
  const { singleSortUpdatePayloads } = await installAccountRpcMock(page);

  await page.goto("/accounts/");
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();

  const secondAccountRow = page
    .getByRole("row")
    .filter({ hasText: "second@example.com" });
  const sortButton = secondAccountRow.getByRole("button", {
    name: "更新账号顺序",
  });

  await sortButton.click();
  const sortInput = secondAccountRow.getByRole("spinbutton", {
    name: "顺序",
  });
  await expect(sortInput).toBeFocused();
  await expect(sortInput).toHaveValue("5");

  await sortInput.fill("8");
  await page.keyboard.press("Escape");
  await expect(sortInput).toHaveCount(0);
  expect(singleSortUpdatePayloads).toEqual([]);

  await sortButton.click();
  await secondAccountRow
    .getByRole("spinbutton", { name: "顺序" })
    .fill("9");
  await page.keyboard.press("Enter");

  await expect.poll(() => singleSortUpdatePayloads.length).toBe(1);
  expect(singleSortUpdatePayloads[0]).toEqual({
    accountId: "acct-2",
    sort: 9,
  });
  await expect(page.getByText("账号顺序已更新")).toBeVisible();
  await expect(
    secondAccountRow.getByRole("button", { name: "更新账号顺序" }),
  ).toHaveText("9");
});

test("failed inline sort update keeps the draft available", async ({ page }) => {
  const { singleSortUpdatePayloads } = await installAccountRpcMock(page, {
    failSingleSortUpdate: true,
  });

  await page.goto("/accounts/");
  const firstAccountRow = page
    .getByRole("row")
    .filter({ hasText: "first@example.com" });
  await firstAccountRow
    .getByRole("button", { name: "更新账号顺序" })
    .click();

  const sortInput = firstAccountRow.getByRole("spinbutton", {
    name: "顺序",
  });
  await sortInput.fill("7");
  await page.keyboard.press("Enter");

  await expect.poll(() => singleSortUpdatePayloads.length).toBe(1);
  await expect(page.getByText(/更新顺序失败.*sort update failed/)).toBeVisible();
  await expect(sortInput).toBeVisible();
  await expect(sortInput).toHaveValue("7");
  await expect(sortInput).toBeFocused();
});
