"use client";

import { useEffect, useRef, useState } from "react";
import { Check, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/lib/i18n/provider";

interface AccountSortCellProps {
  value: number;
  isEditing: boolean;
  isSaving: boolean;
  disabled?: boolean;
  onEdit: () => void;
  onCancel: () => void;
  onSave: (value: number) => Promise<void>;
}

export function AccountSortCell({
  value,
  isEditing,
  isSaving,
  disabled = false,
  onEdit,
  onCancel,
  onSave,
}: AccountSortCellProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(String(value));
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const submittingRef = useRef(false);

  useEffect(() => {
    if (!isEditing) {
      setDraft(String(value));
    }
  }, [isEditing, value]);

  useEffect(() => {
    if (!isEditing || isSaving) return;
    const frame = window.requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [isEditing, isSaving]);

  const startEditing = () => {
    if (disabled || isSaving) return;
    setDraft(String(value));
    onEdit();
  };

  const save = async () => {
    if (disabled || isSaving || submittingRef.current) return;

    const raw = draft.trim();
    if (!raw) {
      toast.error(t("请输入顺序值"));
      inputRef.current?.focus();
      return;
    }

    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) {
      toast.error(t("顺序必须是数字"));
      inputRef.current?.focus();
      return;
    }

    const nextValue = Math.max(0, Math.trunc(parsed));
    setDraft(String(nextValue));
    if (nextValue === value) {
      onCancel();
      return;
    }

    submittingRef.current = true;
    try {
      await onSave(nextValue);
      onCancel();
    } catch {
      inputRef.current?.focus();
      inputRef.current?.select();
    } finally {
      submittingRef.current = false;
    }
  };

  if (!isEditing) {
    return (
      <Button
        type="button"
        variant="ghost"
        className="h-8 min-w-8 bg-muted/60 px-2 font-mono text-xs font-semibold tabular-nums text-foreground hover:bg-primary/10 hover:text-primary"
        disabled={disabled || isSaving}
        onClick={startEditing}
        title={t("更新账号顺序")}
        aria-label={t("更新账号顺序")}
      >
        {value}
      </Button>
    );
  }

  return (
    <div
      ref={containerRef}
      className="flex w-[120px] items-center gap-1"
      onBlurCapture={(event) => {
        if (submittingRef.current || isSaving) return;
        const nextTarget = event.relatedTarget;
        if (
          nextTarget instanceof Node &&
          containerRef.current?.contains(nextTarget)
        ) {
          return;
        }
        onCancel();
      }}
    >
      <Input
        ref={inputRef}
        type="number"
        min={0}
        step={1}
        value={draft}
        disabled={disabled || isSaving}
        className="h-8 w-14 px-1.5 text-center font-mono text-xs font-semibold tabular-nums"
        aria-label={t("顺序")}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void save();
            return;
          }
          if (event.key === "Escape" && !isSaving) {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="text-primary hover:bg-primary/10 hover:text-primary"
        disabled={disabled || isSaving}
        onClick={() => void save()}
        title={t("保存")}
        aria-label={t("保存")}
      >
        {isSaving ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Check className="h-3.5 w-3.5" />
        )}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground hover:bg-muted hover:text-foreground"
        disabled={isSaving}
        onClick={onCancel}
        title={t("取消")}
        aria-label={t("取消")}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
