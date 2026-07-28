import type { MarketplaceOffer } from "@/types";

export type MarketplaceFilterTagGroup =
  | "access"
  | "plusChannel"
  | "plusRecharge"
  | "team"
  | "proxy"
  | "warranty";

export type MarketplaceFilterTagDefinition = {
  id: string;
  label: string;
  group: MarketplaceFilterTagGroup;
  description: string;
};

export const MARKETPLACE_FILTER_TAGS: MarketplaceFilterTagDefinition[] = [
  { id: "shared_access", label: "拼车/团购", group: "access", description: "多人共享、拼车、团购、车位或合租类报价。" },
  { id: "web_only_account", label: "网页号", group: "access", description: "仅限网页或只能通过网页使用的 ChatGPT Plus 报价。" },
  { id: "domestic_mirror_site", label: "国内镜像站", group: "access", description: "国内镜像、网页镜像、镜像站或 mirror 方式访问的报价。" },
  { id: "delivery_recharge", label: "充值", group: "access", description: "充值、直充、代充、自助开通、卡密、CDK 或兑换码类报价。" },
  { id: "delivery_account", label: "成品号", group: "access", description: "交付账号、账密、独享号、首登或接码状态明确的报价。" },
  { id: "account_verified", label: "已接码成品号", group: "access", description: "已接码、已绑定手机或手机验证状态已完成的 ChatGPT Plus 成品号。" },
  { id: "account_unverified", label: "未接码成品号", group: "access", description: "未接码、未绑定手机或需要自行接码的 ChatGPT Plus 成品号。" },
  { id: "chatgpt_plus_brazil_pix", label: "巴西 Pix", group: "plusChannel", description: "ChatGPT Plus 试用订阅中的巴西 Pix 渠道报价。" },
  { id: "chatgpt_plus_netherlands_ideal", label: "荷兰 iDEAL", group: "plusChannel", description: "ChatGPT Plus 试用订阅中的荷兰 iDEAL 渠道报价。" },
  { id: "chatgpt_plus_india_upi", label: "印度 UPI", group: "plusChannel", description: "ChatGPT Plus 试用订阅中的印度 UPI 渠道报价。" },
  { id: "chatgpt_plus_europe_channel", label: "欧洲渠道", group: "plusChannel", description: "ChatGPT Plus 试用订阅中的欧洲、欧区或 AT 渠道报价。" },
  { id: "chatgpt_plus_recharge_ph_card", label: "菲区卡充", group: "plusRecharge", description: "ChatGPT Plus 正价代充中的菲律宾或菲区卡充报价。" },
  { id: "chatgpt_plus_recharge_us_ios", label: "美区 iOS", group: "plusRecharge", description: "ChatGPT Plus 正价代充中的美区 iOS、App Store 或内购报价。" },
  { id: "chatgpt_plus_recharge_official_direct", label: "官方直充", group: "plusRecharge", description: "ChatGPT Plus 正价代充中的官方充值、正价代充或正规直充报价。" },
  { id: "team_k12", label: "K12", group: "team", description: "ChatGPT Team / Business 中的 K12、K12 子号或 K12 渠道报价。" },
  { id: "team_bug", label: "Bug Team", group: "team", description: "ChatGPT Team / Business 中的 Bug Team、Team Bug 或 Bug 号报价。" },
  { id: "team_official", label: "正价/官方 Team", group: "team", description: "正规官方 Team、Business、激活码或续费码报价。" },
  { id: "proxy_supported", label: "可反代", group: "proxy", description: "支持反代、Codex、sub2、cpa、JSON 或 API 格式的报价。" },
  { id: "warranty_long", label: "长期质保", group: "warranty", description: "15 天以上、一个月、整月、包月或全程质保。" },
];

export const MARKETPLACE_FILTER_TAG_BY_ID = new Map(
  MARKETPLACE_FILTER_TAGS.map((definition) => [definition.id, definition]),
);

export const MARKETPLACE_FILTER_GROUP_LABELS: Record<MarketplaceFilterTagGroup, string> = {
  access: "商品特征",
  plusChannel: "Plus 渠道",
  plusRecharge: "Plus 代充渠道",
  team: "Team 类型",
  proxy: "反代能力",
  warranty: "质保",
};

export const MARKETPLACE_QUICK_FILTER_TAGS = new Set([
  "shared_access",
  "web_only_account",
  "domestic_mirror_site",
  "delivery_recharge",
  "delivery_account",
  "account_verified",
  "account_unverified",
  "proxy_supported",
  "warranty_long",
]);

