import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

test("account client exposes one RPC call for batch sort updates", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "lib", "api", "account-client.ts"),
    "utf8",
  );

  assert.match(
    source,
    /updateSorts:\s*\(updates:[\s\S]*?invoke\(\s*["']service_account_update_sorts["']/,
  );
  assert.match(source, /updates:\s*updates\.map\(\(update\) => \(\{/);
});

test("accounts hook reorders accounts through the batch sort API", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "hooks", "useAccounts.ts"),
    "utf8",
  );

  const mutation = source.match(
    /const reorderAccountsMutation = useMutation\(\{[\s\S]*?const updateAccountProfileMutation/,
  )?.[0] || "";
  assert.match(mutation, /await accountClient\.updateSorts\(updates\)/);
  assert.doesNotMatch(mutation, /for \(const update of updates\)/);
});

test("account access toggle validates before enabling and keeps a force-enable escape hatch", async () => {
  const hookSource = await fs.readFile(
    path.join(appsRoot, "src", "hooks", "useAccounts.ts"),
    "utf8",
  );
  const viewSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "accounts", "accounts-page-view.tsx"),
    "utf8",
  );

  const toggleMutation = hookSource.match(
    /const toggleAccountStatusMutation = useMutation\(\{[\s\S]*?const importByDirectoryMutation/,
  )?.[0] || "";
  assert.match(
    toggleMutation,
    /const refreshResult = await accountClient\.refreshUsage\(accountId\);[\s\S]*?await accountClient\.enableAccount\(accountId\);/,
  );
  assert.match(toggleMutation, /action: "validation_failed" as const/);
  assert.doesNotMatch(toggleMutation, /markUnavailableOnFailure:\s*true/);
  assert.match(
    toggleMutation,
    /if \(force\) \{[\s\S]*?await accountClient\.enableAccount\(accountId\);[\s\S]*?action: "force_enabled"/,
  );
  assert.match(
    viewSource,
    /String\(account\.status \|\| ""\)[\s\S]*?\.toLowerCase\(\) !==[\s\S]*?"disabled"/,
  );
  assert.doesNotMatch(viewSource, /accessEnabled[\s\S]{0,120}"inactive"/);
  assert.match(
    viewSource,
    /checked=\{accessEnabled\}[\s\S]{0,240}isUpdatingManyStatuses/,
  );
  assert.match(
    viewSource,
    /!accessEnabled[\s\S]{0,420}isUpdatingManyStatuses[\s\S]{0,240}forceEnableAccount\(account\.id\)/,
  );
  assert.match(
    viewSource,
    /canRecoverInactive[\s\S]*?toggleAccountStatus\(account\.id, true\)/,
  );
});

test("bulk account access validates each account before enabling", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "hooks", "useAccounts.ts"),
    "utf8",
  );

  const mutation = source.match(
    /const toggleManyAccountStatusMutation = useMutation\(\{[\s\S]*?const importByDirectoryMutation/,
  )?.[0] || "";
  assert.match(
    mutation,
    /if \(!enabled\) \{[\s\S]*?accountClient\.disableAccount\(accountId\)[\s\S]*?accountClient\.refreshUsage\(accountId\)[\s\S]*?accountClient\.enableAccount\(accountId\)/,
  );
  assert.match(
    mutation,
    /!refreshResult\.ok[\s\S]*?refreshResult\.total === 0[\s\S]*?refreshResult\.processed === 0/,
  );
  assert.match(
    mutation,
    /start \+= BULK_ACCOUNT_STATUS_CONCURRENCY[\s\S]*?normalizedIds\.slice\([\s\S]*?Promise\.allSettled\(batch\.map\(updateStatus\)\)/,
  );
  assert.doesNotMatch(mutation, /normalizedIds\.map\(updateStatus\)/);
});

test("account UI groups by the selected display mode and moves only inside the same group", async () => {
  const pageSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "accounts", "page.tsx"),
    "utf8",
  );

  assert.match(
    pageSource,
    /return groupAccountsByDisplayOrder\(matchedAccounts, displayOrderMode\);/,
  );
  assert.match(
    pageSource,
    /const originalGroup = accounts\.filter\([\s\S]*?getAccountDisplayGroup\(item, displayOrderMode\)[\s\S]*?getAccountDisplayGroup\(account, displayOrderMode\)/,
  );
  assert.match(
    pageSource,
    /buildAccountGroupOrderUpdates\(originalGroup, reorderedAccounts\)/,
  );
  assert.match(
    pageSource,
    /getAccountDisplayGroup\(targetAccount, displayOrderMode\)[\s\S]*?!==[\s\S]*?getAccountDisplayGroup\(account, displayOrderMode\)/,
  );
});

test("account status filter includes manually disabled accounts", async () => {
  const pageSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "accounts", "page.tsx"),
    "utf8",
  );
  const helperSource = await fs.readFile(
    path.join(
      appsRoot,
      "src",
      "app",
      "accounts",
      "accounts-page-helpers.tsx",
    ),
    "utf8",
  );

  assert.match(helperSource, /StatusFilter[\s\S]*?\| "disabled"/);
  assert.match(helperSource, /case "disabled":[\s\S]*?return t\("已禁用"\)/);
  assert.match(
    pageSource,
    /statusFilter === "disabled"[\s\S]*?\.toLowerCase\(\) === "disabled"/,
  );
  assert.match(pageSource, /id: "disabled" as const/);
});

test("aggregate API list uses one RPC call for atomic batch reordering", async () => {
  const clientSource = await fs.readFile(
    path.join(appsRoot, "src", "lib", "api", "account-client.ts"),
    "utf8",
  );
  const pageSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "aggregate-api", "page.tsx"),
    "utf8",
  );

  assert.match(
    clientSource,
    /updateAggregateApiSorts:[\s\S]*?service_aggregate_api_update_sorts/,
  );
  assert.match(pageSource, /accountClient\.updateAggregateApiSorts\(updates\)/);
  assert.doesNotMatch(pageSource, /for \(const update of updates\)/);
});

test("aggregate API automatic control stays separate from manual status", async () => {
  const clientSource = await fs.readFile(
    path.join(appsRoot, "src", "lib", "api", "account-client.ts"),
    "utf8",
  );
  const pageSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "aggregate-api", "page.tsx"),
    "utf8",
  );
  const modalSource = await fs.readFile(
    path.join(appsRoot, "src", "components", "modals", "aggregate-api-modal.tsx"),
    "utf8",
  );

  assert.match(clientSource, /autoToggleEnabled:\s*[\s\S]*?params\.autoToggleEnabled/);
  assert.match(
    clientSource,
    /recoverAggregateApi:[\s\S]*?service_aggregate_api_recover/,
  );
  assert.match(pageSource, /status:\s*enabled \? "active" : "disabled"/);
  assert.match(pageSource, /api\.autoToggleEnabled && api\.autoDisabled/);
  assert.match(modalSource, /useState\(false\)[\s\S]*?setAutoToggleEnabled/);
});

test("desktop import picker results are not imported a second time by the web client", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "lib", "api", "account-client.ts"),
    "utf8",
  );

  assert.match(
    source,
    /if \(picked\?\.canceled \|\| !Array\.isArray\(picked\?\.contents\) \|\| picked\.contents\.length === 0\) \{\s*return picked;\s*\}/,
  );
  assert.match(source, /const imported = await importAccountContents\(picked\.contents\)/);
});
