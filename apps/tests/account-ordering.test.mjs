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