export type MarketplaceCollectorFilter =
  | "all"
  | "liandongShop"
  | "yunmaoConsignment"
  | "qxvx"
  | "dujiao"
  | "kami"
  | "other";

export const MARKETPLACE_COLLECTOR_FILTERS: Array<{
  id: MarketplaceCollectorFilter;
  label: string;
}> = [
  { id: "all", label: "全部来源" },
  { id: "liandongShop", label: "链动小铺" },
  { id: "yunmaoConsignment", label: "云猫寄售" },
  { id: "qxvx", label: "QXVX Pay" },
  { id: "dujiao", label: "独角数卡" },
  { id: "kami", label: "异次元" },
  { id: "other", label: "自研" },
];

export type MarketplaceAdvancedFilters = {
  tags: string[];
  collector: MarketplaceCollectorFilter;
  minStock: number | null;
  freshWithinMinutes: number | null;
  minPrice: number | null;
  maxPrice: number | null;
  query: string;
  excludeQuery: string;
};

export const EMPTY_MARKETPLACE_ADVANCED_FILTERS: MarketplaceAdvancedFilters = {
  tags: [],
  collector: "all",
  minStock: null,
  freshWithinMinutes: null,
  minPrice: null,
  maxPrice: null,
  query: "",
  excludeQuery: "",
};

function offerText(offer: MarketplaceOffer): string {
  return [
    offer.title,
    offer.sourceName,
    offer.sourceId,
    offer.collectorKind,
    offer.productId,
    offer.url,
    ...offer.tags,
    ...offer.filterTags,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function sourceText(offer: MarketplaceOffer): string {
  return [offer.sourceName, offer.sourceId, offer.url].filter(Boolean).join(" ").toLowerCase();
}

function urlHost(value: string | null): string {
  if (!value) return "";
  try {
    return new URL(value).hostname.toLowerCase().replace(/^www\./, "");
  } catch {
    return "";
  }
}

export function marketplaceOfferCollector(offer: MarketplaceOffer): Exclude<MarketplaceCollectorFilter, "all"> {
  const host = urlHost(offer.url);
  const source = sourceText(offer);
  if (["ldxp.cn", "pay.ldxp.cn"].includes(host) || /ldxp|链动|鏈動|liandong/.test(source)) {
    return "liandongShop";
  }
  if (host === "catfk.com" || /catfk|云猫|yunmao/.test(source)) {
    return "yunmaoConsignment";
  }
  if (host === "pay.qxvx.cn" || /qxvx/.test(source)) {
    return "qxvx";
  }
  if (offer.collectorKind === "dujiao") return "dujiao";
  if (offer.collectorKind === "kami") return "kami";
  return "other";
}

export function marketplaceOfferMatchesAdvancedFilters(
  offer: MarketplaceOffer,
  filters: MarketplaceAdvancedFilters,
  now = Date.now(),
): boolean {
  if (!filters.tags.every((tag) => offer.filterTags.includes(tag))) return false;
  if (filters.collector !== "all" && marketplaceOfferCollector(offer) !== filters.collector) {
    return false;
  }
  if (filters.minStock != null && (offer.stockCount == null || offer.stockCount < filters.minStock)) {
    return false;
  }
  if (filters.minPrice != null && (offer.price == null || offer.price < filters.minPrice)) return false;
  if (filters.maxPrice != null && (offer.price == null || offer.price > filters.maxPrice)) return false;
  if (filters.freshWithinMinutes != null) {
    const updatedAt = offer.priceAiUpdatedAt ? Date.parse(offer.priceAiUpdatedAt) : Number.NaN;
    if (!Number.isFinite(updatedAt) || updatedAt < now - filters.freshWithinMinutes * 60_000) {
      return false;
    }
  }
  const haystack = offerText(offer);
  const query = filters.query.trim().toLowerCase();
  if (query && !haystack.includes(query)) return false;
  const excludedTerms = filters.excludeQuery
    .toLowerCase()
    .split(/[,，\s]+/)
    .map((term) => term.trim())
    .filter(Boolean);
  return excludedTerms.every((term) => !haystack.includes(term));
}

export function marketplaceFiltersActive(filters: MarketplaceAdvancedFilters): boolean {
  return (
    filters.tags.length > 0 ||
    filters.collector !== "all" ||
    filters.minStock != null ||
    filters.freshWithinMinutes != null ||
    filters.minPrice != null ||
    filters.maxPrice != null ||
    Boolean(filters.query.trim()) ||
    Boolean(filters.excludeQuery.trim())
  );
}
