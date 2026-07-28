use codexmanager_core::rpc::types::{DashboardSourceRef, JsonRpcRequest, JsonRpcResponse};

use crate::{dashboard, RpcActor};

pub(super) fn try_handle(req: &JsonRpcRequest, actor: &RpcActor) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "dashboard/adminUsageSummary" => {
            let start_ts = super::i64_param(req, "startTs");
            let end_ts = super::i64_param(req, "endTs");
            let include_breakdowns = super::bool_param(req, "includeBreakdowns").unwrap_or(true);
            let include_series = super::bool_param(req, "includeSeries").unwrap_or(false);
            let series_bucket_seconds = super::i64_param(req, "seriesBucketSeconds");
            let include_unavailable_sources =
                super::bool_param(req, "includeUnavailableSources").unwrap_or(true);
            super::value_or_error((|| {
                let source_kinds: Vec<String> = json_param_or_default(req, "sourceKinds")?;
                let selected_sources: Vec<DashboardSourceRef> =
                    json_param_or_default(req, "selectedSources")?;
                dashboard::read_admin_usage_summary(
                    actor,
                    start_ts,
                    end_ts,
                    include_breakdowns,
                    include_series,
                    series_bucket_seconds,
                    &source_kinds,
                    &selected_sources,
                    include_unavailable_sources,
                )
            })())
        }
        "dashboard/sourceOptions" => super::value_or_error((|| {
            let source_kinds: Vec<String> = json_param_or_default(req, "sourceKinds")?;
            let selected_sources: Vec<DashboardSourceRef> =
                json_param_or_default(req, "selectedSources")?;
            dashboard::read_dashboard_source_options(
                actor,
                super::i64_param(req, "startTs"),
                super::i64_param(req, "endTs"),
                &source_kinds,
                super::str_param(req, "search"),
                super::i64_param(req, "page"),
                super::i64_param(req, "pageSize"),
                super::bool_param(req, "includeUnavailableSources").unwrap_or(true),
                &selected_sources,
            )
        })()),
        "dashboard/memberSummary" => {
            let user_id = super::string_param(req, "userId");
            let day_start_ts = super::i64_param(req, "dayStartTs");
            let day_end_ts = super::i64_param(req, "dayEndTs");
            let include_details = super::bool_param(req, "includeDetails").unwrap_or(true);
            super::value_or_error(dashboard::read_member_dashboard_summary(
                actor,
                user_id,
                day_start_ts,
                day_end_ts,
                include_details,
            ))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}

fn json_param_or_default<T>(req: &JsonRpcRequest, key: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    let Some(value) = req.params.as_ref().and_then(|params| params.get(key)) else {
        return Ok(T::default());
    };
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value.clone()).map_err(|_| format!("invalid_params: {key}"))
}
