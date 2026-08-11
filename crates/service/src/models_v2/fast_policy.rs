use codexmanager_core::storage::ModelFastPolicyV2;
use serde_json::Value;

pub(crate) const FAST_REQUEST_BLOCKED: &str = "fast_request_blocked";

pub(crate) fn apply(
    body: Vec<u8>,
    policy: ModelFastPolicyV2,
    client_has_service_tier: bool,
) -> Result<(Vec<u8>, bool), &'static str> {
    if policy == ModelFastPolicyV2::Block && client_has_service_tier {
        return Err(FAST_REQUEST_BLOCKED);
    }
    if matches!(
        policy,
        ModelFastPolicyV2::Passthrough | ModelFastPolicyV2::Block
    ) {
        return Ok((body, false));
    }

    let Ok(mut payload) = serde_json::from_slice::<Value>(&body) else {
        return Ok((body, false));
    };
    let Some(object) = payload.as_object_mut() else {
        return Ok((body, false));
    };
    let changed = match policy {
        ModelFastPolicyV2::Filter => object.remove("service_tier").is_some(),
        ModelFastPolicyV2::Force => {
            object.insert(
                "service_tier".to_string(),
                Value::String("priority".to_string()),
            );
            true
        }
        ModelFastPolicyV2::Passthrough | ModelFastPolicyV2::Block => false,
    };
    if !changed {
        return Ok((body, false));
    }
    Ok((serde_json::to_vec(&payload).unwrap_or(body), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_tier(body: &[u8]) -> Option<String> {
        serde_json::from_slice::<Value>(body)
            .ok()?
            .get("service_tier")?
            .as_str()
            .map(str::to_string)
    }

    #[test]
    fn passthrough_preserves_service_tier() {
        let body = br#"{"service_tier":"fast"}"#.to_vec();
        let (body, applied) = apply(body, ModelFastPolicyV2::Passthrough, true).unwrap();
        assert!(!applied);
        assert_eq!(service_tier(&body).as_deref(), Some("fast"));
    }

    #[test]
    fn filter_removes_service_tier() {
        let body = br#"{"service_tier":"fast","input":[]}"#.to_vec();
        let (body, applied) = apply(body, ModelFastPolicyV2::Filter, true).unwrap();
        assert!(applied);
        assert_eq!(service_tier(&body), None);
    }

    #[test]
    fn force_sets_priority() {
        let body = br#"{"input":[]}"#.to_vec();
        let (body, applied) = apply(body, ModelFastPolicyV2::Force, false).unwrap();
        assert!(applied);
        assert_eq!(service_tier(&body).as_deref(), Some("priority"));
    }

    #[test]
    fn block_only_rejects_explicit_client_service_tier() {
        let body = br#"{"service_tier":"priority"}"#.to_vec();
        assert_eq!(
            apply(body.clone(), ModelFastPolicyV2::Block, true),
            Err(FAST_REQUEST_BLOCKED)
        );
        let (body, applied) = apply(body, ModelFastPolicyV2::Block, false).unwrap();
        assert!(!applied);
        assert_eq!(service_tier(&body).as_deref(), Some("priority"));
    }
}
