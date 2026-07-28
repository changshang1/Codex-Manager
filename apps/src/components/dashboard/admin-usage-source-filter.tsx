"use client";

import { useEffect, useMemo, useState } from "react";
import { ChevronDown, LoaderCircle, Search, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useDashboardSourceOptions } from "@/hooks/useDashboardSourceOptions";
import { formatCompactTokenAmount } from "@/lib/dashboard/format";
import { useI18n } from "@/lib/i18n/provider";
import type {
  DashboardSourceKind,
  DashboardSourceOption,
  DashboardSourceRef,
} from "@/types";

export type DashboardSourceType = "all" | "openai_account" | "aggregate_api";

interface AdminUsageSourceFilterProps {
  startTs: number | null;
  endTs: number | null;
  sourceType: DashboardSourceType;
  selectedSources: DashboardSourceRef[];
  includeUnavailableSources: boolean;
  onSourceTypeChange: (value: DashboardSourceType) => void;
  onSelectedSourcesChange: (value: DashboardSourceRef[]) => void;
  onIncludeUnavailableSourcesChange: (value: boolean) => void;
}

function sourceKey(source: DashboardSourceRef): string {
  return `${source.sourceKind}:${source.sourceId}`;
}

function kindsForType(sourceType: DashboardSourceType): DashboardSourceKind[] {
  if (sourceType === "all") return [];
  return [sourceType];
}

