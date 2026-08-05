"use client";

import { useEffect, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { CloudDownload, DatabaseZap, Save } from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { modelProfilesClient } from "@/lib/api/model-profiles";
import { getAppErrorMessage } from "@/lib/api/transport";
import type { AppSettings } from "@/types";

function formatTimestamp(timestamp: number | null | undefined): string {
  if (!timestamp) return "-";
  return new Date(timestamp * 1000).toLocaleString("zh-CN");
}

export function ModelProfileSettingsCard({
  t,
  snapshot,
  updateSettings,
}: {
  t: (message: string, params?: Record<string, string | number>) => string;
  snapshot: AppSettings;
  updateSettings: {
    mutate: (patch: Partial<AppSettings>) => void;
    mutateAsync: (patch: Partial<AppSettings>) => Promise<unknown>;
  };
}) {
  const [sourceUrl, setSourceUrl] = useState(snapshot.modelProfileSourceUrl || "");
  useEffect(() => {
    setSourceUrl(snapshot.modelProfileSourceUrl || "");
  }, [snapshot.modelProfileSourceUrl]);

  const statusQuery = useQuery({
    queryKey: ["model-profile-status"],
    queryFn: () => modelProfilesClient.status(),
    staleTime: 30_000,
    retry: 1,
  });
  const refreshMutation = useMutation({
    mutationFn: () => modelProfilesClient.refresh(),
    onSuccess: async () => {
      await statusQuery.refetch();
      toast.success(t("模型档案已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新模型档案失败")}: ${getAppErrorMessage(error)}`);
      void statusQuery.refetch();
    },
  });
  const saveUrlMutation = useMutation({
    mutationFn: () =>
      updateSettings.mutateAsync({
        modelProfileSourceUrl: sourceUrl.trim(),
        _silent: true,
      }),
    onSuccess: async () => {
      await statusQuery.refetch();
      toast.success(t("模型档案地址已保存"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存模型档案地址失败")}: ${getAppErrorMessage(error)}`);
    },
  });
  const status = statusQuery.data;

  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <DatabaseZap className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("模型档案高级设置")}</CardTitle>
        </div>
        <CardDescription>
          {t("独立更新模型基本信息、能力和价格；不会远程修改认证、API 地址或请求处理规则。")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="flex items-center justify-between gap-4 rounded-md border border-border/60 px-4 py-3">
          <div>
            <Label htmlFor="model-profile-auto-update">{t("自动更新模型档案")}</Label>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("服务启动后异步检查，每 24 小时最多一次。")}
            </p>
          </div>
          <Switch
            id="model-profile-auto-update"
            checked={snapshot.modelProfileAutoUpdate}
            onCheckedChange={(checked) =>
              updateSettings.mutate({ modelProfileAutoUpdate: checked })
            }
          />
        </div>

        <div className="flex items-center justify-between gap-4 rounded-md border border-border/60 px-4 py-3">
          <div>
            <Label htmlFor="codex-sync-catalog">{t("同步模型目录到 Codex 配置")}</Label>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("模型改动时更新 config.toml 的 model_catalog_json，指向本地网关目录。")}
            </p>
          </div>
          <Switch
            id="codex-sync-catalog"
            checked={snapshot.codexSyncModelCatalogJson}
            onCheckedChange={(checked) => updateSettings.mutate({ codexSyncModelCatalogJson: checked })}
          />
        </div>

        <div className="flex items-center justify-between gap-4 rounded-md border border-border/60 px-4 py-3">
          <div>
            <Label htmlFor="codex-sync-provider">{t("同步路由提供方到 Codex 配置")}</Label>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("模型改动时更新 config.toml 的 model_provider 为本地网关，关闭后保留自定义提供方。")}
            </p>
          </div>
          <Switch
            id="codex-sync-provider"
            checked={snapshot.codexSyncModelProvider}
            onCheckedChange={(checked) => updateSettings.mutate({ codexSyncModelProvider: checked })}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="model-profile-source-url">{t("模型档案 URL")}</Label>
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              id="model-profile-source-url"
              value={sourceUrl}
              onChange={(event) => setSourceUrl(event.target.value)}
              placeholder={t("留空使用项目默认地址")}
            />
            <Button
              type="button"
              variant="outline"
              disabled={saveUrlMutation.isPending}
              onClick={() => saveUrlMutation.mutate()}
            >
              <Save className="mr-1.5 h-4 w-4" />
              {t("保存地址")}
            </Button>
            <Button
              type="button"
              disabled={refreshMutation.isPending}
              onClick={() => refreshMutation.mutate()}
            >
              <CloudDownload
                className={`mr-1.5 h-4 w-4 ${refreshMutation.isPending ? "animate-pulse" : ""}`}
              />
              {t("立即更新")}
            </Button>
          </div>
        </div>

        <div className="grid gap-3 rounded-md border border-border/60 p-4 text-sm sm:grid-cols-2 lg:grid-cols-4">
          <div>
            <p className="text-xs text-muted-foreground">{t("当前档案")}</p>
            <div className="mt-1 flex items-center gap-2">
              <span className="font-semibold">v{status?.revision ?? "-"}</span>
              <Badge variant="secondary">
                {status?.source === "cache" ? t("在线缓存") : t("程序内置")}
              </Badge>
            </div>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t("最后检查")}</p>
            <p className="mt-1 font-medium">{formatTimestamp(status?.lastCheckedAt)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t("最后成功")}</p>
            <p className="mt-1 font-medium">{formatTimestamp(status?.lastSuccessAt)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t("待处理")}</p>
            <p className="mt-1 font-medium">
              {t("可导入 {importable}，可更新 {updates}", {
                importable: status?.importableCount ?? 0,
                updates: status?.updateCount ?? 0,
              })}
            </p>
          </div>
        </div>

        {status?.lastError ? (
          <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            {status.lastError}
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}
