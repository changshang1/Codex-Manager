const ACCOUNT_SORT_STEP = 5;

interface AccountOrderItem {
  id: string;
  sort: number;
  priority?: number;
  isAvailable: boolean;
  status?: string;
  refreshTokenInvalidReason?: string | null;
  warrantyExpiresOn?: string | null;
}

export type AccountDisplayOrderMode = "availability" | "access";
export const ACCOUNT_DISPLAY_ORDER_STORAGE_KEY =
  "codexmanager.accounts.display-order";

export function normalizeAccountDisplayOrderMode(
  value: unknown,
): AccountDisplayOrderMode {
  return value === "access" ? "access" : "availability";
}

export interface AccountSortUpdate {
  accountId: string;
  sort: number;
}

function readAccountSort(account: AccountOrderItem): number {
  if (Number.isFinite(account.sort)) {
    return account.sort;
  }
  return Number.isFinite(account.priority) ? Number(account.priority) : 0;
}

export function groupAccountsByAvailability<T extends AccountOrderItem>(
  accounts: T[],
): T[] {
  return groupAccountsByDisplayOrder(accounts, "availability");
}

export function groupAccountsByDisplayOrder<T extends AccountOrderItem>(
  accounts: T[],
  mode: AccountDisplayOrderMode,
): T[] {
  const warrantyIncidents: T[] = [];
  const available: T[] = [];
  const unavailableOrDisabled: T[] = [];

  for (const account of accounts) {
    if (isAccountWarrantyIncident(account)) {
      warrantyIncidents.push(account);
      continue;
    }
    (isAccountInFirstDisplayGroup(account, mode)
      ? available
      : unavailableOrDisabled
    ).push(account);
  }

  return [...warrantyIncidents, ...available, ...unavailableOrDisabled];
}

export function getLocalDateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function isAccountWarrantyIncident(
  account: AccountOrderItem,
  today = getLocalDateKey(),
): boolean {
  const warrantyExpiresOn = String(account.warrantyExpiresOn || "").trim();
  const refreshTokenInvalidReason = String(
    account.refreshTokenInvalidReason || "",
  )
    .trim()
    .toLowerCase();
  return (
    /^\d{4}-\d{2}-\d{2}$/.test(warrantyExpiresOn) &&
    warrantyExpiresOn >= today &&
    refreshTokenInvalidReason.startsWith("refresh_token_invalid:")
  );
}

export function getAccountDisplayGroup(
  account: AccountOrderItem,
  mode: AccountDisplayOrderMode,
): "warranty-incident" | "primary" | "secondary" {
  if (isAccountWarrantyIncident(account)) return "warranty-incident";
  return isAccountInFirstDisplayGroup(account, mode) ? "primary" : "secondary";
}

export function isAccountInFirstDisplayGroup(
  account: AccountOrderItem,
  mode: AccountDisplayOrderMode,
): boolean {
  if (mode === "availability") {
    return account.isAvailable;
  }
  return String(account.status || "").trim().toLowerCase() !== "disabled";
}

export function buildAccountOrderUpdates<T extends AccountOrderItem>(
  orderedAccounts: T[],
): AccountSortUpdate[] {
  return orderedAccounts.reduce<AccountSortUpdate[]>((updates, account, index) => {
    const sort = index * ACCOUNT_SORT_STEP;
    if (readAccountSort(account) !== sort) {
      updates.push({ accountId: account.id, sort });
    }
    return updates;
  }, []);
}

export function buildAccountGroupOrderUpdates<T extends AccountOrderItem>(
  originalGroup: T[],
  reorderedGroup: T[],
): AccountSortUpdate[] {
  if (originalGroup.length !== reorderedGroup.length) {
    return [];
  }

  const originalIds = new Set(originalGroup.map((account) => account.id));
  if (
    reorderedGroup.some((account) => !originalIds.has(account.id)) ||
    new Set(reorderedGroup.map((account) => account.id)).size !== originalIds.size
  ) {
    return [];
  }

  const sortSlots = originalGroup
    .map(readAccountSort)
    .sort((left, right) => left - right);
  return reorderedGroup.reduce<AccountSortUpdate[]>((updates, account, index) => {
    const sort = sortSlots[index];
    if (readAccountSort(account) !== sort) {
      updates.push({ accountId: account.id, sort });
    }
    return updates;
  }, []);
}