export function AdminUsageSourceFilter({
  startTs,
  endTs,
  sourceType,
  selectedSources,
  includeUnavailableSources,
  onSourceTypeChange,
  onSelectedSourcesChange,
  onIncludeUnavailableSourcesChange,
}: AdminUsageSourceFilterProps) {
  const { t } = useI18n();
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(searchInput.trim()), 250);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  const query = useDashboardSourceOptions({
    startTs,
    endTs,
    sourceKinds: kindsForType(sourceType),
    search,
    includeUnavailableSources,
    selectedSources,
  });
  const options = useMemo(() => {
    const byKey = new Map<string, DashboardSourceOption>();
    // Keep the server's stable usage/name ordering. Selected items are only appended when
    // they are outside the current page, so checking an item never moves it to the top.
    for (const item of [...query.options, ...query.selectedItems]) {
      byKey.set(sourceKey(item), item);
    }
    return Array.from(byKey.values());
  }, [query.options, query.selectedItems]);
  const selectedKeys = useMemo(
    () => new Set(selectedSources.map(sourceKey)),
    [selectedSources],
  );

  useEffect(() => {
    if (
      includeUnavailableSources ||
      selectedSources.length === 0 ||
      !query.isSuccess ||
      query.isFetching
    ) {
      return;
    }
    const availableKeys = new Set(query.selectedItems.map(sourceKey));
    const next = selectedSources.filter((source) => availableKeys.has(sourceKey(source)));
    if (next.length !== selectedSources.length) {
      onSelectedSourcesChange(next);
    }
  }, [
    includeUnavailableSources,
    onSelectedSourcesChange,
    query.isFetching,
    query.isSuccess,
    query.selectedItems,
    selectedSources,
  ]);

  const toggleSource = (item: DashboardSourceOption, checked: boolean) => {
    const key = sourceKey(item);
    if (checked) {
      if (!selectedKeys.has(key)) {
        onSelectedSourcesChange([
          ...selectedSources,
          { sourceKind: item.sourceKind, sourceId: item.sourceId },
        ]);
      }
      return;
    }
    onSelectedSourcesChange(
      selectedSources.filter((source) => sourceKey(source) !== key),
    );
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Select
        value={sourceType}
        onValueChange={(value) => {
          const next = value as DashboardSourceType;
          onSourceTypeChange(next);
          if (next !== "all") {
            onSelectedSourcesChange(
              selectedSources.filter((source) => source.sourceKind === next),
            );
          }
        }}
      >
        <SelectTrigger className="w-[132px] bg-background/40">
          <SelectValue>
            {sourceType === "openai_account"
              ? t("账号池")
              : sourceType === "aggregate_api"
                ? t("聚合 API")
                : t("全部来源")}
          </SelectValue>
        </SelectTrigger>
        <SelectContent align="start" alignItemWithTrigger={false}>
          <SelectGroup>
            <SelectItem value="all">{t("全部来源")}</SelectItem>
            <SelectItem value="openai_account">{t("账号池")}</SelectItem>
            <SelectItem value="aggregate_api">{t("聚合 API")}</SelectItem>
          </SelectGroup>
        </SelectContent>
      </Select>

      <DropdownMenu>
        <DropdownMenuTrigger>
          <Button
            variant="outline"
            className="w-[168px] justify-between bg-background/40"
            render={<span />}
            nativeButton={false}
          >
            <span className="truncate">
              {selectedSources.length === 0
                ? t("全部具体来源")
                : `${t("已选来源")} ${selectedSources.length}`}
            </span>
            <ChevronDown className="size-4 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className="w-[360px] max-w-[calc(100vw-1rem)] p-2"
        >
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
            <Input
              value={searchInput}
              onChange={(event) => setSearchInput(event.target.value)}
              onKeyDown={(event) => event.stopPropagation()}
              placeholder={t("搜索账号、来源 ID 或聚合 API")}
              className="pl-8 pr-8"
            />
            {searchInput ? (
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="absolute right-1 top-1"
                onClick={() => setSearchInput("")}
                title={t("清空搜索")}
              >
                <X className="size-3.5" />
              </Button>
            ) : null}
          </div>

          <div className="mt-2 max-h-[300px] space-y-1 overflow-y-auto pr-1">
            {options.map((item) => {
              const key = sourceKey(item);
              const label = item.name || item.sourceId;
              return (
                <label
                  key={key}
                  className="flex cursor-pointer items-start gap-2 rounded-md px-2 py-2 hover:bg-muted/70"
                >
                  <Checkbox
                    checked={selectedKeys.has(key)}
                    onCheckedChange={(checked) => toggleSource(item, checked === true)}
                    className="mt-0.5"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span className="truncate text-sm font-medium">{label}</span>
                      {item.availability === "deleted" ? (
                        <Badge variant="outline" className="shrink-0 text-[10px]">
                          {t("已删除")}
                        </Badge>
                      ) : item.availability === "unavailable" ? (
                        <Badge variant="destructive" className="shrink-0 text-[10px]">
                          {t("不可用")}
                        </Badge>
                      ) : null}
                    </span>
                    <span className="mt-0.5 flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
                      <span className="truncate font-mono">{item.sourceId}</span>
                      <span className="shrink-0 font-mono">
                        {formatCompactTokenAmount(item.rangeUsage.totalTokens)}
                      </span>
                    </span>
                  </span>
                </label>
              );
            })}
            {query.isLoading ? (
              <div className="flex items-center justify-center py-8 text-muted-foreground">
                <LoaderCircle className="size-4 animate-spin" />
              </div>
            ) : options.length === 0 ? (
              <div className="py-8 text-center text-xs text-muted-foreground">
                {t("没有匹配的来源")}
              </div>
            ) : null}
          </div>

          <div className="mt-2 flex items-center justify-between border-t pt-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={selectedSources.length === 0}
              onClick={() => onSelectedSourcesChange([])}
            >
              {t("清除选择")}
            </Button>
            {query.hasNextPage ? (
              <Button
                variant="outline"
                size="sm"
                disabled={query.isFetchingNextPage}
                onClick={() => query.fetchNextPage()}
              >
                {query.isFetchingNextPage ? t("加载中") : t("加载更多")}
              </Button>
            ) : (
              <span className="text-[11px] text-muted-foreground">
                {query.total} {t("个来源")}
              </span>
            )}
          </div>
        </DropdownMenuContent>
      </DropdownMenu>

      <label className="flex h-9 cursor-pointer items-center gap-2 rounded-md border bg-background/40 px-3 text-xs">
        <Checkbox
          checked={includeUnavailableSources}
          onCheckedChange={(checked) =>
            onIncludeUnavailableSourcesChange(checked === true)
          }
        />
        <span>{t("包含不可用来源")}</span>
      </label>
    </div>
  );
}
