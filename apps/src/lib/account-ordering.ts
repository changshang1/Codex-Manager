const ACCOUNT_SORT_STEP = 5;

interface AccountOrderItem {
  id: string;
  sort: number;
  priority?: number;
  isAvailable: boolean;
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
  const available: T[] = [];
  const unavailable: T[] = [];

  for (const account of accounts) {
    (account.isAvailable ? available : unavailable).push(account);
  }

  return [...available, ...unavailable];
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
