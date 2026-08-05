import type { ManagedModelV2 } from "@/types/model-v2";

export interface ModelProfileStatus {
  autoUpdateEnabled: boolean;
  sourceUrl: string;
  schemaVersion: number;
  revision: number;
  catalogUpdatedAt: string;
  lastCheckedAt: number | null;
  lastSuccessAt: number | null;
  lastError: string | null;
  source: "cache" | "builtin" | string;
  importableCount: number;
  updateCount: number;
}

export interface ModelProfileChange {
  field: string;
  before: unknown;
  after: unknown;
}

export interface ModelProfileCandidate {
  kind: "import" | "update";
  slug: string;
  displayName: string;
  sourceId: string;
  sourceName: string;
  upstreamModel: string;
  profileRevision: number;
  profileHash: string;
  changes: ModelProfileChange[];
}

export interface ModelProfileCandidateList {
  items: ModelProfileCandidate[];
}

export interface ApplyModelProfileParams {
  sourceId: string;
  upstreamModel: string;
}

export interface ApplyModelProfileResult {
  model: ManagedModelV2;
  appliedRevision: number;
  appliedHash: string;
}
