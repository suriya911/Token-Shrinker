//! Deterministic FAST, BUILD, and DEEP routing.

use serde::{Deserialize, Serialize};
use token_shrinker_types::{
    RouteDecision, RouteMode, RouteOperation, RouteReason, RouteRequest, RouteScope, TokenBudget,
};

/// Configurable defaults used by route selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouterConfig {
    /// Fixed mode used when a request has no override; `None` enables automatic rules.
    #[serde(default)]
    pub default_mode: Option<RouteMode>,
    /// Default budget for focused work.
    pub fast_budget: TokenBudget,
    /// Default budget for normal implementation work.
    pub build_budget: TokenBudget,
    /// Default budget for broad investigations.
    pub deep_budget: TokenBudget,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            default_mode: None,
            fast_budget: budget(4_000),
            build_budget: budget(16_000),
            deep_budget: budget(48_000),
        }
    }
}

impl RouterConfig {
    fn budget_for(self, mode: RouteMode) -> TokenBudget {
        match mode {
            RouteMode::Fast => self.fast_budget,
            RouteMode::Build => self.build_budget,
            RouteMode::Deep => self.deep_budget,
        }
    }
}

/// Selects an explainable route using ordered, offline rules.
#[must_use]
pub fn route(request: &RouteRequest, config: RouterConfig) -> RouteDecision {
    let (mode, reason) = if let Some(mode) = request.explicit_mode {
        (mode, RouteReason::ExplicitOverride)
    } else if let Some(mode) = config.default_mode {
        (mode, RouteReason::ConfiguredMode)
    } else if request.scope == Some(RouteScope::Repository)
        || request.operations.iter().any(|operation| {
            matches!(
                operation,
                RouteOperation::Architecture | RouteOperation::Investigation
            )
        })
    {
        (RouteMode::Deep, RouteReason::BroadScope)
    } else if request.scope == Some(RouteScope::MultiFile)
        || request
            .operations
            .iter()
            .any(|operation| matches!(operation, RouteOperation::Edit | RouteOperation::Debug))
    {
        (RouteMode::Build, RouteReason::BuildOperation)
    } else if request.scope == Some(RouteScope::Named)
        || (!request.operations.is_empty()
            && request.operations.iter().all(|operation| {
                matches!(operation, RouteOperation::Lookup | RouteOperation::Command)
            }))
    {
        (RouteMode::Fast, RouteReason::FocusedLookup)
    } else {
        (RouteMode::Build, RouteReason::AmbiguousDefault)
    };

    let budget = request
        .budget_override
        .unwrap_or_else(|| config.budget_for(mode));
    let mut decision = RouteDecision::new(mode, budget, reason);
    if request.budget_override.is_some() {
        decision.add_reason(RouteReason::BudgetOverride);
    }
    decision
}

fn budget(tokens: u32) -> TokenBudget {
    TokenBudget::from_u32(tokens).expect("router defaults are nonzero")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct FixtureCase {
        name: String,
        #[serde(default)]
        config: Option<RouterConfig>,
        request: RouteRequest,
        expected: RouteDecision,
    }

    #[test]
    fn public_v1_fixtures_match_router_contract() {
        let cases: Vec<FixtureCase> =
            serde_json::from_str(include_str!("../../../fixtures/routing/v1.json"))
                .expect("valid routing fixture");

        for case in cases {
            assert_eq!(
                route(&case.request, case.config.unwrap_or_default()),
                case.expected,
                "fixture: {}",
                case.name
            );
        }
    }

    #[test]
    fn ordered_rules_cover_primary_route_cases() {
        let cases = [
            (
                RouteRequest {
                    explicit_mode: Some(RouteMode::Fast),
                    scope: Some(RouteScope::Repository),
                    ..RouteRequest::default()
                },
                RouteMode::Fast,
                RouteReason::ExplicitOverride,
            ),
            (
                RouteRequest {
                    operations: vec![RouteOperation::Architecture],
                    ..RouteRequest::default()
                },
                RouteMode::Deep,
                RouteReason::BroadScope,
            ),
            (
                RouteRequest {
                    operations: vec![RouteOperation::Debug],
                    ..RouteRequest::default()
                },
                RouteMode::Build,
                RouteReason::BuildOperation,
            ),
            (
                RouteRequest {
                    operations: vec![RouteOperation::Lookup],
                    scope: Some(RouteScope::Named),
                    ..RouteRequest::default()
                },
                RouteMode::Fast,
                RouteReason::FocusedLookup,
            ),
            (
                RouteRequest::default(),
                RouteMode::Build,
                RouteReason::AmbiguousDefault,
            ),
        ];

        for (request, expected_mode, expected_reason) in cases {
            let decision = route(&request, RouterConfig::default());
            assert_eq!(decision.mode, expected_mode);
            assert_eq!(decision.reasons[0], expected_reason);
        }
    }

    #[test]
    fn request_budget_overrides_mode_default() {
        let override_budget = TokenBudget::from_u32(2_048).expect("nonzero test value");
        let request = RouteRequest {
            explicit_mode: Some(RouteMode::Deep),
            budget_override: Some(override_budget),
            ..RouteRequest::default()
        };

        let decision = route(&request, RouterConfig::default());

        assert_eq!(decision.budget, override_budget);
        assert_eq!(
            decision.reasons,
            vec![RouteReason::ExplicitOverride, RouteReason::BudgetOverride]
        );
    }

    #[test]
    fn route_is_deterministic_for_identical_input() {
        let request = RouteRequest {
            operations: vec![RouteOperation::Investigation, RouteOperation::Lookup],
            scope: Some(RouteScope::MultiFile),
            ..RouteRequest::default()
        };

        let first = route(&request, RouterConfig::default());
        for _ in 0..100 {
            assert_eq!(route(&request, RouterConfig::default()), first);
        }
    }
}
