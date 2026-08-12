"use client";

import { useEffect, useMemo, useState } from "react";
import {
  BadgeCheck,
  Bell,
  BellOff,
  Boxes,
  Check,
  ChevronLeft,
  ChevronRight,
  Clock3,
  ExternalLink,
  Info,
  ListFilter,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Settings,
  Star,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { appClient } from "@/lib/api/app-client";
import { marketplaceClient } from "@/lib/api/marketplace-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import {
  EMPTY_MARKETPLACE_ADVANCED_FILTERS,
  MARKETPLACE_COLLECTOR_FILTERS,
  MARKETPLACE_FILTER_GROUP_LABELS,
  MARKETPLACE_FILTER_TAGS,
  MARKETPLACE_QUICK_FILTER_TAGS,
  marketplaceFiltersActive,
  marketplaceOfferMatchesAdvancedFilters,
  type MarketplaceAdvancedFilters,
  type MarketplaceCollectorFilter,
  type MarketplaceFilterTagGroup,
} from "@/lib/marketplace-filters";
import type {
  MarketplaceAlertRule,
  MarketplaceChange,
  MarketplaceFavoriteMerchant,
  MarketplaceOffer,
  MarketplaceSource,
} from "@/types";

/**
 * 商品池当前是单一用途页面，只展示 PriceAI 的 ChatGPT Plus 试用订阅。
 * 产品与唯一商品源由代码固定，避免其它产品混入同一列表；定时同步的标签、
 * 启停和二次验证仍是用户配置，并由后端持久化到 SQLite。
 */
const MARKETPLACE_PRODUCT_ID = "chatgpt-plus" as const;
const MARKETPLACE_PRODUCT_LABEL = "ChatGPT Plus 试用订阅" as const;
const MARKETPLACE_SOURCE_ID = "default-chatgpt-plus" as const;

const DEFAULT_SOURCE_DRAFT = {
  enabled: true,
  verifyEnabled: true,
  tags: ["account_verified"] as string[],
};

type SourceDraft = typeof DEFAULT_SOURCE_DRAFT;

// Only tags that PriceAI can apply to chatgpt-plus are exposed to the scheduler.
// Multiple selected tags are sent in one request and therefore use PriceAI's AND semantics.
const SCHEDULE_TAG_IDS = new Set([
  "shared_access",
  "web_only_account",
  "domestic_mirror_site",
  "delivery_recharge",
  "account_verified",
  "account_unverified",
  "chatgpt_plus_brazil_pix",
  "chatgpt_plus_netherlands_ideal",
  "chatgpt_plus_india_upi",
  "chatgpt_plus_europe_channel",
  "warranty_long",
]);

const SCHEDULE_TAGS = MARKETPLACE_FILTER_TAGS.filter((tag) => SCHEDULE_TAG_IDS.has(tag.id));

const EMPTY_ALERT_DRAFT = {
  id: "",
  name: "低价提醒",
  merchant: "",
  currency: "CNY",
  tags: "",
  maxPrice: "",
  dropAmount: "",
  dropPercent: "",
  notifyRestock: true,
  notifyVerified: true,
  notifyInvalidLink: false,
  enabled: true,
};

type AlertDraft = typeof EMPTY_ALERT_DRAFT;

function alertToDraft(rule: MarketplaceAlertRule): AlertDraft {
  return {
    id: rule.id,
    name: rule.name,
    merchant: rule.merchant ?? "",
    currency: rule.currency,
    tags: rule.tags.join(","),
    maxPrice: rule.maxPrice?.toString() ?? "",
    dropAmount: rule.dropAmount?.toString() ?? "",
    dropPercent: rule.dropPercent?.toString() ?? "",
    notifyRestock: rule.notifyRestock,
    notifyVerified: rule.notifyVerified,
    notifyInvalidLink: rule.notifyInvalidLink,
    enabled: rule.enabled,
  };
}

