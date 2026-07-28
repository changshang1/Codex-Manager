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

test("account access toggle enables before refreshing and never treats inactive as manual off", async () => {
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
    /await accountClient\.enableAccount\(accountId\);[\s\S]*?await accountClient\.refreshUsage\(accountId,\s*\{[\s\S]*?markUnavailableOnFailure:\s*true/,
  );
  assert.match(toggleMutation, /return \{ refreshResult: null, refreshError \};/);
  assert.match(
    viewSource,
    /String\(account\.status \|\| ""\)[\s\S]*?\.toLowerCase\(\) !==[\s\S]*?"disabled"/,
  );
  assert.doesNotMatch(viewSource, /accessEnabled[\s\S]{0,120}"inactive"/);
});

test("account UI groups availability for display and moves only inside the same group", async () => {
  const pageSource = await fs.readFile(
    path.join(appsRoot, "src", "app", "accounts", "page.tsx"),
    "utf8",
  );

  assert.match(pageSource, /return groupAccountsByAvailability\(matchedAccounts\);/);
  assert.match(
    pageSource,
    /const originalGroup = accounts\.filter\([\s\S]*?item\.isAvailable === account\.isAvailable/,
  );
  assert.match(
    pageSource,
    /buildAccountGroupOrderUpdates\(originalGroup, reorderedAccounts\)/,
  );
  assert.match(
    pageSource,
    /targetAccount\.isAvailable !== account\.isAvailable/,
  );
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
