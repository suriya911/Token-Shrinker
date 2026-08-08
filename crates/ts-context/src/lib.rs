//! Context candidates, ranking, budgets, and provenance.

use token_shrinker_types::{CountPrecision, CounterId, TokenBudget, TokenCount};

/// Stable identity of the baseline UTF-8 byte upper-bound estimator.
pub const CONSERVATIVE_ESTIMATOR_ID: &str = "byte_upper_bound_v1";

/// Counts text for context budgeting while disclosing precision and identity.
pub trait TokenCounter {
    /// Stable tokenizer or estimator identity.
    fn id(&self) -> CounterId;

    /// Whether this counter is exact for a known tokenizer.
    fn precision(&self) -> CountPrecision;

    /// Counts one text segment deterministically.
    fn count(&self, text: &str) -> TokenCount;
}

/// Offline fallback that assigns at most one conservative unit per UTF-8 byte.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConservativeEstimator;

impl TokenCounter for ConservativeEstimator {
    fn id(&self) -> CounterId {
        CounterId::new(CONSERVATIVE_ESTIMATOR_ID).expect("built-in counter ID is valid")
    }

    fn precision(&self) -> CountPrecision {
        CountPrecision::Estimated
    }

    fn count(&self, text: &str) -> TokenCount {
        TokenCount::new(text.len() as u64, self.precision(), self.id())
    }
}

/// Result of deterministic in-order candidate selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetSelection {
    /// Indices of candidates that fit, in input order.
    pub selected: Vec<usize>,
    /// Indices omitted because they would exceed the budget.
    pub omitted: Vec<usize>,
    /// Labeled total for selected candidate content.
    pub used: TokenCount,
}

/// Selects candidates in order without allowing their counted content to exceed the budget.
#[must_use]
pub fn select_within_budget<C: TokenCounter>(
    candidates: &[&str],
    budget: TokenBudget,
    counter: &C,
) -> BudgetSelection {
    let limit = u64::from(budget.get());
    let mut used = 0_u64;
    let mut selected = Vec::new();
    let mut omitted = Vec::new();

    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_tokens = counter.count(candidate).tokens();
        if used.saturating_add(candidate_tokens) <= limit {
            used += candidate_tokens;
            selected.push(index);
        } else {
            omitted.push(index);
        }
    }

    BudgetSelection {
        selected,
        omitted,
        used: TokenCount::new(used, counter.precision(), counter.id()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_is_deterministic_and_labeled() {
        let estimator = ConservativeEstimator;
        let first = estimator.count("café");
        let second = estimator.count("café");

        assert_eq!(first, second);
        assert_eq!(first.tokens(), "café".len() as u64);
        assert_eq!(first.precision(), CountPrecision::Estimated);
        assert_eq!(first.counter_id().as_str(), CONSERVATIVE_ESTIMATOR_ID);
    }

    #[test]
    fn estimate_is_nonnegative_and_monotonic_for_fixture_corpus() {
        let estimator = ConservativeEstimator;
        for text in ["", "a", "hello world", "你好", "fn main() {}", "not false"] {
            let base = estimator.count(text).tokens();
            let extended = estimator.count(&format!("{text}x")).tokens();
            assert!(extended >= base);
        }
    }

    #[test]
    fn selection_never_exceeds_budget_and_can_skip_large_items() {
        let estimator = ConservativeEstimator;
        let budget = TokenBudget::from_u32(8).expect("nonzero test budget");
        let selection = select_within_budget(&["abc", "oversized", "de"], budget, &estimator);

        assert_eq!(selection.selected, vec![0, 2]);
        assert_eq!(selection.omitted, vec![1]);
        assert_eq!(selection.used.tokens(), 5);
        assert!(selection.used.tokens() <= u64::from(budget.get()));
    }
}
