import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "api", "normalize.ts");

function readFunctionSource(source, sourceFile, functionName) {
  const declaration = sourceFile.statements.find(
    (statement) =>
      ts.isFunctionDeclaration(statement) &&
      statement.name?.text === functionName,
  );
  assert.ok(declaration, `${functionName} not found`);
  return source.slice(declaration.getStart(sourceFile), declaration.end);
}

async function loadNormalizeAccount() {
  const source = await fs.readFile(sourcePath, "utf8");
  const sourceFile = ts.createSourceFile(
    sourcePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const functionNames = [
    "asObject",
    "asString",
    "asInteger",
    "asStringArray",
    "normalizeAccount",
  ];
  const testableSource = `
const toNullableNumber = (value) => {
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};
const calcAvailability = () => ({ text: "未知", level: "unknown" });
const getUsageDisplayBuckets = () => ({
  primaryRemainPercent: null,
  secondaryRemainPercent: null,
});
const isLowQuotaUsage = () => false;
const normalizeAccountProxySummaryFields = () => ({});
${functionNames
  .map((functionName) => readFunctionSource(source, sourceFile, functionName))
  .join("\n")}
`;
  const compiled = ts.transpileModule(testableSource, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-account-normalize-"),
  );
  const tempFile = path.join(tempDir, "account-normalize.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const { normalizeAccount } = await loadNormalizeAccount();

test("normalizeAccount keeps refresh-token invalidation independent from status reason", () => {
  const camelCaseAccount = normalizeAccount({
    id: "camel-account",
    status: "disabled",
    statusReason: "manual_disable",
    refreshTokenInvalidReason:
      " refresh_token_invalid:refresh_token_expired ",
  });
  const snakeCaseAccount = normalizeAccount({
    id: "snake-account",
    status: "disabled",
    status_reason: "manual_disable",
    refresh_token_invalid_reason:
      " refresh_token_invalid:refresh_token_reused ",
  });

  assert.equal(camelCaseAccount.statusReason, "manual_disable");
  assert.equal(
    camelCaseAccount.refreshTokenInvalidReason,
    "refresh_token_invalid:refresh_token_expired",
  );
  assert.equal(snakeCaseAccount.statusReason, "manual_disable");
  assert.equal(
    snakeCaseAccount.refreshTokenInvalidReason,
    "refresh_token_invalid:refresh_token_reused",
  );
});

test("normalizeAccount maps a missing refresh-token invalid reason to null", () => {
  const account = normalizeAccount({
    id: "healthy-account",
    statusReason: "refresh_token_invalid:stale_legacy_reason",
  });

  assert.equal(account.statusReason, "refresh_token_invalid:stale_legacy_reason");
  assert.equal(account.refreshTokenInvalidReason, null);
});
