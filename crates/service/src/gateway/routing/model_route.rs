use codexmanager_core::storage::ModelRouteV2;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModelRouteScheduleKey {
    key_id: String,
    model: String,
    source_kind: String,
    priority: i64,
}

type RouteWeights = HashMap<String, i128>;
type RouteScheduleState = HashMap<ModelRouteScheduleKey, RouteWeights>;

static MODEL_ROUTE_SCHEDULE_STATE: OnceLock<Mutex<RouteScheduleState>> = OnceLock::new();

fn schedule_state() -> &'static Mutex<RouteScheduleState> {
    MODEL_ROUTE_SCHEDULE_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn route_identity(route: &ModelRouteV2) -> String {
    let id = route.id.trim();
    if !id.is_empty() {
        return id.to_string();
    }
    format!(
        "{}\0{}\0{}",
        route.source_kind, route.source_id, route.upstream_model
    )
}

fn stable_route_cmp(left: &ModelRouteV2, right: &ModelRouteV2) -> std::cmp::Ordering {
    left.source_kind
        .cmp(&right.source_kind)
        .then(left.source_id.cmp(&right.source_id))
        .then(left.upstream_model.cmp(&right.upstream_model))
        .then(left.id.cmp(&right.id))
}

fn schedule_priority_group(
    mut routes: Vec<ModelRouteV2>,
    key: ModelRouteScheduleKey,
) -> Vec<ModelRouteV2> {
    routes.sort_by(stable_route_cmp);
    if routes.len() <= 1 {
        return routes;
    }

    let route_ids = routes.iter().map(route_identity).collect::<Vec<_>>();
    let mut state = schedule_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let weights = state.entry(key).or_default();
    weights.retain(|route_id, _| route_ids.contains(route_id));

    let total_weight = routes
        .iter()
        .map(|route| i128::from(route.weight.max(1)))
        .sum::<i128>();
    for route in &routes {
        *weights.entry(route_identity(route)).or_default() += i128::from(route.weight.max(1));
    }

    let selected_index = routes
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            let left_weight = weights
                .get(&route_identity(left))
                .copied()
                .unwrap_or_default();
            let right_weight = weights
                .get(&route_identity(right))
                .copied()
                .unwrap_or_default();
            left_weight
                .cmp(&right_weight)
                .then_with(|| stable_route_cmp(right, left))
        })
        .map(|(index, _)| index)
        .unwrap_or_default();
    let selected = routes.remove(selected_index);
    *weights.entry(route_identity(&selected)).or_default() -= total_weight;

    routes.sort_by(|left, right| {
        let left_weight = weights
            .get(&route_identity(left))
            .copied()
            .unwrap_or_default();
        let right_weight = weights
            .get(&route_identity(right))
            .copied()
            .unwrap_or_default();
        right_weight
            .cmp(&left_weight)
            .then_with(|| right.weight.cmp(&left.weight))
            .then_with(|| stable_route_cmp(left, right))
    });

    std::iter::once(selected).chain(routes).collect()
}

/// Orders enabled routes by hard priority tiers and smooth weighted rotation inside each tier.
/// The process-wide account `ordered`/`balanced` setting intentionally does not participate here.
pub(crate) fn schedule_model_routes(
    routes: &[ModelRouteV2],
    key_id: &str,
    model: Option<&str>,
    source_kind: &str,
) -> Vec<ModelRouteV2> {
    let mut priority_groups = BTreeMap::<Reverse<i64>, Vec<ModelRouteV2>>::new();
    for route in routes
        .iter()
        .filter(|route| route.enabled && route.source_kind == source_kind)
    {
        priority_groups
            .entry(Reverse(route.priority))
            .or_default()
            .push(route.clone());
    }

    let model = model.unwrap_or_default().trim().to_string();
    priority_groups
        .into_iter()
        .flat_map(|(Reverse(priority), routes)| {
            schedule_priority_group(
                routes,
                ModelRouteScheduleKey {
                    key_id: key_id.to_string(),
                    model: model.clone(),
                    source_kind: source_kind.to_string(),
                    priority,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::schedule_model_routes;
    use codexmanager_core::storage::ModelRouteV2;

    fn route(id: &str, priority: i64, weight: i64) -> ModelRouteV2 {
        ModelRouteV2 {
            id: id.to_string(),
            source_kind: "account_pool".to_string(),
            source_id: "default".to_string(),
            upstream_model: format!("upstream-{id}"),
            enabled: true,
            priority,
            weight,
            ..Default::default()
        }
    }

    #[test]
    fn higher_priority_routes_always_precede_lower_priority_routes() {
        let routes = vec![route("low", 1, 100), route("high", 10, 1)];
        let ordered = schedule_model_routes(
            &routes,
            "priority-key",
            Some("priority-model"),
            "account_pool",
        );

        assert_eq!(ordered[0].id, "high");
        assert_eq!(ordered[1].id, "low");
    }

    #[test]
    fn smooth_weighted_rotation_controls_the_first_route_without_duplication() {
        let routes = vec![route("weight-a", 10, 3), route("weight-b", 10, 1)];
        let mut a_first = 0;
        let mut b_first = 0;
        for _ in 0..40 {
            let ordered =
                schedule_model_routes(&routes, "weight-key", Some("weight-model"), "account_pool");
            assert_eq!(ordered.len(), 2);
            assert_ne!(ordered[0].id, ordered[1].id);
            match ordered[0].id.as_str() {
                "weight-a" => a_first += 1,
                "weight-b" => b_first += 1,
                other => panic!("unexpected route: {other}"),
            }
        }

        assert_eq!((a_first, b_first), (30, 10));
    }

    #[test]
    fn source_types_have_independent_schedules() {
        let mut aggregate_a = route("aggregate-a", 0, 1);
        aggregate_a.source_kind = "aggregate_api".to_string();
        aggregate_a.source_id = "agg-a".to_string();
        let mut aggregate_b = route("aggregate-b", 0, 1);
        aggregate_b.source_kind = "aggregate_api".to_string();
        aggregate_b.source_id = "agg-b".to_string();
        let routes = vec![route("account", 100, 1), aggregate_a, aggregate_b];

        let aggregate =
            schedule_model_routes(&routes, "source-key", Some("source-model"), "aggregate_api");

        assert_eq!(aggregate.len(), 2);
        assert!(aggregate
            .iter()
            .all(|route| route.source_kind == "aggregate_api"));
    }
}
