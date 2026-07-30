const ACCOUNT_SORT_STEP = 5;

interface AccountOrderItem {
  id: string;
  sort: number;
  priority?: number;
  isAvailable: boolean;
  status?: string;
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
  const available: T[] = [];
  const unavailableOrDisabled: T[] = [];

  for (const account of accounts) {
    (isAccountInFirstDisplayGroup(account, mode)
      ? available
      : unavailableOrDisabled
    ).push(account);
  }

  return [...available, ...unavailableOrDisabled];
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
