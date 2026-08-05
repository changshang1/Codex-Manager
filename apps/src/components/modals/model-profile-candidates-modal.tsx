"use client";

import { Download, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { useI18n } from "@/lib/i18n/provider";
import type { ModelProfileCandidate } from "@/types/model-profile";

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return "-";
  if (typeof value === "string") return value || "-";
  return JSON.stringify(value);
}

function fieldLabel(field: string, t: (message: string) => string): string {
  const labels: Record<string, string> = {
    model: "模型",
    displayName: "显示名称",
    description: "描述",
    provider: "提供方",
    family: "模型系列",
    category: "模型分类",
    tags: "标签",
    contextWindow: "上下文窗口",
    maxContextWindow: "最大上下文窗口",
    defaultReasoningEffort: "默认推理强度",
    capabilities: "关键能力",
    price: "价格",
    routes: "路由",
  };
  return t(labels[field] || field);
}

export function ModelProfileCandidatesModal({
  open,
  onOpenChange,
  items,
  applyingKey,
  onApply,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: ModelProfileCandidate[];
  applyingKey: string | null;
  onApply: (candidate: ModelProfileCandidate) => void;
}) {
  const { t } = useI18n();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card p-0 sm:max-w-[820px]">
        <div className="max-h-[82vh] overflow-y-auto p-5">
          <DialogHeader>
            <DialogTitle>{t("模型档案候选")}</DialogTitle>
            <DialogDescription>
              {t("这里只显示供应商已经发现并且存在精确模型档案的模型。已有配置必须确认后才会更新。")}
            </DialogDescription>
          </DialogHeader>

          <div className="mt-5 space-y-3">
            {items.length === 0 ? (
              <Empty className="min-h-48">
                <EmptyHeader>
                  <EmptyTitle>{t("当前没有可导入或可更新的模型。")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              items.map((candidate) => {
                const key = `${candidate.sourceId}:${candidate.upstreamModel}`;
                const isApplying = applyingKey === key;
                return (
                  <Card key={key} size="sm">
                    <CardContent className="space-y-3">
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-semibold">{candidate.displayName}</span>
                            <Badge variant={candidate.kind === "import" ? "default" : "secondary"}>
                              {candidate.kind === "import" ? t("可导入") : t("配置可更新")}
                            </Badge>
                          </div>
                          <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                            {candidate.upstreamModel}
                          </p>
                          <p className="mt-1 text-xs text-muted-foreground">
                            {t("来源")}：{candidate.sourceName} · {t("档案版本")} {candidate.profileRevision}
                          </p>
                        </div>
                        <Button
                          type="button"
                          size="sm"
                          disabled={Boolean(applyingKey)}
                          onClick={() => onApply(candidate)}
                        >
                          {candidate.kind === "import" ? (
                            <Download className="mr-1.5 h-4 w-4" />
                          ) : (
                            <RefreshCw className={`mr-1.5 h-4 w-4 ${isApplying ? "animate-spin" : ""}`} />
                          )}
                          {isApplying
                            ? t("正在应用")
                            : candidate.kind === "import"
                              ? t("导入并启用")
                              : t("确认应用更新")}
                        </Button>
                      </div>

                      <div className="overflow-hidden rounded-md border border-border/60">
                        {candidate.changes.map((change) => (
                          <div
                            key={change.field}
                            className="grid gap-1 border-b border-border/50 px-3 py-2 text-xs last:border-b-0 md:grid-cols-[130px_1fr_20px_1fr] md:items-center"
                          >
                            <span className="font-medium">{fieldLabel(change.field, t)}</span>
                            <code className="max-h-20 overflow-auto break-all text-muted-foreground">
                              {formatValue(change.before)}
                            </code>
                            <span className="hidden text-center text-muted-foreground md:block">→</span>
                            <code className="max-h-20 overflow-auto break-all text-foreground">
                              {formatValue(change.after)}
                            </code>
                          </div>
                        ))}
                      </div>
                    </CardContent>
                  </Card>
                );
              })
            )}
          </div>
        </div>
        <DialogFooter className="border-t border-border/60 px-5 py-4">
          <DialogClose className={buttonVariants({ variant: "outline" })}>
            {t("关闭")}
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
