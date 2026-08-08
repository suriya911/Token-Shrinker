//! Shared service composition for stdio, one-shot, and future daemon modes.

use std::path::Path;
use token_shrinker_context::{ConservativeEstimator, ContextBundle, build_context};
use token_shrinker_repo::{
    RepositoryError, RepositoryProvider, RepositoryQuery, RepositoryTrace, ScanWarning,
};
use token_shrinker_types::TokenBudget;

/// Observable result of the dependency-free native context baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineContext {
    /// Deterministically ranked and budgeted context sent to the next pipeline stage.
    pub bundle: ContextBundle,
    /// Non-fatal repository omissions, without source contents.
    pub warnings: Vec<ScanWarning>,
    /// Content-free repository counters.
    pub repository_trace: RepositoryTrace,
}

/// Builds context using only the native repository provider and conservative token estimator.
///
/// This path intentionally does not invoke optional symbol indexes, external search tools, or an
/// LLM. It is the deterministic fallback that later provider layers may enrich.
///
/// # Errors
///
/// Returns [`RepositoryError`] when the allowed root cannot be opened or scanned.
pub fn build_baseline_context(
    root: impl AsRef<Path>,
    query: &str,
    budget: TokenBudget,
) -> Result<BaselineContext, RepositoryError> {
    let terms = query_terms(query);
    let provider = RepositoryProvider::open(root)?;
    let scan = provider.scan(&RepositoryQuery {
        path_hints: terms.clone(),
        terms,
    })?;
    let bundle = build_context(&scan.candidates, budget, &ConservativeEstimator);
    Ok(BaselineContext {
        bundle,
        warnings: scan.warnings,
        repository_trace: scan.trace,
    })
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_shrinker_context::Sensitivity;

    fn fixture_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("fixtures/demo-repo")
    }

    #[test]
    fn baseline_builds_ranked_context_without_optional_tools() {
        let budget = TokenBudget::from_u32(300).expect("positive test budget");
        let first =
            build_baseline_context(fixture_root(), "fix session authorization policy", budget)
                .expect("build native baseline");
        let second =
            build_baseline_context(fixture_root(), "fix session authorization policy", budget)
                .expect("rebuild native baseline");

        assert!(!first.bundle.items.is_empty());
        assert!(first.bundle.trace.used_tokens <= u64::from(budget.get()));
        assert_eq!(first.bundle, second.bundle);
        assert!(
            first.bundle.items.iter().any(|item| item
                .location
                .uri
                .to_ascii_lowercase()
                .contains("session")),
            "mandatory session evidence must fit the baseline budget"
        );
        assert!(first.bundle.items.iter().all(|item| {
            item.sensitivity != Sensitivity::Redacted || !item.content.contains("canary-secret")
        }));
        assert!(!first.repository_trace.cancelled);
    }

    #[test]
    fn query_terms_are_normalized_deduplicated_and_stable() {
        assert_eq!(
            query_terms("Session, AUTH session x"),
            vec!["auth", "session"]
        );
    }
}