function tagsFromInput(value: string): string[] {
  return value
    .split(/[,，]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function optionalNumber(value: string): number | null {
  return value.trim() ? Number(value) : null;
}

function alertPayload(draft: AlertDraft): Record<string, unknown> {
  return {
    id: draft.id || undefined,
    name: draft.name,
    // Alert rules use the same fixed source as the list and scheduler so an old
    // rule cannot silently target a product that is no longer visible here.
    productId: MARKETPLACE_PRODUCT_ID,
    sourceConfigId: MARKETPLACE_SOURCE_ID,
    merchant: draft.merchant || null,
    currency: draft.currency.trim().toUpperCase() || "CNY",
    tags: tagsFromInput(draft.tags),
    maxPrice: optionalNumber(draft.maxPrice),
    dropAmount: optionalNumber(draft.dropAmount),
    dropPercent: optionalNumber(draft.dropPercent),
    notifyRestock: draft.notifyRestock,
    notifyVerified: draft.notifyVerified,
    notifyInvalidLink: draft.notifyInvalidLink,
    enabled: draft.enabled,
  };
}

/**
 * 首次打开页面默认筛选 PriceAI 的 account_verified 标签。
 * 这是 PriceAI 标签，不是商品源名称；清除筛选后仍只保留固定的 Plus 产品。
 */
const DEFAULT_MARKETPLACE_FILTERS: MarketplaceAdvancedFilters = {
  ...EMPTY_MARKETPLACE_ADVANCED_FILTERS,
  tags: ["account_verified"],
};

type PriceAiStatus = "available" | "out_of_stock" | "unavailable" | "invalid";

function getPriceAiStatus(offer: MarketplaceOffer): PriceAiStatus {
  if (offer.rawStatus === "out_of_stock") {
    return "out_of_stock";
  }
  if (offer.price == null || !Number.isFinite(offer.price) || !offer.url?.trim()) {
    return "invalid";
  }
  if (offer.effectiveStatus === "failed" || offer.freshnessStatus === "failed") {
    return "invalid";
  }
  const expiresAt = offer.expiresAt ? Date.parse(offer.expiresAt) : Number.NaN;
  if (
    offer.effectiveStatus === "unavailable" ||
    offer.effectiveStatus === "stale" ||
    offer.freshnessStatus === "expired" ||
    (Number.isFinite(expiresAt) && expiresAt <= Date.now())
  ) {
    return "unavailable";
  }
  return "available";
}

function priceAiStatusLabel(status: PriceAiStatus, t: Translate): string {
  if (status === "available") return t("有货");
  if (status === "out_of_stock") return t("缺货");
  if (status === "unavailable") return t("暂不可用");
  return t("状态异常");
}

function localStatusLabel(status: string, t: Translate): string {
  if (status === "available") return t("可用");
  if (status === "unavailable") return t("缺货");
  if (status === "invalid") return t("链接失效");
  return t("无法确认");
}

function isHttpUrl(value: string | null): value is string {
  return Boolean(value && /^https?:\/\//i.test(value));
}

type Translate = (message: string, values?: Record<string, string | number>) => string;

const OFFERS_PER_PAGE = 100;

function inputNumber(value: string): number | null {
  if (!value.trim()) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function formatTimestamp(timestamp: number | null, t: Translate): string {
  return timestamp ? new Date(timestamp * 1000).toLocaleString() : t("未验证");
}

function daysSince(value: string | null): number | null {
  if (!value) return null;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return null;
  return Math.max(0, Math.floor((Date.now() - timestamp) / 86_400_000));
}

function sourceIncludedLabel(value: string | null, t: Translate): string | null {
  const days = daysSince(value);
  if (days == null) return null;
  return days < 1 ? t("收录 今天") : t("收录 {days}天前", { days });
}

function sourceShopAgeLabel(value: string | null, t: Translate): string | null {
  const days = daysSince(value);
  if (days == null) return null;
  if (days < 1) return t("公开运营 今天");
  if (days < 30) return t("公开运营 {days}天", { days });
  const months = Math.floor(days / 30);
  if (months < 12) return t("公开运营 {months}个月", { months });
  const years = Math.floor(months / 12);
  const remainingMonths = months % 12;
  return remainingMonths
    ? t("公开运营 {years}年{months}个月", { years, months: remainingMonths })
    : t("公开运营 {years}年", { years });
}

function sourceToDraft(source: MarketplaceSource): SourceDraft {
  return {
    enabled: source.enabled,
    verifyEnabled: source.verifyEnabled,
    tags: source.tags,
  };
}

function scheduledSyncCoversRule(source: MarketplaceSource | null, rule: MarketplaceAlertRule) {
  if (!source?.enabled) return false;
  return source.tags.length === 0 || source.tags.every((tag) => rule.tags.includes(tag));
}

function changeLabel(type: string, t: Translate): string {
  if (type === "restock") return t("缺货变有货");
  if (type === "verification") return t("验证状态变化");
  if (type === "price") return t("价格变化");
  if (type === "priceai_status") return t("PriceAI 状态变化");
  if (type === "invalid_link") return t("商品链接失效");
  return type;
}

export default function MarketplacePage() {
  const { t } = useI18n();
  const [offers, setOffers] = useState<MarketplaceOffer[]>([]);
  const [favoriteMerchants, setFavoriteMerchants] = useState<MarketplaceFavoriteMerchant[]>([]);
  const [changes, setChanges] = useState<MarketplaceChange[]>([]);
  const [alerts, setAlerts] = useState<MarketplaceAlertRule[]>([]);
  const [source, setSource] = useState<MarketplaceSource | null>(null);
  const [sourceDraft, setSourceDraft] = useState<SourceDraft>(DEFAULT_SOURCE_DRAFT);
  const [priceAiStatus, setPriceAiStatus] = useState("all");
  const [localStatus, setLocalStatus] = useState("all");
  const [favoriteOnly, setFavoriteOnly] = useState(false);
  const [filters, setFilters] = useState<MarketplaceAdvancedFilters>(
    DEFAULT_MARKETPLACE_FILTERS,
  );
  const [filterOpen, setFilterOpen] = useState(false);
  const [offerPage, setOfferPage] = useState(1);
  const [notificationEnabled, setNotificationEnabled] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sourceSaving, setSourceSaving] = useState(false);
  const [verifyingKey, setVerifyingKey] = useState<string | null>(null);
  const [favoriteSavingKey, setFavoriteSavingKey] = useState<string | null>(null);
  const [alertDraft, setAlertDraft] = useState<AlertDraft>(() => ({
    ...EMPTY_ALERT_DRAFT,
    name: t("低价提醒"),
  }));

  const reload = async () => {
    const [nextOffers, nextFavorites, nextChanges, nextAlerts, nextNotification, nextSources] = await Promise.all([
      marketplaceClient.offers({ productId: MARKETPLACE_PRODUCT_ID, limit: 10_000 }),
      marketplaceClient.favoriteMerchants(),
      marketplaceClient.changes(200),
      marketplaceClient.alerts(),
      marketplaceClient.notificationEnabled(),
      marketplaceClient.sources(),
    ]);
    const nextSource =
      nextSources.find((item) => item.id === MARKETPLACE_SOURCE_ID) ?? nextSources[0] ?? null;
    setOffers(nextOffers);
    setFavoriteMerchants(nextFavorites);
    setChanges(
      nextChanges.filter((change) => change.offerKey.startsWith(`${MARKETPLACE_SOURCE_ID}:`)),
    );
    setAlerts(
      nextAlerts.filter(
        (rule) =>
          (!rule.productId || rule.productId === MARKETPLACE_PRODUCT_ID) &&
          (!rule.sourceConfigId || rule.sourceConfigId === MARKETPLACE_SOURCE_ID),
      ),
    );
    setNotificationEnabled(nextNotification);
    setSource(nextSource);
    if (nextSource) setSourceDraft(sourceToDraft(nextSource));
  };

  useEffect(() => {
    void reload().catch((error) => toast.error(getAppErrorMessage(error)));
  }, []);

  const scopedOffers = useMemo(
    () =>
      offers.filter(
        (offer) => offer.productId === MARKETPLACE_PRODUCT_ID && offer.isCurrent,
      ),
    [offers],
  );

  const tagFacets = useMemo(() => {
    const counts = new Map<string, number>();
    for (const offer of scopedOffers) {
      for (const tag of offer.filterTags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
    return MARKETPLACE_FILTER_TAGS
      .filter((definition) => counts.has(definition.id) || filters.tags.includes(definition.id))
      .map((definition) => ({ ...definition, count: counts.get(definition.id) ?? 0 }));
  }, [scopedOffers, filters.tags]);

  const quickTagFacets = useMemo(
    () => tagFacets.filter((facet) => MARKETPLACE_QUICK_FILTER_TAGS.has(facet.id)),
    [tagFacets],
  );

  const advancedTagGroups = useMemo(
    () =>
      (["plusChannel", "plusRecharge", "team"] as MarketplaceFilterTagGroup[])
        .map((group) => ({ group, facets: tagFacets.filter((facet) => facet.group === group) }))
        .filter((entry) => entry.facets.length > 0),
    [tagFacets],
  );

  const merchantOptions = useMemo(
    () =>
      Array.from(
        new Set(
          scopedOffers
            .flatMap((offer) => [offer.sourceName, offer.sourceId])
            .filter((merchant): merchant is string => Boolean(merchant?.trim())),
        ),
      ).sort((left, right) => left.localeCompare(right, "zh-CN")),
    [scopedOffers],
  );

  const favoriteMerchantKeys = useMemo(
    () => new Set(favoriteMerchants.map((merchant) => merchant.merchantKey)),
    [favoriteMerchants],
  );

  const filteredOffers = useMemo(
    () =>
      scopedOffers.filter((offer) => {
        const status = getPriceAiStatus(offer);
        if (priceAiStatus !== "all" && status !== priceAiStatus) return false;
        if (localStatus !== "all" && offer.localStatus !== localStatus) return false;
        if (
          favoriteOnly &&
          (!offer.merchantKey || !favoriteMerchantKeys.has(offer.merchantKey))
        ) {
          return false;
        }
        return marketplaceOfferMatchesAdvancedFilters(offer, filters);
      }),
    [
      scopedOffers,
      priceAiStatus,
      localStatus,
      favoriteOnly,
      favoriteMerchantKeys,
      filters,
    ],
  );

  const pageCount = Math.max(1, Math.ceil(filteredOffers.length / OFFERS_PER_PAGE));
  const currentPage = Math.min(offerPage, pageCount);
  const visibleOffers = useMemo(
    () =>
      filteredOffers.slice(
        (currentPage - 1) * OFFERS_PER_PAGE,
        currentPage * OFFERS_PER_PAGE,
      ),
    [filteredOffers, currentPage],
  );

  const filterSignature = [
    priceAiStatus,
    localStatus,
    favoriteOnly,
    filters.tags.join(","),
    filters.collector,
    filters.minStock ?? "",
    filters.freshWithinMinutes ?? "",
    filters.minPrice ?? "",
    filters.maxPrice ?? "",
    filters.query,
    filters.excludeQuery,
  ].join("|");

  useEffect(() => {
    setOfferPage(1);
  }, [filterSignature]);

  const setFilter = <Key extends keyof MarketplaceAdvancedFilters>(
    key: Key,
    value: MarketplaceAdvancedFilters[Key],
  ) => setFilters((current) => ({ ...current, [key]: value }));

  const toggleFilterTag = (tag: string) => {
    setFilters((current) => ({
      ...current,
      tags: current.tags.includes(tag)
        ? current.tags.filter((item) => item !== tag)
        : [...current.tags, tag],
    }));
  };

  const clearFilters = () => {
    setFilters(EMPTY_MARKETPLACE_ADVANCED_FILTERS);
    setPriceAiStatus("all");
    setLocalStatus("all");
    setFavoriteOnly(false);
  };

  const toggleScheduledTag = (tag: string) => {
    setSourceDraft((current) => ({
      ...current,
      tags: current.tags.includes(tag)
        ? current.tags.filter((item) => item !== tag)
        : [...current.tags, tag],
    }));
  };

  const offerCounts = useMemo(
    () => ({
      available: scopedOffers.filter((offer) => getPriceAiStatus(offer) === "available").length,
      outOfStock: scopedOffers.filter(
        (offer) => getPriceAiStatus(offer) === "out_of_stock",
      ).length,
    }),
    [scopedOffers],
  );

  const refresh = async () => {
    setBusy(true);
    try {
      const result = await marketplaceClient.refresh();
      await reload();
      if (result.errors.length) {
        toast.warning(
          t("已更新 {count} 个商品，{failed} 个商品源失败", {
            count: result.count,
            failed: result.errors.length,
          }),
        );
      } else {
        toast.success(t("已更新 {count} 个商品", { count: result.count }));
      }
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const saveSourceSettings = async () => {
    setSourceSaving(true);
    try {
      const saved = await marketplaceClient.saveSource({
        id: MARKETPLACE_SOURCE_ID,
        productId: MARKETPLACE_PRODUCT_ID,
        tags: sourceDraft.tags,
        enabled: sourceDraft.enabled,
        verifyEnabled: sourceDraft.verifyEnabled,
      });
      setSource(saved);
      setSourceDraft(sourceToDraft(saved));
      toast.success(t("定时同步设置已保存"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setSourceSaving(false);
    }
  };

  const verifyOffer = async (offer: MarketplaceOffer) => {
    setVerifyingKey(offer.offerKey);
    try {
      await marketplaceClient.verifyOffer(offer.offerKey);
      await reload();
      toast.success(t("库存验证已完成"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setVerifyingKey(null);
    }
  };

  const toggleFavoriteMerchant = async (offer: MarketplaceOffer) => {
    if (!offer.merchantKey) return;
    const favorite = !favoriteMerchantKeys.has(offer.merchantKey);
    setFavoriteSavingKey(offer.merchantKey);
    try {
      const nextFavorites = await marketplaceClient.setFavoriteMerchant(offer.offerKey, favorite);
      setFavoriteMerchants(nextFavorites);
      toast.success(favorite ? t("商家已收藏") : t("已取消收藏商家"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setFavoriteSavingKey(null);
    }
  };

  const saveAlert = async () => {
    try {
      const saved = await marketplaceClient.saveAlert(alertPayload(alertDraft));
      setAlerts((items) => [saved, ...items.filter((item) => item.id !== saved.id)]);
      setAlertDraft(alertToDraft(saved));
      toast.success(t("提醒规则已保存"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };

  const toggleAlert = async (rule: MarketplaceAlertRule, enabled: boolean) => {
    try {
      const saved = await marketplaceClient.saveAlert({
        ...alertPayload(alertToDraft(rule)),
        enabled,
      });
      setAlerts((items) => items.map((item) => (item.id === saved.id ? saved : item)));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };

  const deleteAlert = async (id: string) => {
    try {
      await marketplaceClient.deleteAlert(id);
      setAlerts((items) => items.filter((item) => item.id !== id));
      if (alertDraft.id === id) {
        setAlertDraft({ ...EMPTY_ALERT_DRAFT, name: t("低价提醒") });
      }
      toast.success(t("提醒规则已删除"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };

  const toggleNotifications = async (checked: boolean) => {
    try {
      await marketplaceClient.setNotificationEnabled(checked);
      setNotificationEnabled(checked);
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-xl font-semibold">{t("商品池")}</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {t(MARKETPLACE_PRODUCT_LABEL)} · {t("商品源由代码固定，避免混入其它产品")}
          </p>
        </div>
        <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto">
          <Button
            variant="outline"
            className="w-full sm:w-auto"
            title={t("抓取全部 Plus 商品；不执行二次验证，也不发送桌面通知")}
            onClick={() => void refresh()}
            disabled={busy}
          >
            <RefreshCw className={`mr-2 h-4 w-4 ${busy ? "animate-spin" : ""}`} />
            {t("立即同步全部")}
          </Button>
          <Button
            variant="outline"
            className="w-full sm:w-auto"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className="mr-2 h-4 w-4" />
            {t("商品池设置")}
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="gap-3">
          <CardTitle className="text-base">{t("报价列表")}</CardTitle>
          <div className="grid gap-3 xl:grid-cols-[minmax(260px,1.5fr)_repeat(2,minmax(130px,0.7fr))_repeat(2,minmax(150px,1fr))]">
            <label className="space-y-1 text-xs text-muted-foreground">
              <span>{t("包含关键词")}</span>
              <Input
                placeholder={t("渠道、商家或商品名")}
                value={filters.query}
                onChange={(event) => setFilter("query", event.target.value)}
              />
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              <span>{t("最低价")}</span>
              <Input
                type="number"
                min="0"
                value={filters.minPrice ?? ""}
                placeholder="0"
                onChange={(event) => setFilter("minPrice", inputNumber(event.target.value))}
              />
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              <span>{t("最高价")}</span>
              <Input
                type="number"
                min="0"
                value={filters.maxPrice ?? ""}
                placeholder="∞"
                onChange={(event) => setFilter("maxPrice", inputNumber(event.target.value))}
              />
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              <span>{t("PriceAI 状态")}</span>
              <select
                className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                value={priceAiStatus}
                onChange={(event) => setPriceAiStatus(event.target.value)}
              >
                <option value="all">{t("全部")}</option>
                <option value="available">{t("有货")} ({offerCounts.available})</option>
                <option value="out_of_stock">{t("缺货")} ({offerCounts.outOfStock})</option>
                <option value="unavailable">{t("暂不可用")}</option>
                <option value="invalid">{t("状态异常")}</option>
              </select>
            </label>
            <label className="space-y-1 text-xs text-muted-foreground">
              <span>{t("本地验证")}</span>
              <select
                className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                value={localStatus}
                onChange={(event) => setLocalStatus(event.target.value)}
              >
                <option value="all">{t("全部")}</option>
                <option value="available">{t("验证可用")}</option>
                <option value="unavailable">{t("验证缺货")}</option>
                <option value="unknown">{t("无法确认")}</option>
                <option value="invalid">{t("链接失效")}</option>
              </select>
            </label>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant={
                filterOpen || marketplaceFiltersActive(filters) || favoriteOnly
                  ? "secondary"
                  : "outline"
              }
              onClick={() => setFilterOpen((open) => !open)}
              aria-expanded={filterOpen}
            >
              <ListFilter className="mr-1.5 h-4 w-4" />
              {t("筛选")} · {filteredOffers.length}
            </Button>
            <Button
              size="sm"
              variant={favoriteOnly ? "default" : "outline"}
              className={favoriteOnly ? "ring-2 ring-primary/30 ring-offset-1" : undefined}
              aria-pressed={favoriteOnly}
              onClick={() => setFavoriteOnly((selected) => !selected)}
            >
              <Star className={favoriteOnly ? "mr-1.5 h-4 w-4 fill-current" : "mr-1.5 h-4 w-4"} />
              {t("只看收藏商家")}
            </Button>
            <Button
              size="sm"
              variant={filters.minStock === 50 ? "secondary" : "ghost"}
              aria-pressed={filters.minStock === 50}
              onClick={() => setFilter("minStock", filters.minStock === 50 ? null : 50)}
            >
              <Boxes className="mr-1.5 h-4 w-4" />
              {t("库存 ≥50")}
            </Button>
            <Button
              size="sm"
              variant={filters.freshWithinMinutes === 60 ? "secondary" : "ghost"}
              aria-pressed={filters.freshWithinMinutes === 60}
              onClick={() =>
                setFilter(
                  "freshWithinMinutes",
                  filters.freshWithinMinutes === 60 ? null : 60,
                )
              }
            >
              <Clock3 className="mr-1.5 h-4 w-4" />
              {t("1 小时内更新")}
            </Button>
            {quickTagFacets.map((facet) => {
              const selected = filters.tags.includes(facet.id);
              return (
                <Button
                  key={facet.id}
                  size="sm"
                  variant={selected ? "default" : "outline"}
                  className={selected ? "ring-2 ring-primary/30 ring-offset-1" : undefined}
                  aria-pressed={selected}
                  title={t(facet.description)}
                  onClick={() => toggleFilterTag(facet.id)}
                >
                  {selected && <Check className="mr-1 h-3.5 w-3.5" />}
                  {t(facet.label)} {facet.count}
                </Button>
              );
            })}
            {(marketplaceFiltersActive(filters) ||
              priceAiStatus !== "all" ||
              localStatus !== "all" ||
              favoriteOnly) && (
              <Button size="sm" variant="ghost" onClick={clearFilters}>
                <RotateCcw className="mr-1.5 h-4 w-4" />
                {t("清除筛选")}
              </Button>
            )}
          </div>
          {filterOpen && (
            <div className="space-y-4 rounded-md border bg-muted/20 p-3">
              <fieldset className="space-y-2">
                <legend className="text-xs font-medium text-muted-foreground">{t("渠道来源")}</legend>
                <div className="flex flex-wrap gap-2">
                  {MARKETPLACE_COLLECTOR_FILTERS.map((collector) => (
                    <Button
                      key={collector.id}
                      size="sm"
                      variant={filters.collector === collector.id ? "secondary" : "outline"}
                      aria-pressed={filters.collector === collector.id}
                      onClick={() =>
                        setFilter("collector", collector.id as MarketplaceCollectorFilter)
                      }
                    >
                      {t(collector.label)}
                    </Button>
                  ))}
                </div>
              </fieldset>
              {advancedTagGroups.map(({ group, facets }) => (
                <fieldset key={group} className="space-y-2">
                  <legend className="text-xs font-medium text-muted-foreground">
                    {t(MARKETPLACE_FILTER_GROUP_LABELS[group])}
                  </legend>
                  <div className="flex flex-wrap gap-2">
                    {facets.map((facet) => {
                      const selected = filters.tags.includes(facet.id);
                      return (
                        <Button
                          key={facet.id}
                          size="sm"
                          variant={selected ? "default" : "outline"}
                          className={selected ? "ring-2 ring-primary/30 ring-offset-1" : undefined}
                          aria-pressed={selected}
                          title={t(facet.description)}
                          onClick={() => toggleFilterTag(facet.id)}
                        >
                          {selected && <Check className="mr-1 h-3.5 w-3.5" />}
                          {t(facet.label)} {facet.count}
                        </Button>
                      );
                    })}
                  </div>
                </fieldset>
              ))}
              <div className="grid gap-3 md:grid-cols-3">
                <label className="space-y-1 text-xs text-muted-foreground">
                  <span>{t("最低库存")}</span>
                  <Input
                    type="number"
                    min="0"
                    value={filters.minStock ?? ""}
                    placeholder="0"
                    onChange={(event) => setFilter("minStock", inputNumber(event.target.value))}
                  />
                </label>
                <label className="space-y-1 text-xs text-muted-foreground">
                  <span>{t("PriceAI 更新时间")}</span>
                  <select
                    className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                    value={filters.freshWithinMinutes ?? ""}
                    onChange={(event) =>
                      setFilter(
                        "freshWithinMinutes",
                        event.target.value ? Number(event.target.value) : null,
                      )
                    }
                  >
                    <option value="">{t("不限时间")}</option>
                    <option value="60">{t("1 小时内")}</option>
                    <option value="360">{t("6 小时内")}</option>
                    <option value="1440">{t("24 小时内")}</option>
                  </select>
                </label>
                <label className="space-y-1 text-xs text-muted-foreground">
                  <span>{t("排除关键词")}</span>
                  <Input
                    value={filters.excludeQuery}
                    placeholder={t("网页、无质保、日抛")}
                    onChange={(event) => setFilter("excludeQuery", event.target.value)}
                  />
                </label>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("多个标签需要同时满足；排除关键词命中任意一项即排除。")}
              </p>
            </div>
          )}
        </CardHeader>
        <CardContent>
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="border-b text-xs text-muted-foreground">
                <tr>
                  <th className="py-2 pr-3">{t("商品")}</th>
                  <th className="py-2 pr-3">{t("价格")}</th>
                  <th className="py-2 pr-3">{t("PriceAI 状态")}</th>
                  <th className="py-2 pr-3">{t("本地验证")}</th>
                  <th className="py-2 pr-3">{t("商家")}</th>
                  <th className="py-2 text-right">{t("操作")}</th>
                </tr>
              </thead>
              <tbody>
                {visibleOffers.map((offer) => {
                  const status = getPriceAiStatus(offer);
                  const merchantName = offer.sourceName || offer.sourceId || t("未知商家");
                  const isFavorite = Boolean(
                    offer.merchantKey && favoriteMerchantKeys.has(offer.merchantKey),
                  );
                  const favoriteLabel = isFavorite ? t("取消收藏商家") : t("收藏商家");
                  const includedLabel = sourceIncludedLabel(offer.sourceIncludedAt, t);
                  const shopAgeLabel = sourceShopAgeLabel(offer.sourceShopCreatedAt, t);
                  const detail = [
                    offer.rawStatus || "raw: unknown",
                    offer.effectiveStatus || "effective: unknown",
                    offer.freshnessStatus || "freshness: unknown",
                    offer.stockCount == null ? "stock: unknown" : `stock: ${offer.stockCount}`,
                    offer.priceAiUpdatedAt
                      ? `updated: ${offer.priceAiUpdatedAt}`
                      : "updated: unknown",
                    offer.expiresAt ? `expires: ${offer.expiresAt}` : "expires: none",
                  ].join(" · ");
                  return (
                    <tr key={offer.offerKey} className="border-b last:border-0">
                      <td className="max-w-[360px] py-3 pr-3">
                        <div className="truncate font-medium">{offer.title || offer.offerId}</div>
                        <div className="truncate text-xs text-muted-foreground">
                          {t(MARKETPLACE_PRODUCT_LABEL)}
                        </div>
                      </td>
                      <td className="whitespace-nowrap py-3 pr-3 font-mono">
                        {offer.price == null ? "-" : `${offer.price.toFixed(2)} ${offer.currency}`}
                      </td>
                      <td className="py-3 pr-3" title={detail}>
                        <div className="flex items-center gap-1">
                          {priceAiStatusLabel(status, t)}
                          <Info className="h-3.5 w-3.5 text-muted-foreground" aria-label={detail} />
                        </div>
                      </td>
                      <td className="max-w-[240px] py-3 pr-3">
                        <div>{localStatusLabel(offer.localStatus, t)}</div>
                        <div className="truncate text-xs text-muted-foreground">
                          {offer.localError || formatTimestamp(offer.localCheckedAt, t)}
                        </div>
                      </td>
                      <td className="py-3 pr-3">
                        <div className="flex items-start gap-1.5">
                          <Button
                            size="icon-xs"
                            variant="ghost"
                            className="-ml-1 shrink-0"
                            title={
                              offer.merchantKey
                                ? `${favoriteLabel}: ${merchantName}`
                                : t("无法识别商家")
                            }
                            aria-label={
                              offer.merchantKey
                                ? `${favoriteLabel}: ${merchantName}`
                                : t("无法识别商家")
                            }
                            aria-pressed={isFavorite}
                            disabled={
                              !offer.merchantKey || favoriteSavingKey === offer.merchantKey
                            }
                            onClick={() => void toggleFavoriteMerchant(offer)}
                          >
                            {favoriteSavingKey === offer.merchantKey ? (
                              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <Star
                                className={
                                  isFavorite
                                    ? "h-3.5 w-3.5 fill-current text-amber-500"
                                    : "h-3.5 w-3.5"
                                }
                              />
                            )}
                          </Button>
                          <div className="min-w-0">
                            <div>{merchantName}</div>
                            <div className="text-xs text-muted-foreground">
                              {offer.collectorKind || t("未知渠道")}
                            </div>
                            {(includedLabel || shopAgeLabel) && (
                              <div className="mt-1 flex flex-wrap gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
                                {includedLabel && <span>{includedLabel}</span>}
                                {shopAgeLabel && <span>{shopAgeLabel}</span>}
                              </div>
                            )}
                          </div>
                        </div>
                      </td>
                      <td className="py-3 text-right">
                        <div className="flex justify-end gap-1">
                          <Button
                            size="icon"
                            variant="ghost"
                            title={t("验证库存")}
                            aria-label={t("验证库存")}
                            disabled={verifyingKey === offer.offerKey}
                            onClick={() => void verifyOffer(offer)}
                          >
                            {verifyingKey === offer.offerKey ? (
                              <RefreshCw className="h-4 w-4 animate-spin" />
                            ) : (
                              <BadgeCheck className="h-4 w-4" />
                            )}
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            title={t("打开商品链接")}
                            aria-label={t("打开商品链接")}
                            disabled={!isHttpUrl(offer.url)}
                            onClick={() => {
                              if (isHttpUrl(offer.url)) void appClient.openInBrowser(offer.url);
                            }}
                          >
                            <ExternalLink className="h-4 w-4" />
                          </Button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {filteredOffers.length === 0 && (
              <div className="py-12 text-center text-sm text-muted-foreground">
                {t("暂无匹配报价")}
              </div>
            )}
          </div>
          {filteredOffers.length > 0 && (
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t pt-3 text-xs text-muted-foreground">
              <span>
                {t("显示 {from}-{to} / {total}", {
                  from: (currentPage - 1) * OFFERS_PER_PAGE + 1,
                  to: Math.min(currentPage * OFFERS_PER_PAGE, filteredOffers.length),
                  total: filteredOffers.length,
                })}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  size="icon"
                  variant="outline"
                  title={t("上一页")}
                  aria-label={t("上一页")}
                  disabled={currentPage <= 1}
                  onClick={() => setOfferPage((page) => Math.max(1, page - 1))}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <span>{t("第 {page} / {pages} 页", { page: currentPage, pages: pageCount })}</span>
                <Button
                  size="icon"
                  variant="outline"
                  title={t("下一页")}
                  aria-label={t("下一页")}
                  disabled={currentPage >= pageCount}
                  onClick={() => setOfferPage((page) => Math.min(pageCount, page + 1))}
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Sheet open={settingsOpen} onOpenChange={setSettingsOpen}>
        <SheetContent className="max-w-[min(100vw,680px)]">
          <SheetHeader>
            <SheetTitle>{t("商品池设置")}</SheetTitle>
          </SheetHeader>
          <Tabs defaultValue="sync" className="mt-4 flex min-h-0 flex-1 flex-col">
            <TabsList className="grid h-auto w-full grid-cols-2 sm:grid-cols-4">
              <TabsTrigger
                value="sync"
                className="h-auto min-h-9 whitespace-normal py-2 text-center leading-tight"
              >
                {t("定时同步")}
              </TabsTrigger>
              <TabsTrigger
                value="alerts"
                className="h-auto min-h-9 whitespace-normal py-2 text-center leading-tight"
              >
                {t("提醒规则")}
              </TabsTrigger>
              <TabsTrigger
                value="notifications"
                className="h-auto min-h-9 whitespace-normal py-2 text-center leading-tight"
              >
                {t("桌面通知")}
              </TabsTrigger>
              <TabsTrigger
                value="changes"
                className="h-auto min-h-9 whitespace-normal py-2 text-center leading-tight"
              >
                {t("变化记录")}
              </TabsTrigger>
            </TabsList>
            <div className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1">
              <TabsContent value="sync" className="mt-0 space-y-5">
                <div className="space-y-2 border-b pb-4">
                  <div className="text-sm font-medium">{t(MARKETPLACE_PRODUCT_LABEL)}</div>
                  <p className="text-sm text-muted-foreground">
                    {t("产品和商品源由代码固定；这里只配置每小时自动同步的范围。")}
                  </p>
                </div>
                <div className="space-y-4">
                  <label className="flex items-start justify-between gap-4 text-sm">
                    <span>
                      <span className="block font-medium">{t("启用定时同步")}</span>
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("服务启动后同步一次，之后每小时同步一次；关闭主窗口后仍会继续。")}
                      </span>
                    </span>
                    <Switch
                      checked={sourceDraft.enabled}
                      onCheckedChange={(enabled) =>
                        setSourceDraft((current) => ({ ...current, enabled }))
                      }
                    />
                  </label>
                  <label className="flex items-start justify-between gap-4 text-sm">
                    <span>
                      <span className="block font-medium">{t("自动二次验证")}</span>
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("自动同步会验证低价前 20 个及可能命中提醒的商品。")}
                      </span>
                    </span>
                    <Switch
                      checked={sourceDraft.verifyEnabled}
                      onCheckedChange={(verifyEnabled) =>
                        setSourceDraft((current) => ({ ...current, verifyEnabled }))
                      }
                    />
                  </label>
                </div>
                <fieldset className="space-y-3 border-t pt-4">
                  <div className="flex items-center justify-between gap-2">
                    <legend className="text-sm font-medium">{t("自动同步标签")}</legend>
                    {sourceDraft.tags.length > 0 && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          setSourceDraft((current) => ({ ...current, tags: [] }))
                        }
                      >
                        <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
                        {t("清空（同步全部）")}
                      </Button>
                    )}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {t("多选标签按 AND 同时满足；不选标签时自动同步全部 Plus 商品。")}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    {SCHEDULE_TAGS.map((tag) => {
                      const selected = sourceDraft.tags.includes(tag.id);
                      return (
                        <Button
                          key={tag.id}
                          size="sm"
                          variant={selected ? "default" : "outline"}
                          className={selected ? "ring-2 ring-primary/30 ring-offset-1" : undefined}
                          aria-pressed={selected}
                          title={t(tag.description)}
                          onClick={() => toggleScheduledTag(tag.id)}
                        >
                          {selected && <Check className="mr-1 h-3.5 w-3.5" />}
                          {t(tag.label)}
                        </Button>
                      );
                    })}
                  </div>
                </fieldset>
                <div className="space-y-1 border-t pt-4 text-xs text-muted-foreground">
                  <div>
                    {t("上次同步")}：
                    {source?.lastSyncAt
                      ? new Date(source.lastSyncAt * 1000).toLocaleString()
                      : t("尚未同步")}
                  </div>
                  {source?.lastSyncError && (
                    <div className="text-destructive">{source.lastSyncError}</div>
                  )}
                  <p>{t("立即同步全部不受此开关和标签影响，且不会验证或通知。")}</p>
                </div>
                <Button onClick={() => void saveSourceSettings()} disabled={sourceSaving}>
                  <Save className="mr-2 h-4 w-4" />
                  {t("保存定时同步设置")}
                </Button>
              </TabsContent>

              <TabsContent value="alerts" className="mt-0 space-y-4">
                <div className="flex items-center justify-between gap-2">
                  <p className="text-sm text-muted-foreground">
                    {t("提醒规则只在自动同步时评估；手动同步不会发送桌面通知。")}
                  </p>
                  <Button
                    size="icon"
                    variant="ghost"
                    title={t("新建提醒规则")}
                    aria-label={t("新建提醒规则")}
                    onClick={() => setAlertDraft({ ...EMPTY_ALERT_DRAFT, name: t("低价提醒") })}
                  >
                    <Plus className="h-4 w-4" />
                  </Button>
                </div>
                <div className="grid gap-2 sm:grid-cols-2">
                  <Input
                    placeholder={t("规则名称")}
                    value={alertDraft.name}
                    onChange={(event) => setAlertDraft({ ...alertDraft, name: event.target.value })}
                  />
                  <div className="flex h-9 items-center rounded-md border bg-muted/30 px-3 text-sm text-muted-foreground">
                    {t(MARKETPLACE_PRODUCT_LABEL)}
                  </div>
                </div>
                <div className="grid gap-2 sm:grid-cols-3">
                  <div className="flex h-9 items-center rounded-md border bg-muted/30 px-3 text-sm text-muted-foreground">
                    {t("固定商品源")}
                  </div>
                  <Input
                    list="marketplace-merchant-options"
                    placeholder={t("商家/渠道（可选）")}
                    value={alertDraft.merchant}
                    onChange={(event) =>
                      setAlertDraft({ ...alertDraft, merchant: event.target.value })
                    }
                  />
                  <Input
                    placeholder={t("币种")}
                    value={alertDraft.currency}
                    onChange={(event) =>
                      setAlertDraft({ ...alertDraft, currency: event.target.value })
                    }
                  />
                </div>
                <Input
                  placeholder={t("标签范围，逗号分隔")}
                  value={alertDraft.tags}
                  onChange={(event) => setAlertDraft({ ...alertDraft, tags: event.target.value })}
                />
                <div className="grid gap-2 sm:grid-cols-3">
                  <Input
                    type="number"
                    min="0"
                    placeholder={t("最高价")}
                    value={alertDraft.maxPrice}
                    onChange={(event) =>
                      setAlertDraft({ ...alertDraft, maxPrice: event.target.value })
                    }
                  />
                  <Input
                    type="number"
                    min="0"
                    placeholder={t("下降金额")}
                    value={alertDraft.dropAmount}
                    onChange={(event) =>
                      setAlertDraft({ ...alertDraft, dropAmount: event.target.value })
                    }
                  />
                  <Input
                    type="number"
                    min="0"
                    max="100"
                    placeholder={t("下降比例 %")}
                    value={alertDraft.dropPercent}
                    onChange={(event) =>
                      setAlertDraft({ ...alertDraft, dropPercent: event.target.value })
                    }
                  />
                </div>
                <div className="grid gap-3 sm:grid-cols-3">
                  <label className="flex items-center justify-between gap-2 text-sm">
                    {t("缺货变有货")}
                    <Switch
                      checked={alertDraft.notifyRestock}
                      onCheckedChange={(checked) =>
                        setAlertDraft({ ...alertDraft, notifyRestock: checked })
                      }
                    />
                  </label>
                  <label className="flex items-center justify-between gap-2 text-sm">
                    {t("验证状态变化")}
                    <Switch
                      checked={alertDraft.notifyVerified}
                      onCheckedChange={(checked) =>
                        setAlertDraft({ ...alertDraft, notifyVerified: checked })
                      }
                    />
                  </label>
                  <label className="flex items-center justify-between gap-2 text-sm">
                    {t("链接失效")}
                    <Switch
                      checked={alertDraft.notifyInvalidLink}
                      onCheckedChange={(checked) =>
                        setAlertDraft({ ...alertDraft, notifyInvalidLink: checked })
                      }
                    />
                  </label>
                </div>
                <Button onClick={() => void saveAlert()}>
                  <Save className="mr-2 h-4 w-4" />
                  {t("保存提醒规则")}
                </Button>
                <div className="divide-y border-t">
                  {alerts.map((rule) => (
                    <div key={rule.id} className="flex min-h-12 items-center gap-2 py-2 text-sm">
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-medium">{rule.name}</div>
                        <div className="truncate text-xs text-muted-foreground">
                          {t(MARKETPLACE_PRODUCT_LABEL)}
                          {rule.maxPrice != null ? ` · ${t("最高")} ${rule.maxPrice} ${rule.currency}` : ""}
                        </div>
                        {rule.enabled && !scheduledSyncCoversRule(source, rule) && (
                          <div className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                            {t("当前定时同步范围不能完整覆盖此规则")}
                          </div>
                        )}
                      </div>
                      <Switch
                        aria-label={`${rule.name}${t("启用状态")}`}
                        checked={rule.enabled}
                        onCheckedChange={(checked) => void toggleAlert(rule, checked)}
                      />
                      <Button
                        size="icon"
                        variant="ghost"
                        title={t("编辑提醒规则")}
                        aria-label={t("编辑提醒规则")}
                        onClick={() => setAlertDraft(alertToDraft(rule))}
                      >
                        <Pencil className="h-4 w-4" />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        title={t("删除提醒规则")}
                        aria-label={t("删除提醒规则")}
                        onClick={() => void deleteAlert(rule.id)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                  {alerts.length === 0 && (
                    <div className="py-8 text-center text-sm text-muted-foreground">
                      {t("暂无提醒规则")}
                    </div>
                  )}
                </div>
              </TabsContent>

              <TabsContent value="notifications" className="mt-0 space-y-5">
                <label className="flex items-start justify-between gap-4 border-b pb-5 text-sm">
                  <span>
                    <span className="flex items-center gap-2 font-medium">
                      {notificationEnabled ? (
                        <Bell className="h-4 w-4" />
                      ) : (
                        <BellOff className="h-4 w-4" />
                      )}
                      {t("桌面通知")}
                    </span>
                    <span className="mt-2 block text-xs text-muted-foreground">
                      {t("同一次自动同步命中多个商品时合并为一条桌面通知。")}
                    </span>
                  </span>
                  <Switch
                    aria-label={t("桌面通知")}
                    checked={notificationEnabled}
                    onCheckedChange={toggleNotifications}
                  />
                </label>
                <div className="space-y-2 text-sm text-muted-foreground">
                  <p>{t("首次自动同步只建立提醒基线，不发送通知。")}</p>
                  <p>{t("立即同步全部和手动库存验证都不会发送桌面通知。")}</p>
                  <p>{t("关闭桌面通知不影响应用内变化记录。")}</p>
                </div>
              </TabsContent>

              <TabsContent value="changes" className="mt-0">
                <div className="divide-y">
                  {changes.map((change) => (
                    <div key={change.id} className="flex items-center justify-between gap-3 py-3 text-sm">
                      <div className="min-w-0">
                        <div>{changeLabel(change.changeType, t)}</div>
                        <div className="truncate text-xs text-muted-foreground">{change.offerKey}</div>
                      </div>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {new Date(change.createdAt * 1000).toLocaleString()}
                      </span>
                    </div>
                  ))}
                  {changes.length === 0 && (
                    <div className="py-8 text-center text-sm text-muted-foreground">
                      {t("暂无变化记录")}
                    </div>
                  )}
                </div>
              </TabsContent>
            </div>
          </Tabs>
          <datalist id="marketplace-merchant-options">
            {merchantOptions.map((merchant) => (
              <option key={merchant} value={merchant} />
            ))}
          </datalist>
        </SheetContent>
      </Sheet>
    </div>
  );
}
