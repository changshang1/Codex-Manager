use codexmanager_core::storage::UsageSnapshotRecord;

pub(crate) enum Availability {
    Available,
    Unavailable(&'static str),
}

pub(crate) fn is_account_available(
    status: &str,
    has_token: bool,
    snapshot: Option<&UsageSnapshotRecord>,
) -> bool {
    let status = status.trim().to_ascii_lowercase();
    if matches!(
        status.as_str(),
        "disabled" | "inactive" | "unavailable" | "limited" | "banned"
    ) || !has_token
    {
        return false;
    }
    let Some(snapshot) = snapshot else {
        return false;
    };
    let (Some(primary_used), Some(_)) = (snapshot.used_percent, snapshot.window_minutes) else {
        return false;
    };
    if primary_used >= 100.0 {
        return false;
    }

    // Keep the account page semantics: a missing or partial secondary window
    // does not make an otherwise valid primary quota window unavailable.
    match (
        snapshot.secondary_used_percent,
        snapshot.secondary_window_minutes,
    ) {
        (Some(secondary_used), Some(_)) => secondary_used < 100.0,
        _ => true,
    }
}

/// 函数 `evaluate_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn evaluate_snapshot(snap: &UsageSnapshotRecord) -> Availability {
    let primary_missing = snap.used_percent.is_none() || snap.window_minutes.is_none();
    if primary_missing {
        return Availability::Unavailable("usage_missing_primary");
    }
    // 兼容只返回主窗口额度的账号：
    // 只要 primary 有效，就不再因为 secondary 字段半缺失把账号直接打成不可用。
    // 这样可以避免快照字段短暂不完整时误伤仍有额度的账号。
    if let Some(value) = snap.used_percent {
        if value >= 100.0 {
            return Availability::Unavailable("usage_exhausted_primary");
        }
    }
    if let Some(value) = snap.secondary_used_percent {
        if value >= 100.0 {
            return Availability::Unavailable("usage_exhausted_secondary");
        }
    }
    Availability::Available
}

#[cfg(test)]
#[path = "tests/account_availability_tests.rs"]
mod tests;
