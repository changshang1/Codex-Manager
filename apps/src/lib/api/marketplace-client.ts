import { invoke, withAddr } from "./transport";
import type {
  MarketplaceAlertRule,
  MarketplaceChange,
  MarketplaceFavoriteMerchant,
  MarketplaceOffer,
  MarketplaceSource,
} from "@/types";

const record = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
const string = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;
const number = (value: unknown, fallback = 0) =>
  typeof value === "number" && Number.isFinite(value) ? value : fallback;
const nullableNumber = (value: unknown) =>
  typeof value === "number" && Number.isFinite(value) ? value : null;
const bool = (value: unknown, fallback = false) =>
  typeof value === "boolean" ? value : fallback;
const strings = (value: unknown) =>
  Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];

function parseStrings(value: unknown): string[] {
  try {
    return strings(typeof value === "string" ? JSON.parse(value) : value);
  } catch {
    return [];
  }
}

function source(value: unknown): MarketplaceSource {
  const v = record(value);
  return {
    id: string(v.id),
    productId: string(v.productId),
    tags: parseStrings(v.tagsJson ?? v.tags),
    merchant: string(v.merchant) || null,
    enabled: bool(v.enabled, true),
    verifyEnabled: bool(v.verifyEnabled, true),
    lastSuccessfulSyncId: number(v.lastSuccessfulSyncId),
    lastSyncAt: nullableNumber(v.lastSyncAt),
    lastSyncError: string(v.lastSyncError) || null,
    createdAt: number(v.createdAt),
    updatedAt: number(v.updatedAt),
  };
}

function offer(value: unknown): MarketplaceOffer {
  const v = record(value);
  return {
    offerKey: string(v.offerKey),
    sourceConfigId: string(v.sourceConfigId),
    offerId: string(v.offerId),
    productId: string(v.productId),
    sourceId: string(v.sourceId) || null,
    sourceName: string(v.sourceName) || null,
    sourceIncludedAt: string(v.sourceIncludedAt) || null,
    sourceShopCreatedAt: string(v.sourceShopCreatedAt) || null,
    collectorKind: string(v.collectorKind) || null,
    title: string(v.title) || null,
    price: nullableNumber(v.price),
    listedPrice: nullableNumber(v.listedPrice),
    currency: string(v.currency, "CNY"),
    rawStatus: string(v.rawStatus) || null,
    effectiveStatus: string(v.effectiveStatus) || null,
    freshnessStatus: string(v.freshnessStatus) || null,
    priceAiUpdatedAt: string(v.priceAiUpdatedAt) || null,
    expiresAt: string(v.expiresAt) || null,
    url: string(v.url) || null,
    tags: parseStrings(v.tagsJson ?? v.tags),
    filterTags: parseStrings(v.filterTagsJson ?? v.filterTags),
    stockCount: nullableNumber(v.stockCount),
    localStatus: string(v.localStatus, "unknown"),
    localCheckedAt: nullableNumber(v.localCheckedAt),
    localError: string(v.localError) || null,
    firstSeenAt: number(v.firstSeenAt),
    lastSeenAt: number(v.lastSeenAt),
    isCurrent: bool(v.isCurrent, true),
    merchantKey: string(v.merchantKey) || null,
  };
}

function favoriteMerchant(value: unknown): MarketplaceFavoriteMerchant {
  const v = record(value);
  return {
    merchantKey: string(v.merchantKey),
    sourceId: string(v.sourceId) || null,
    sourceName: string(v.sourceName) || null,
    collectorKind: string(v.collectorKind) || null,
    createdAt: number(v.createdAt),
    updatedAt: number(v.updatedAt),
  };
}

