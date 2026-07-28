export type MarketplaceSource = {
  id: string;
  productId: string;
  tags: string[];
  merchant: string | null;
  enabled: boolean;
  verifyEnabled: boolean;
  lastSuccessfulSyncId: number;
  lastSyncAt: number | null;
  lastSyncError: string | null;
  createdAt: number;
  updatedAt: number;
};

export type MarketplaceOffer = {
  offerKey: string;
  sourceConfigId: string;
  offerId: string;
  productId: string;
  sourceId: string | null;
  sourceName: string | null;
  sourceIncludedAt: string | null;
  sourceShopCreatedAt: string | null;
  collectorKind: string | null;
  title: string | null;
  price: number | null;
  listedPrice: number | null;
  currency: string;
  rawStatus: string | null;
  effectiveStatus: string | null;
  freshnessStatus: string | null;
  priceAiUpdatedAt: string | null;
  expiresAt: string | null;
  url: string | null;
  tags: string[];
  filterTags: string[];
  stockCount: number | null;
  localStatus: string;
  localCheckedAt: number | null;
  localError: string | null;
  firstSeenAt: number;
  lastSeenAt: number;
  isCurrent: boolean;
  merchantKey: string | null;
};

export type MarketplaceFavoriteMerchant = {
  merchantKey: string;
  sourceId: string | null;
  sourceName: string | null;
  collectorKind: string | null;
  createdAt: number;
  updatedAt: number;
};

export type MarketplaceChange = {
  id: number;
  offerKey: string;
  changeType: string;
  summary: Record<string, unknown>;
  createdAt: number;
};

export type MarketplaceAlertRule = {
  id: string;
  name: string;
  sourceConfigId: string | null;
  productId: string | null;
  tags: string[];
  merchant: string | null;
  currency: string;
  maxPrice: number | null;
  dropAmount: number | null;
  dropPercent: number | null;
  notifyRestock: boolean;
  notifyVerified: boolean;
  notifyInvalidLink: boolean;
  enabled: boolean;
};
