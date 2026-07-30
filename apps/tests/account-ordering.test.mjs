import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "account-ordering.ts");

async function loadOrderingModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-account-ordering-"),
  );
  const tempFile = path.join(tempDir, "account-ordering.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const ordering = await loadOrderingModule();

test("availability grouping is stable and does not rewrite persisted sort", () => {
  const accounts = [
    { id: "available-1", sort: 1, priority: 99, isAvailable: true },
    { id: "unavailable-2", sort: 2, priority: 0, isAvailable: false },
    { id: "available-3", sort: 3, priority: 0, isAvailable: true },
  ];

  assert.deepEqual(
    ordering.groupAccountsByAvailability(accounts).map((account) => account.id),
    ["available-1", "available-3", "unavailable-2"],
  );
  assert.deepEqual(accounts.map((account) => account.sort), [1, 2, 3]);
});

test("access ordering puts non-disabled accounts first without changing sort", () => {
  const accounts = [
    { id: "disabled", sort: 0, status: "disabled", isAvailable: false },
    { id: "limited", sort: 5, status: "active", isAvailable: false },
    { id: "available", sort: 10, status: "active", isAvailable: true },
  ];

  assert.deepEqual(
    ordering
      .groupAccountsByDisplayOrder(accounts, "access")
      .map((account) => account.id),
    ["limited", "available", "disabled"],
  );
  assert.deepEqual(accounts.map((account) => account.sort), [0, 5, 10]);
});

test("in-warranty refresh-token incidents are placed before every display group", () => {
  const accounts = [
    { id: "available", sort: 0, status: "active", isAvailable: true },
    {
      id: "warranty-incident",
      sort: 5,
      status: "unavailable",
      statusReason: "refresh_token_invalid:expired",
      warrantyExpiresOn: "2999-08-06",
      isAvailable: false,
    },
    { id: "disabled", sort: 10, status: "disabled", isAvailable: false },
  ];

  assert.deepEqual(
    ordering
      .groupAccountsByDisplayOrder(accounts, "availability")
      .map((account) => account.id),
    ["warranty-incident", "available", "disabled"],
  );
  assert.deepEqual(
    ordering
      .groupAccountsByDisplayOrder(accounts, "access")
      .map((account) => account.id),
    ["warranty-incident", "available", "disabled"],
  );
});

test("warranty incidents require an unexpired date and explicit refresh-token invalidation", () => {
  const base = {
    id: "account",
    sort: 0,
    status: "unavailable",
    statusReason: "refresh_token_invalid:expired",
    warrantyExpiresOn: "2026-08-06",
    isAvailable: false,
  };

  assert.equal(ordering.isAccountWarrantyIncident(base, "2026-08-06"), true);
  assert.equal(ordering.isAccountWarrantyIncident(base, "2026-08-07"), false);
  assert.equal(
    ordering.isAccountWarrantyIncident(
      { ...base, statusReason: "usage_http_401" },
      "2026-08-01",
    ),
    false,
  );
  assert.equal(
    ordering.isAccountWarrantyIncident(
      { ...base, status: "disabled" },
      "2026-08-01",
    ),
    true,
  );
});

test("invalid persisted display order falls back to availability", () => {
  assert.equal(ordering.normalizeAccountDisplayOrderMode("access"), "access");
  assert.equal(ordering.normalizeAccountDisplayOrderMode("unknown"), "availability");
  assert.equal(ordering.normalizeAccountDisplayOrderMode(null), "availability");
});

test("same-availability moves reuse that group's original sort slots", () => {
  const availableFirst = {
    id: "available-1",
    sort: 1,
    priority: 99,
    isAvailable: true,
  };
  const unavailable = {
    id: "unavailable-2",
    sort: 2,
    priority: 0,
    isAvailable: false,
  };
  const availableLast = {
    id: "available-3",
    sort: 3,
    priority: 0,
    isAvailable: true,
  };

  const updates = ordering.buildAccountGroupOrderUpdates(
    [availableFirst, availableLast],
    [availableLast, availableFirst],
  );
  assert.deepEqual(updates, [
    { accountId: "available-3", sort: 1 },
    { accountId: "available-1", sort: 3 },
  ]);
  assert.equal(updates.some((update) => update.accountId === unavailable.id), false);
  assert.equal(unavailable.sort, 2);
});

test("full reorder compares the real sort field before the legacy priority alias", () => {
  const updates = ordering.buildAccountOrderUpdates([
    { id: "first", sort: 0, priority: 500, isAvailable: true },
    { id: "second", sort: 5, priority: 0, isAvailable: true },
  ]);

  assert.deepEqual(updates, []);
});