function rule(value: unknown): MarketplaceAlertRule {
  const v = record(value);
  return {
    id: string(v.id),
    name: string(v.name),
    sourceConfigId: string(v.sourceConfigId) || null,
    productId: string(v.productId) || null,
    tags: parseStrings(v.tagsJson ?? v.tags),
    merchant: string(v.merchant) || null,
    currency: string(v.currency, "CNY"),
    maxPrice: nullableNumber(v.maxPrice),
    dropAmount: nullableNumber(v.dropAmount),
    dropPercent: nullableNumber(v.dropPercent),
    notifyRestock: bool(v.notifyRestock, true),
    notifyVerified: bool(v.notifyVerified, true),
    notifyInvalidLink: bool(v.notifyInvalidLink),
    enabled: bool(v.enabled, true),
  };
}

function change(value: unknown): MarketplaceChange {
  const v = record(value);
  let summary: Record<string, unknown> = {};
  try {
    summary = record(JSON.parse(string(v.summaryJson, "{}")));
  } catch {
    // Keep malformed historical summaries readable as empty metadata.
  }
  return {
    id: number(v.id),
    offerKey: string(v.offerKey),
    changeType: string(v.changeType),
    summary,
    createdAt: number(v.createdAt),
  };
}

export const marketplaceClient = {
  async sources(): Promise<MarketplaceSource[]> {
    const value = await invoke<unknown>("service_marketplace_source_list", withAddr());
    return Array.isArray(value) ? value.map(source) : [];
  },
  async saveSource(payload: Record<string, unknown>): Promise<MarketplaceSource> {
    return source(await invoke("service_marketplace_source_upsert", withAddr({ payload })));
  },
  async deleteSource(id: string) {
    await invoke("service_marketplace_source_delete", withAddr({ id }));
  },
  async offers(payload: Record<string, unknown> = {}): Promise<MarketplaceOffer[]> {
    const value = await invoke<unknown>(
      "service_marketplace_offer_list",
      withAddr({ payload }),
    );
    return Array.isArray(value) ? value.map(offer) : [];
  },
  async verifyOffer(offerKey: string): Promise<MarketplaceOffer> {
    return offer(
      await invoke("service_marketplace_offer_verify", withAddr({ offerKey })),
    );
  },
  async favoriteMerchants(): Promise<MarketplaceFavoriteMerchant[]> {
    const value = await invoke<unknown>(
      "service_marketplace_favorite_merchant_list",
      withAddr(),
    );
    return Array.isArray(value) ? value.map(favoriteMerchant) : [];
  },
  async setFavoriteMerchant(
    offerKey: string,
    favorite: boolean,
  ): Promise<MarketplaceFavoriteMerchant[]> {
    const value = await invoke<unknown>(
      "service_marketplace_favorite_merchant_set",
      withAddr({ offerKey, favorite }),
    );
    return Array.isArray(value) ? value.map(favoriteMerchant) : [];
  },
  async refresh() {
    return invoke<{ count: number; notified: number; errors: string[]; completedAt: number }>(
      "service_marketplace_refresh",
      withAddr(),
    );
  },
  async changes(limit = 200): Promise<MarketplaceChange[]> {
    const value = await invoke<unknown>(
      "service_marketplace_change_list",
      withAddr({ payload: { limit } }),
    );
    return Array.isArray(value) ? value.map(change) : [];
  },
  async alerts(): Promise<MarketplaceAlertRule[]> {
    const value = await invoke<unknown>("service_marketplace_alert_list", withAddr());
    return Array.isArray(value) ? value.map(rule) : [];
  },
  async saveAlert(payload: Record<string, unknown>): Promise<MarketplaceAlertRule> {
    return rule(await invoke("service_marketplace_alert_upsert", withAddr({ payload })));
  },
  async deleteAlert(id: string) {
    await invoke("service_marketplace_alert_delete", withAddr({ id }));
  },
  async notificationEnabled(): Promise<boolean> {
    return Boolean(await invoke("service_marketplace_notification_get", withAddr()));
  },
  async setNotificationEnabled(enabled: boolean) {
    await invoke("service_marketplace_notification_set", withAddr({ enabled }));
  },
};
