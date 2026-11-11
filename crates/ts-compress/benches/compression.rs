//! `BUILD_PLAN` section 15 suite 3: noisy tool outputs with known required lines.
//!
//! Throughput alone is not the property that matters here. A compressor that drops the one line
//! naming the failure is fast and useless, so every case asserts its required lines survive before
//! it is timed. A regression that loses evidence fails the benchmark rather than quietly speeding
//! it up.

use criterion::{Criterion, criterion_group, criterion_main};
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Duration;
use token_shrinker_compress::{TerminalInput, compress_context, compress_terminal};
use token_shrinker_context::{
    ContentHash, ContextCandidate, RelevanceSignals, Sensitivity, SourceId, SourceKind,
    SourceLocation,
};
use token_shrinker_types::TokenBudget;

/// The lines a caller must still be able to read after compression.
const REQUIRED_TERMINAL_LINES: [&str; 3] = [
    "error[E0308]: mismatched types",
    "src/auth/session.rs:42",
    "assertion failed: expires_at > now",
];

/// Builds a realistic noisy build log: repeated progress chatter around a few decisive lines.
fn noisy_terminal_output(repetitions: usize) -> String {
    let mut output = String::new();
    for index in 0..repetitions {
        output.push_str("   Compiling token-shrinker-core v0.2.0\n");
        output.push_str("    Checking dependency graph\n");
        writeln!(output, "warning: unused variable `tmp_{index}`").expect("benchmark write");
        if index == repetitions / 2 {
            for line in REQUIRED_TERMINAL_LINES {
                output.push_str(line);
                output.push('\n');
            }
        }
    }
    output
}

fn terminal_input(stdout: String) -> TerminalInput {
    TerminalInput {
        command: vec!["cargo".to_owned(), "test".to_owned()],
        exit_code: Some(101),
        duration: Duration::from_secs(3),
        stdout,
        stderr: String::new(),
        truncated: false,
        raw_handle: None,
    }
}

/// Fails loudly if compression lost a line the caller needs, before any timing happens.
fn assert_terminal_evidence_retained(stdout: &str) {
    let input = terminal_input(stdout.to_owned());
    let summary = compress_terminal(&input, 20, 20);
    let retained = summary
        .evidence
        .iter()
        .map(|item| item.line.as_str())
        .chain(summary.first_lines.iter().map(String::as_str))
        .chain(summary.tail_lines.iter().map(String::as_str))
        .collect::<Vec<_>>();
    for required in REQUIRED_TERMINAL_LINES {
        assert!(
            retained.iter().any(|line| line.contains(required)),
            "terminal compression dropped required evidence: {required}"
        );
    }
    assert_eq!(summary.exit_code, Some(101), "exit status must survive");
}

fn terminal_benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("compression_terminal");
    for repetitions in [64_usize, 512, 4_096] {
        let stdout = noisy_terminal_output(repetitions);
        assert_terminal_evidence_retained(&stdout);
        let input = terminal_input(stdout);
        group.throughput(criterion::Throughput::Bytes(input.stdout.len() as u64));
        group.bench_function(format!("noisy_lines_{}", repetitions * 3), |bencher| {
            bencher.iter(|| compress_terminal(black_box(&input), black_box(20), black_box(20)));
        });
    }
    group.finish();
}

/// The source lines a context query must still cite after extractive compression.
const REQUIRED_CONTEXT_LINES: [&str; 2] = [
    "if session.expires_at >= now {",
    "pub fn authorize(session: &Session)",
];

fn context_candidate(filler_lines: usize) -> ContextCandidate {
    let mut content = String::new();
    content.push_str("pub fn authorize(session: &Session) -> bool {\n");
    for index in 0..filler_lines {
        writeln!(content, "    let unrelated_{index} = compute({index});").expect("bench write");
    }
    content.push_str("    if session.expires_at >= now {\n");
    content.push_str("        return true;\n    }\n    false\n}\n");
    ContextCandidate {
        source_id: SourceId::new("repo:src/auth/session.rs").expect("benchmark source ID"),
        source_kind: SourceKind::RepositoryFile,
        location: SourceLocation {
            uri: "src/auth/session.rs".to_owned(),
            start_line: None,
            end_line: None,
        },
        content,
        content_hash: ContentHash::new(format!("{:064x}", 1)).expect("benchmark content hash"),
        sensitivity: Sensitivity::Public,
        modified_unix_ms: None,
        relevance: RelevanceSignals {
            exact_match: true,
            term_match_count: 2,
            path_match: true,
            diagnostic: false,
            freshness: 200,
        },
    }
}

fn context_benchmark(criterion: &mut Criterion) {
    let terms = vec!["expires_at".to_owned(), "authorize".to_owned()];
    let budget = TokenBudget::from_u32(2_048).expect("benchmark budget");
    let mut group = criterion.benchmark_group("compression_context");
    for filler_lines in [50_usize, 500, 5_000] {
        let candidate = context_candidate(filler_lines);
        let compressed = compress_context(&candidate, &terms, 2, budget);
        let retained = compressed
            .passages
            .iter()
            .map(|passage| passage.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for required in REQUIRED_CONTEXT_LINES {
            assert!(
                retained.contains(required),
                "context compression dropped required evidence at {filler_lines} filler lines: \
                 {required}"
            );
        }
        group.throughput(criterion::Throughput::Bytes(candidate.content.len() as u64));
        group.bench_function(format!("extract_from_{filler_lines}_filler_lines"), |b| {
            b.iter(|| {
                compress_context(
                    black_box(&candidate),
                    black_box(&terms),
                    black_box(2),
                    black_box(budget),
                )
            });
        });
    }
    group.finish();
}

criterion_group!(benches, terminal_benchmark, context_benchmark);
criterion_main!(benches);
