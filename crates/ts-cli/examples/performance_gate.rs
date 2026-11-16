//! Cross-platform Section 15 v1 performance-budget gate.

use serde_json::json;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;
use std::{
    env, fs,
    hint::black_box,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use token_shrinker_daemon::{
    BuiltInHandler, CapabilityRegistry, DaemonConfig, DaemonError, DaemonHandle,
    build_baseline_context,
};
use token_shrinker_protocol::ProtocolError;
use token_shrinker_router::{RouterConfig, route};
use token_shrinker_types::{RouteOperation, RouteRequest, RouteScope, TokenBudget};

const SAMPLES: usize = 25;

fn percentile(samples: &mut [u128], percentile: usize) -> u128 {
    samples.sort_unstable();
    let index = ((samples.len() - 1) * percentile).div_ceil(100);
    samples[index]
}

fn memory_bytes() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-Process -Id $PID).WorkingSet64",
            ])
            .output()
            .ok()?;
        return String::from_utf8_lossy(&output.stdout).trim().parse().ok();
    }
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?;
        return value
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024);
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        return String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .ok()
            .map(|kb| kb * 1024);
    }
    #[allow(unreachable_code)]
    None
}

fn daemon_config(directory: &TempDir) -> DaemonConfig {
    DaemonConfig {
        runtime_directory: directory.path().to_owned(),
        max_frame_bytes: 64 * 1024,
        max_concurrency: 8,
        graceful_shutdown_timeout: Duration::from_secs(5),
        max_log_bytes: 64 * 1024,
    }
}

fn daemon_health(daemon: &DaemonHandle) -> Result<serde_json::Value, DaemonError> {
    let mut last_error = None;
    for _ in 0..10 {
        match daemon.call("health", serde_json::Value::Null) {
            Ok(value) => return Ok(value),
            Err(error)
                if matches!(
                    &error,
                    DaemonError::Protocol(ProtocolError::Io(io_error))
                        if io_error.kind() == std::io::ErrorKind::BrokenPipe
                ) =>
            {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("health retry error"))
}

#[allow(clippy::too_many_lines)]
fn main() {
    let mut failures = Vec::new();
    let request = RouteRequest {
        operations: vec![RouteOperation::Investigation, RouteOperation::Lookup],
        scope: Some(RouteScope::Repository),
        ..RouteRequest::default()
    };
    let config = RouterConfig::default();
    let mut router_samples = (0..SAMPLES)
        .map(|_| {
            let started = Instant::now();
            black_box(route(black_box(&request), black_box(config)));
            started.elapsed().as_micros()
        })
        .collect::<Vec<_>>();
    let router_p50 = percentile(&mut router_samples, 50);
    let router_p95 = percentile(&mut router_samples, 95);
    if router_p50 >= 25_000 {
        failures.push(format!("router p50 {router_p50}us >= 25000us"));
    }
    if router_p95 >= 75_000 {
        failures.push(format!("router p95 {router_p95}us >= 75000us"));
    }

    let root = env::current_dir()
        .expect("repository root")
        .join("fixtures/demo-repo");
    let query = "Find the session expiry boundary in the authorization flow";
    let mut context_samples = (0..SAMPLES)
        .map(|_| {
            let started = Instant::now();
            let context =
                build_baseline_context(&root, query, TokenBudget::from_u32(4_000).expect("budget"))
                    .expect("warm context build");
            black_box(context);
            started.elapsed().as_micros()
        })
        .collect::<Vec<_>>();
    let context_p50 = percentile(&mut context_samples, 50);
    let context_p95 = percentile(&mut context_samples, 95);
    if context_p50 >= 150_000 {
        failures.push(format!("context p50 {context_p50}us >= 150000us"));
    }
    if context_p95 >= 500_000 {
        failures.push(format!("context p95 {context_p95}us >= 500000us"));
    }

    let mut daemon_samples = (0..SAMPLES)
        .map(|_| {
            let directory = TempDir::new().expect("temp dir");
            let started = Instant::now();
            let daemon = DaemonHandle::start(
                &daemon_config(&directory),
                std::sync::Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
            )
            .expect("daemon start");
            daemon_health(&daemon).expect("health");
            let elapsed = started.elapsed().as_micros();
            daemon.shutdown().expect("daemon shutdown");
            elapsed
        })
        .collect::<Vec<_>>();
    let daemon_p50 = percentile(&mut daemon_samples, 50);
    if daemon_p50 >= 500_000 {
        failures.push(format!("daemon ready p50 {daemon_p50}us >= 500000us"));
    }

    let directory = TempDir::new().expect("memory temp dir");
    let daemon = DaemonHandle::start(
        &daemon_config(&directory),
        std::sync::Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
    )
    .expect("daemon start");
    thread::sleep(Duration::from_millis(50));
    let idle_memory = memory_bytes();
    daemon.shutdown().expect("daemon shutdown");
    if let Some(bytes) = idle_memory
        && bytes >= 100 * 1024 * 1024
    {
        failures.push(format!("daemon idle memory {bytes}B >= 104857600B"));
    }

    let report = fs::read_to_string("benchmarks/public-demo.json").expect("public demo report");
    let report: serde_json::Value = serde_json::from_str(&report).expect("public demo JSON");
    if report["reductionBasisPoints"].as_u64().unwrap_or_default() < 3_000 {
        failures.push("reduction is below 30%".to_owned());
    }
    if report["requiredEvidenceRecallBasisPoints"]
        .as_u64()
        .unwrap_or_default()
        < 9_500
    {
        failures.push("evidence recall is below 95%".to_owned());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "samples": SAMPLES,
            "routerMicros": {"p50": router_p50, "p95": router_p95},
            "contextMicros": {"p50": context_p50, "p95": context_p95},
            "daemonReadyMicros": {"p50": daemon_p50},
            "daemonIdleMemoryBytes": idle_memory,
            "passed": failures.is_empty(),
            "failures": failures,
        }))
        .expect("serialize result")
    );
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
