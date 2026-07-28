"use client";

import { useInfiniteQuery } from "@tanstack/react-query";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { dashboardClient } from "@/lib/api/dashboard-client";
import { useAppStore } from "@/lib/store/useAppStore";
import type { DashboardSourceKind, DashboardSourceRef } from "@/types";

export interface DashboardSourceOptionsParams {
  startTs?: number | null;
  endTs?: number | null;
  sourceKinds: DashboardSourceKind[];
  search: string;
  includeUnavailableSources: boolean;
  selectedSources: DashboardSourceRef[];
}

export function useDashboardSourceOptions(
  params: DashboardSourceOptionsParams,
  enabled = true,
) {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const isPageActive = useDesktopPageActive("/");
  const queryEnabled = useDeferredDesktopActivation(
    enabled && serviceStatus.connected && isPageActive,
  );
  const query = useInfiniteQuery({
    queryKey: [
      "dashboard",
      "source-options",
      serviceStatus.addr,
      params.startTs ?? null,
      params.endTs ?? null,
      params.sourceKinds,
      params.search,
      params.includeUnavailableSources,
      params.selectedSources,
    ],
    queryFn: ({ pageParam }) =>
      dashboardClient.getSourceOptions({
        ...params,
        page: pageParam,
        pageSize: 30,
      }),
    initialPageParam: 1,
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? lastPage.page + 1 : undefined,
    enabled: queryEnabled,
    retry: 1,
    staleTime: 30_000,
  });

  const options = query.data?.pages.flatMap((page) => page.items) ?? [];
  const selectedItems = query.data?.pages[0]?.selectedItems ?? [];
  return {
    ...query,
    options,
    selectedItems,
    total: query.data?.pages[0]?.total ?? 0,
  };
}
