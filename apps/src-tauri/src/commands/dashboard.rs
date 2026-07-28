use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_dashboard_admin_usage_summary(
    addr: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    include_breakdowns: Option<bool>,
    include_series: Option<bool>,
    series_bucket_seconds: Option<i64>,
    source_kinds: Option<Vec<String>>,
    selected_sources: Option<Vec<serde_json::Value>>,
    include_unavailable_sources: Option<bool>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "dashboard/adminUsageSummary",
        addr,
        Some(serde_json::json!({
            "startTs": start_ts,
            "endTs": end_ts,
            "includeBreakdowns": include_breakdowns,
            "includeSeries": include_series,
            "seriesBucketSeconds": series_bucket_seconds,
            "sourceKinds": source_kinds,
            "selectedSources": selected_sources,
            "includeUnavailableSources": include_unavailable_sources,
        })),
    )
    .await
}

#[tauri::command]
pub async fn service_dashboard_source_options(
    addr: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    source_kinds: Option<Vec<String>>,
    search: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
    include_unavailable_sources: Option<bool>,
    selected_sources: Option<Vec<serde_json::Value>>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "dashboard/sourceOptions",
        addr,
        Some(serde_json::json!({
            "startTs": start_ts,
            "endTs": end_ts,
            "sourceKinds": source_kinds,
            "search": search,
            "page": page,
            "pageSize": page_size,
            "includeUnavailableSources": include_unavailable_sources,
            "selectedSources": selected_sources,
        })),
    )
    .await
}

#[tauri::command]
pub async fn service_dashboard_member_summary(
    addr: Option<String>,
    user_id: Option<String>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "dashboard/memberSummary",
        addr,
        Some(serde_json::json!({
            "userId": user_id,
            "dayStartTs": day_start_ts,
            "dayEndTs": day_end_ts,
        })),
    )
    .await
}
