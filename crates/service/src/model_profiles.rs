use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use codexmanager_core::storage::{
    ManagedModelV2, ManagedModelV2Upsert, ModelPriceTierV2, ModelPriceV2, ModelRouteV2,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_CATALOG_URL: &str = "https://raw.githubusercontent.com/changshang1/Codex-Manager/main/crates/service/resources/model-profiles-v1.json";
const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const REFRESH_INTERVAL_SECS: i64 = 24 * 60 * 60;
const SETTING_AUTO_UPDATE: &str = "model_profiles.auto_update";
const SETTING_SOURCE_URL: &str = "model_profiles.source_url";
const SETTING_CACHE_JSON: &str = "model_profiles.cache_json";
const SETTING_LAST_CHECKED_AT: &str = "model_profiles.last_checked_at";
const SETTING_LAST_SUCCESS_AT: &str = "model_profiles.last_success_at";
const SETTING_LAST_ERROR: &str = "model_profiles.last_error";
const SETTING_APPLIED_STATE: &str = "model_profiles.applied_state";

static REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ModelProfileCatalog {
    schema_version: i64,
    revision: i64,
    updated_at: String,
    #[serde(default)]
    models: BTreeMap<String, ModelProfileTemplate>,
    #[serde(default)]
    family_rules: Vec<ModelProfileFamilyRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ModelProfileFamilyRule {
    prefix: String,
    defaults: ModelProfileTemplate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ModelProfileTemplate {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    context_window: Option<i64>,
    #[serde(default)]
    max_context_window: Option<i64>,
    #[serde(default)]
    default_reasoning_effort: Option<String>,
    #[serde(default)]
    capabilities: Option<Value>,
    #[serde(default)]
    price: Option<ModelProfilePrice>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ModelProfilePrice {
    #[serde(default)]
    price_status: Option<String>,
    #[serde(default)]
    price_source: Option<String>,
    #[serde(default)]
    input_microusd_per_1m: Option<i64>,
    #[serde(default)]
    cached_input_microusd_per_1m: Option<i64>,
    #[serde(default)]
    cache_write_microusd_per_1m: Option<i64>,
    #[serde(default)]
    output_microusd_per_1m: Option<i64>,
}

impl ModelProfilePrice {
    fn to_model_price(&self) -> ModelPriceV2 {
        ModelPriceV2 {
            price_status: self
                .price_status
                .clone()
                .unwrap_or_else(|| "custom".to_string()),
            price_source: self.price_source.clone(),
            input_microusd_per_1m: self.input_microusd_per_1m,
            cached_input_microusd_per_1m: self.cached_input_microusd_per_1m,
            cache_write_microusd_per_1m: self.cache_write_microusd_per_1m,
            output_microusd_per_1m: self.output_microusd_per_1m,
        }
    }

    fn to_base_price_tier(&self) -> Option<ModelPriceTierV2> {
        Some(ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: self.input_microusd_per_1m?,
            cached_input_microusd_per_1m: self.cached_input_microusd_per_1m?,
            cache_write_microusd_per_1m: self.cache_write_microusd_per_1m,
            output_microusd_per_1m: self.output_microusd_per_1m?,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelProfileStatus {
    pub auto_update_enabled: bool,
    pub source_url: String,
    pub schema_version: i64,
    pub revision: i64,
    pub catalog_updated_at: String,
    pub last_checked_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub source: String,
    pub importable_count: usize,
    pub update_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelProfileChange {
    pub field: String,
    pub before: Value,
    pub after: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelProfileCandidate {
    pub kind: String,
    pub slug: String,
    pub display_name: String,
    pub source_id: String,
    pub source_name: String,
    pub upstream_model: String,
    pub profile_revision: i64,
    pub profile_hash: String,
    pub changes: Vec<ModelProfileChange>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelProfileCandidateList {
    pub items: Vec<ModelProfileCandidate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyModelProfileParams {
    pub source_id: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyModelProfileResult {
    pub model: ManagedModelV2,
    pub applied_revision: i64,
    pub applied_hash: String,
}

fn setting_i64(key: &str) -> Option<i64> {
    crate::app_settings::get_persisted_app_setting(key)
        .and_then(|value| value.trim().parse::<i64>().ok())
}

pub(crate) fn auto_update_enabled() -> bool {
    crate::app_settings::get_persisted_app_setting(SETTING_AUTO_UPDATE)
        .map(|value| crate::app_settings::parse_bool_with_default(&value, true))
        .unwrap_or(true)
}

pub(crate) fn source_url() -> String {
    crate::app_settings::get_persisted_app_setting(SETTING_SOURCE_URL)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_CATALOG_URL.to_string())
}

pub(crate) fn set_settings(
    auto_update: Option<bool>,
    source_url: Option<String>,
) -> Result<(), String> {
    if let Some(enabled) = auto_update {
        crate::app_settings::save_persisted_bool_setting(SETTING_AUTO_UPDATE, enabled)?;
    }
    if let Some(url) = source_url {
        let normalized = url.trim();
        if !normalized.is_empty() {
            validate_catalog_url(normalized)?;
        }
        crate::app_settings::save_persisted_app_setting(
            SETTING_SOURCE_URL,
            (!normalized.is_empty()).then_some(normalized),
        )?;
        crate::app_settings::save_persisted_app_setting(SETTING_LAST_CHECKED_AT, None)?;
        crate::app_settings::save_persisted_app_setting(SETTING_LAST_ERROR, None)?;
    }
    Ok(())
}

fn validate_catalog_url(raw: &str) -> Result<(), String> {
    let url =
        reqwest::Url::parse(raw).map_err(|err| format!("invalid model profile URL: {err}"))?;
    if url.scheme() != "https" {
        return Err("model profile URL must use HTTPS".to_string());
    }
    Ok(())
}

fn parse_catalog(raw: &str) -> Result<ModelProfileCatalog, String> {
    if raw.len() > MAX_CATALOG_BYTES {
        return Err("model profile catalog is too large".to_string());
    }
    let catalog = serde_json::from_str::<ModelProfileCatalog>(raw)
        .map_err(|err| format!("invalid model profile catalog: {err}"))?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "unsupported model profile schema: {}",
            catalog.schema_version
        ));
    }
    if catalog.revision <= 0 {
        return Err("model profile revision must be positive".to_string());
    }
    if catalog.updated_at.trim().is_empty() {
        return Err("model profile updatedAt is required".to_string());
    }
    for (slug, profile) in &catalog.models {
        let slug = slug.trim();
        if slug.is_empty() {
            return Err("model profile slug cannot be empty".to_string());
        }
        validate_importable_profile(slug, profile)?;
    }
    for rule in &catalog.family_rules {
        if rule.prefix.trim().is_empty() {
            return Err("model profile family prefix cannot be empty".to_string());
        }
    }
    Ok(catalog)
}

fn validate_importable_profile(slug: &str, profile: &ModelProfileTemplate) -> Result<(), String> {
    if profile.context_window.is_some_and(|value| value <= 0)
        || profile.max_context_window.is_some_and(|value| value <= 0)
    {
        return Err(format!(
            "model profile context window must be positive: {slug}"
        ));
    }
    let Some(price) = profile.price.as_ref() else {
        return Err(format!("model profile price is required: {slug}"));
    };
    if price.input_microusd_per_1m.is_none()
        || price.cached_input_microusd_per_1m.is_none()
        || price.output_microusd_per_1m.is_none()
    {
        return Err(format!("model profile base prices are incomplete: {slug}"));
    }
    if [
        price.input_microusd_per_1m,
        price.cached_input_microusd_per_1m,
        price.cache_write_microusd_per_1m,
        price.output_microusd_per_1m,
    ]
    .into_iter()
    .flatten()
    .any(|value| value < 0)
    {
        return Err(format!("model profile prices cannot be negative: {slug}"));
    }
    Ok(())
}

fn builtin_catalog() -> ModelProfileCatalog {
    parse_catalog(include_str!("../resources/model-profiles-v1.json"))
        .expect("built-in model profile catalog must be valid")
}

fn cached_catalog() -> Option<ModelProfileCatalog> {
    crate::app_settings::get_persisted_app_setting(SETTING_CACHE_JSON)
        .and_then(|raw| parse_catalog(&raw).ok())
}

fn effective_catalog() -> (ModelProfileCatalog, String) {
    let builtin = builtin_catalog();
    if let Some(cached) = cached_catalog() {
        if cached.revision >= builtin.revision {
            return (cached, "cache".to_string());
        }
    }
    (builtin, "builtin".to_string())
}

fn fetch_catalog(url: &str) -> Result<(ModelProfileCatalog, String), String> {
    validate_catalog_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("CodexManager/model-profiles")
        .build()
        .map_err(|err| format!("build model profile client failed: {err}"))?;
    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .map_err(|err| format!("fetch model profile catalog failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "fetch model profile catalog http_status={}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_CATALOG_BYTES)
    {
        return Err("model profile catalog is too large".to_string());
    }
    let bytes = response
        .bytes()
        .map_err(|err| format!("read model profile catalog failed: {err}"))?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err("model profile catalog is too large".to_string());
    }
    let raw = String::from_utf8(bytes.to_vec())
        .map_err(|_| "model profile catalog must be UTF-8".to_string())?;
    let catalog = parse_catalog(&raw)?;
    Ok((catalog, raw))
}

pub(crate) fn refresh(force: bool) -> Result<ModelProfileStatus, String> {
    let now = codexmanager_core::storage::now_ts();
    if !force {
        if let Some(last) = setting_i64(SETTING_LAST_CHECKED_AT) {
            let age = now.saturating_sub(last);
            if (0..REFRESH_INTERVAL_SECS).contains(&age) {
                return status();
            }
        }
    }
    crate::app_settings::save_persisted_app_setting(
        SETTING_LAST_CHECKED_AT,
        Some(&now.to_string()),
    )?;
    let url = source_url();
    match fetch_catalog(&url) {
        Ok((catalog, raw)) => {
            let minimum_revision = effective_catalog().0.revision;
            if catalog.revision < minimum_revision {
                let error = format!(
                    "model profile revision rollback rejected: {} < {}",
                    catalog.revision, minimum_revision
                );
                crate::app_settings::save_persisted_app_setting(SETTING_LAST_ERROR, Some(&error))?;
                return Err(error);
            }
            crate::app_settings::save_persisted_app_setting(SETTING_CACHE_JSON, Some(&raw))?;
            crate::app_settings::save_persisted_app_setting(
                SETTING_LAST_SUCCESS_AT,
                Some(&now.to_string()),
            )?;
            crate::app_settings::save_persisted_app_setting(SETTING_LAST_ERROR, None)?;
            status()
        }
        Err(err) => {
            crate::app_settings::save_persisted_app_setting(SETTING_LAST_ERROR, Some(&err))?;
            Err(err)
        }
    }
}

pub(crate) fn ensure_background_refresh() {
    if !auto_update_enabled() || REFRESH_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if let Err(err) = refresh(false) {
            log::warn!("event=model_profile_refresh_failed error={err}");
        }
        REFRESH_RUNNING.store(false, Ordering::SeqCst);
    });
}

fn merge_json(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            let mut merged = base.clone();
            for (key, value) in overlay {
                let next = merged
                    .get(key)
                    .map(|current| merge_json(current, value))
                    .unwrap_or_else(|| value.clone());
                merged.insert(key.clone(), next);
            }
            Value::Object(merged)
        }
        (_, overlay) => overlay.clone(),
    }
}

fn merge_template(
    base: &ModelProfileTemplate,
    overlay: &ModelProfileTemplate,
) -> ModelProfileTemplate {
    ModelProfileTemplate {
        display_name: overlay
            .display_name
            .clone()
            .or_else(|| base.display_name.clone()),
        description: overlay
            .description
            .clone()
            .or_else(|| base.description.clone()),
        provider: overlay.provider.clone().or_else(|| base.provider.clone()),
        family: overlay.family.clone().or_else(|| base.family.clone()),
        category: overlay.category.clone().or_else(|| base.category.clone()),
        tags: overlay.tags.clone().or_else(|| base.tags.clone()),
        sort_order: overlay.sort_order.or(base.sort_order),
        context_window: overlay.context_window.or(base.context_window),
        max_context_window: overlay.max_context_window.or(base.max_context_window),
        default_reasoning_effort: overlay
            .default_reasoning_effort
            .clone()
            .or_else(|| base.default_reasoning_effort.clone()),
        capabilities: match (&base.capabilities, &overlay.capabilities) {
            (Some(base), Some(overlay)) => Some(merge_json(base, overlay)),
            (None, Some(value)) | (Some(value), None) => Some(value.clone()),
            (None, None) => None,
        },
        price: match (&base.price, &overlay.price) {
            (Some(base), Some(overlay)) => Some(ModelProfilePrice {
                price_status: overlay
                    .price_status
                    .clone()
                    .or_else(|| base.price_status.clone()),
                price_source: overlay
                    .price_source
                    .clone()
                    .or_else(|| base.price_source.clone()),
                input_microusd_per_1m: overlay.input_microusd_per_1m.or(base.input_microusd_per_1m),
                cached_input_microusd_per_1m: overlay
                    .cached_input_microusd_per_1m
                    .or(base.cached_input_microusd_per_1m),
                cache_write_microusd_per_1m: overlay
                    .cache_write_microusd_per_1m
                    .or(base.cache_write_microusd_per_1m),
                output_microusd_per_1m: overlay
                    .output_microusd_per_1m
                    .or(base.output_microusd_per_1m),
            }),
            (None, Some(value)) | (Some(value), None) => Some(value.clone()),
            (None, None) => None,
        },
    }
}

fn aggregate_profile_override(
    api: &codexmanager_core::storage::AggregateApi,
    upstream_model: &str,
) -> Option<ModelProfileTemplate> {
    let value = api
        .compatibility_config_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())?;
    let profiles = value.get("modelProfiles")?.as_object()?;
    serde_json::from_value(profiles.get(upstream_model)?.clone()).ok()
}

fn profile_for_source(
    catalog: &ModelProfileCatalog,
    api: &codexmanager_core::storage::AggregateApi,
    upstream_model: &str,
) -> Option<ModelProfileTemplate> {
    let family = catalog
        .family_rules
        .iter()
        .find(|rule| upstream_model.starts_with(rule.prefix.trim()))
        .map(|rule| rule.defaults.clone());
    let exact = catalog.models.get(upstream_model).cloned();
    let base = match (family, exact) {
        (Some(family), Some(exact)) => Some(merge_template(&family, &exact)),
        (None, Some(exact)) => Some(exact),
        _ => None,
    };
    let overlay = aggregate_profile_override(api, upstream_model);
    match (base, overlay) {
        (Some(base), Some(overlay)) => Some(merge_template(&base, &overlay)),
        (Some(base), None) => Some(base),
        (None, Some(overlay)) => {
            let candidate = catalog
                .family_rules
                .iter()
                .find(|rule| upstream_model.starts_with(rule.prefix.trim()))
                .map(|rule| merge_template(&rule.defaults, &overlay))
                .unwrap_or(overlay);
            validate_importable_profile(upstream_model, &candidate)
                .is_ok()
                .then_some(candidate)
        }
        _ => None,
    }
}

fn profile_hash(
    revision: i64,
    source_id: &str,
    upstream_model: &str,
    profile: &ModelProfileTemplate,
) -> Result<String, String> {
    let payload = serde_json::to_vec(&(revision, source_id, upstream_model, profile))
        .map_err(|err| format!("serialize model profile hash failed: {err}"))?;
    let digest = Sha256::digest(payload);
    Ok(digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(""))
}

fn applied_state() -> HashMap<String, String> {
    crate::app_settings::get_persisted_app_setting(SETTING_APPLIED_STATE)
        .and_then(|raw| serde_json::from_str::<HashMap<String, String>>(&raw).ok())
        .unwrap_or_default()
}

fn save_applied_state(state: &HashMap<String, String>) -> Result<(), String> {
    let raw = serde_json::to_string(state)
        .map_err(|err| format!("serialize model profile state failed: {err}"))?;
    crate::app_settings::save_persisted_app_setting(SETTING_APPLIED_STATE, Some(&raw))
}

fn desired_model(
    existing: Option<&ManagedModelV2>,
    slug: &str,
    source_id: &str,
    profile: &ModelProfileTemplate,
) -> ManagedModelV2 {
    let now = codexmanager_core::storage::now_ts();
    let price = profile
        .price
        .as_ref()
        .map(ModelProfilePrice::to_model_price)
        .unwrap_or_default();
    let price_tiers = profile
        .price
        .as_ref()
        .and_then(ModelProfilePrice::to_base_price_tier)
        .into_iter()
        .collect();
    let mut routes = existing
        .map(|model| model.routes.clone())
        .unwrap_or_default();
    if !routes.iter().any(|route| {
        route.source_kind == "aggregate_api"
            && route.source_id == source_id
            && route.upstream_model == slug
    }) {
        routes.push(ModelRouteV2 {
            id: String::new(),
            source_kind: "aggregate_api".to_string(),
            source_id: source_id.to_string(),
            upstream_model: slug.to_string(),
            enabled: true,
            sort_order: routes
                .iter()
                .map(|route| route.sort_order)
                .max()
                .unwrap_or(0)
                + 10,
            priority: 0,
            weight: 1,
            compatibility_override_json: None,
        });
    }
    ManagedModelV2 {
        id: existing.map(|model| model.id.clone()).unwrap_or_default(),
        slug: slug.to_string(),
        display_name: profile
            .display_name
            .clone()
            .unwrap_or_else(|| slug.to_string()),
        description: profile.description.clone(),
        provider: profile.provider.clone(),
        family: profile.family.clone(),
        category: profile.category.clone(),
        tags: profile.tags.clone().unwrap_or_default(),
        origin: "custom".to_string(),
        enabled: existing.map(|model| model.enabled).unwrap_or(true),
        supported_in_api: existing.map(|model| model.supported_in_api).unwrap_or(true),
        visibility: existing
            .map(|model| model.visibility.clone())
            .unwrap_or_else(|| "list".to_string()),
        sort_order: profile
            .sort_order
            .unwrap_or_else(|| existing.map(|model| model.sort_order).unwrap_or(0)),
        context_window: profile.context_window,
        max_context_window: profile.max_context_window.or(profile.context_window),
        default_reasoning_effort: profile.default_reasoning_effort.clone(),
        capabilities: profile
            .capabilities
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        instructions_mode: existing
            .map(|model| model.instructions_mode.clone())
            .unwrap_or_else(|| "passthrough".to_string()),
        instructions_text: existing.and_then(|model| model.instructions_text.clone()),
        builtin_revision: None,
        user_edited: true,
        price,
        price_tiers,
        routes,
        permission_group_ids: existing
            .map(|model| model.permission_group_ids.clone())
            .unwrap_or_default(),
        created_at: existing.map(|model| model.created_at).unwrap_or(now),
        updated_at: now,
    }
}

fn push_change(changes: &mut Vec<ModelProfileChange>, field: &str, before: Value, after: Value) {
    if before != after {
        changes.push(ModelProfileChange {
            field: field.to_string(),
            before,
            after,
        });
    }
}

fn model_changes(
    existing: Option<&ManagedModelV2>,
    desired: &ManagedModelV2,
) -> Vec<ModelProfileChange> {
    let Some(existing) = existing else {
        return vec![ModelProfileChange {
            field: "model".to_string(),
            before: Value::Null,
            after: serde_json::json!(desired.display_name),
        }];
    };
    let mut changes = Vec::new();
    push_change(
        &mut changes,
        "displayName",
        serde_json::json!(existing.display_name),
        serde_json::json!(desired.display_name),
    );
    push_change(
        &mut changes,
        "description",
        serde_json::json!(existing.description),
        serde_json::json!(desired.description),
    );
    push_change(
        &mut changes,
        "provider",
        serde_json::json!(existing.provider),
        serde_json::json!(desired.provider),
    );
    push_change(
        &mut changes,
        "family",
        serde_json::json!(existing.family),
        serde_json::json!(desired.family),
    );
    push_change(
        &mut changes,
        "category",
        serde_json::json!(existing.category),
        serde_json::json!(desired.category),
    );
    push_change(
        &mut changes,
        "tags",
        serde_json::json!(existing.tags),
        serde_json::json!(desired.tags),
    );
    push_change(
        &mut changes,
        "contextWindow",
        serde_json::json!(existing.context_window),
        serde_json::json!(desired.context_window),
    );
    push_change(
        &mut changes,
        "maxContextWindow",
        serde_json::json!(existing.max_context_window),
        serde_json::json!(desired.max_context_window),
    );
    push_change(
        &mut changes,
        "defaultReasoningEffort",
        serde_json::json!(existing.default_reasoning_effort),
        serde_json::json!(desired.default_reasoning_effort),
    );
    push_change(
        &mut changes,
        "capabilities",
        existing.capabilities.clone(),
        desired.capabilities.clone(),
    );
    push_change(
        &mut changes,
        "price",
        serde_json::json!(existing.price),
        serde_json::json!(desired.price),
    );
    if existing.routes != desired.routes {
        push_change(
            &mut changes,
            "routes",
            serde_json::json!(existing.routes),
            serde_json::json!(desired.routes),
        );
    }
    changes
}

fn candidates_with_catalog(
    catalog: &ModelProfileCatalog,
) -> Result<Vec<ModelProfileCandidate>, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let apis = storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?;
    let existing = storage
        .list_managed_models_v2(true)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|model| (model.slug.to_ascii_lowercase(), model))
        .collect::<HashMap<_, _>>();
    let applied = applied_state();
    let mut items = Vec::new();
    for api in apis {
        if api.status != "active" {
            continue;
        }
        let discovered = storage
            .list_aggregate_api_supplier_models(Some(&api.id), None)
            .map_err(|err| err.to_string())?;
        for item in discovered
            .into_iter()
            .filter(|item| item.status == "available")
        {
            let slug = item.upstream_model.trim().to_string();
            if slug.is_empty() {
                continue;
            }
            let Some(profile) = profile_for_source(catalog, &api, &slug) else {
                continue;
            };
            validate_importable_profile(&slug, &profile)?;
            let existing_model = existing.get(&slug.to_ascii_lowercase());
            let desired = desired_model(existing_model, &slug, &api.id, &profile);
            let hash = profile_hash(catalog.revision, &api.id, &slug, &profile)?;
            let changes = model_changes(existing_model, &desired);
            if existing_model.is_some()
                && applied
                    .get(&slug)
                    .is_some_and(|applied_hash| applied_hash == &hash)
            {
                continue;
            }
            if existing_model.is_some() && changes.is_empty() {
                continue;
            }
            items.push(ModelProfileCandidate {
                kind: if existing_model.is_some() {
                    "update"
                } else {
                    "import"
                }
                .to_string(),
                slug: slug.clone(),
                display_name: desired.display_name,
                source_id: api.id.clone(),
                source_name: api.supplier_name.clone().unwrap_or_else(|| api.id.clone()),
                upstream_model: slug,
                profile_revision: catalog.revision,
                profile_hash: hash,
                changes,
            });
        }
    }
    items.sort_by(|left, right| left.slug.cmp(&right.slug));
    Ok(items)
}

pub(crate) fn candidates() -> Result<ModelProfileCandidateList, String> {
    let (catalog, _) = effective_catalog();
    Ok(ModelProfileCandidateList {
        items: candidates_with_catalog(&catalog)?,
    })
}

pub(crate) fn status() -> Result<ModelProfileStatus, String> {
    let (catalog, source) = effective_catalog();
    let items = candidates_with_catalog(&catalog)?;
    Ok(ModelProfileStatus {
        auto_update_enabled: auto_update_enabled(),
        source_url: source_url(),
        schema_version: catalog.schema_version,
        revision: catalog.revision,
        catalog_updated_at: catalog.updated_at,
        last_checked_at: setting_i64(SETTING_LAST_CHECKED_AT),
        last_success_at: setting_i64(SETTING_LAST_SUCCESS_AT),
        last_error: crate::app_settings::get_persisted_app_setting(SETTING_LAST_ERROR),
        source,
        importable_count: items.iter().filter(|item| item.kind == "import").count(),
        update_count: items.iter().filter(|item| item.kind == "update").count(),
    })
}

pub(crate) fn apply(params: ApplyModelProfileParams) -> Result<ApplyModelProfileResult, String> {
    let source_id = params.source_id.trim();
    let upstream_model = params.upstream_model.trim();
    if source_id.is_empty() || upstream_model.is_empty() {
        return Err("model profile sourceId and upstreamModel are required".to_string());
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if !storage
        .list_aggregate_api_supplier_models(Some(source_id), None)
        .map_err(|err| err.to_string())?
        .into_iter()
        .any(|item| item.status == "available" && item.upstream_model == upstream_model)
    {
        return Err(
            "model is not available in the selected aggregate API discovery cache".to_string(),
        );
    }
    let api = storage
        .find_aggregate_api_by_id(source_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let (catalog, _) = effective_catalog();
    let profile = profile_for_source(&catalog, &api, upstream_model)
        .ok_or_else(|| "exact model profile not found".to_string())?;
    validate_importable_profile(upstream_model, &profile)?;
    let existing = storage
        .get_managed_model_v2(upstream_model)
        .map_err(|err| err.to_string())?;
    let desired = desired_model(existing.as_ref(), upstream_model, source_id, &profile);
    let saved = storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: existing.as_ref().map(|model| model.slug.clone()),
            model: desired,
        })
        .map_err(|err| format!("apply model profile failed: {err}"))?;
    let hash = profile_hash(catalog.revision, source_id, upstream_model, &profile)?;
    let mut state = applied_state();
    state.insert(saved.slug.clone(), hash.clone());
    save_applied_state(&state)?;
    crate::models_v2::sync_active_gateway_catalog_best_effort(&storage);
    Ok(ApplyModelProfileResult {
        model: saved,
        applied_revision: catalog.revision,
        applied_hash: hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_contains_importable_deepseek_flash() {
        let catalog = builtin_catalog();
        let profile = catalog.models.get("deepseek-v4-flash").unwrap();
        validate_importable_profile("deepseek-v4-flash", profile).unwrap();
        assert_eq!(profile.context_window, Some(1_000_000));
        assert_eq!(
            profile.price.as_ref().unwrap().input_microusd_per_1m,
            Some(140_000)
        );
    }

    #[test]
    fn applying_priced_profile_builds_matching_zero_threshold_tier() {
        let catalog = builtin_catalog();
        let profile = catalog.models.get("deepseek-v4-flash").unwrap();
        let existing = ManagedModelV2 {
            slug: "deepseek-v4-flash".to_string(),
            display_name: "deepseek-v4-flash".to_string(),
            origin: "custom".to_string(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".to_string(),
            instructions_mode: "passthrough".to_string(),
            ..Default::default()
        };

        let desired = desired_model(Some(&existing), "deepseek-v4-flash", "deepseek", profile);

        assert_eq!(desired.price_tiers.len(), 1);
        let tier = &desired.price_tiers[0];
        assert_eq!(tier.min_input_tokens, 0);
        assert_eq!(
            Some(tier.input_microusd_per_1m),
            desired.price.input_microusd_per_1m
        );
        assert_eq!(
            Some(tier.cached_input_microusd_per_1m),
            desired.price.cached_input_microusd_per_1m
        );
        assert_eq!(
            tier.cache_write_microusd_per_1m,
            desired.price.cache_write_microusd_per_1m
        );
        assert_eq!(
            Some(tier.output_microusd_per_1m),
            desired.price.output_microusd_per_1m
        );
    }

    #[test]
    fn template_merge_recursively_merges_capabilities() {
        let base = ModelProfileTemplate {
            capabilities: Some(serde_json::json!({"inputModalities":["text"],"a":1})),
            ..Default::default()
        };
        let overlay = ModelProfileTemplate {
            capabilities: Some(serde_json::json!({"a":2,"b":3})),
            ..Default::default()
        };
        assert_eq!(
            merge_template(&base, &overlay).capabilities.unwrap(),
            serde_json::json!({"inputModalities":["text"],"a":2,"b":3})
        );
    }

    #[test]
    fn partial_price_override_keeps_base_prices() {
        let base = ModelProfileTemplate {
            price: Some(ModelProfilePrice {
                price_status: Some("official".to_string()),
                input_microusd_per_1m: Some(140_000),
                cached_input_microusd_per_1m: Some(2_800),
                output_microusd_per_1m: Some(280_000),
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = ModelProfileTemplate {
            price: Some(ModelProfilePrice {
                output_microusd_per_1m: Some(300_000),
                ..Default::default()
            }),
            ..Default::default()
        };
        let price = merge_template(&base, &overlay).price.unwrap();
        assert_eq!(price.input_microusd_per_1m, Some(140_000));
        assert_eq!(price.output_microusd_per_1m, Some(300_000));
    }

    #[test]
    fn catalog_rejects_gateway_fields() {
        let raw = r#"{
          "schemaVersion":1,
          "revision":1,
          "updatedAt":"2026-08-04T00:00:00Z",
          "models":{},
          "familyRules":[],
          "staticHeaders":{"x-test":"value"}
        }"#;
        assert!(parse_catalog(raw).is_err());
    }
}
