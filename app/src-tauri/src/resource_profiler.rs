//! On-demand CPU and resident-memory profiling for the desktop process tree.
//!
//! The Rust core is embedded in the Tauri host, so the operating system cannot
//! attribute resource usage separately to `openhuman_core` and the shell. The
//! host row therefore reports their combined cost. CEF runs renderer, GPU, and
//! utility work in child processes, which can be attributed independently.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sysinfo::{Pid, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL};

const SAMPLE_WINDOW: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResourceComponent {
    TauriHostAndEmbeddedCore,
    CefRenderer,
    CefGpu,
    CefUtility,
    CefOther,
    OtherChild,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ComponentResourceUsage {
    component: ResourceComponent,
    #[serde(flatten)]
    totals: ResourceTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ResourceTotals {
    process_count: usize,
    memory_bytes: u64,
    /// Percentage of one logical CPU. This may exceed 100 when a component
    /// keeps more than one logical CPU busy during the sample window.
    cpu_percent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ResourceProfileSnapshot {
    sampled_at_unix_ms: u128,
    sample_window_ms: u64,
    logical_cpu_count: usize,
    rust_binary: ResourceTotals,
    desktop_total: ResourceTotals,
    components: Vec<ComponentResourceUsage>,
    attribution_note: &'static str,
}

#[derive(Debug, Clone)]
struct ProcessSample {
    pid: u32,
    parent_pid: Option<u32>,
    name: String,
    command: String,
    memory_bytes: u64,
    cpu_percent: f32,
}

/// Capture two process-table readings so `sysinfo` can derive CPU usage.
///
/// This is deliberately an explicit command rather than a background task:
/// opening Developer Options costs nothing until the user requests a sample.
pub(crate) async fn snapshot() -> Result<ResourceProfileSnapshot, String> {
    tokio::task::spawn_blocking(snapshot_blocking)
        .await
        .map_err(|err| format!("resource profiler task failed: {err}"))?
}

fn snapshot_blocking() -> Result<ResourceProfileSnapshot, String> {
    let mut system = System::new_all();
    std::thread::sleep(SAMPLE_WINDOW.max(MINIMUM_CPU_UPDATE_INTERVAL));
    system.refresh_processes(ProcessesToUpdate::All, true);

    let samples = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessSample {
            pid: pid.as_u32(),
            parent_pid: process.parent().map(Pid::as_u32),
            name: process.name().to_string_lossy().into_owned(),
            command: process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            memory_bytes: process.memory(),
            cpu_percent: process.cpu_usage(),
        })
        .collect::<Vec<_>>();

    build_snapshot(
        std::process::id(),
        system.cpus().len(),
        SAMPLE_WINDOW.max(MINIMUM_CPU_UPDATE_INTERVAL),
        &samples,
    )
}

fn build_snapshot(
    root_pid: u32,
    logical_cpu_count: usize,
    sample_window: Duration,
    samples: &[ProcessSample],
) -> Result<ResourceProfileSnapshot, String> {
    let by_pid = samples
        .iter()
        .map(|sample| (sample.pid, sample))
        .collect::<HashMap<_, _>>();
    if !by_pid.contains_key(&root_pid) {
        return Err(format!(
            "resource profiler could not find host pid {root_pid}"
        ));
    }

    let process_tree = samples
        .iter()
        .filter(|sample| belongs_to_tree(sample.pid, root_pid, &by_pid))
        .collect::<Vec<_>>();

    let mut grouped: BTreeMap<ResourceComponent, ComponentResourceUsage> = BTreeMap::new();
    for sample in process_tree {
        let component = classify_process(sample, root_pid);
        let usage = grouped
            .entry(component)
            .or_insert_with(|| ComponentResourceUsage {
                component,
                totals: ResourceTotals {
                    process_count: 0,
                    memory_bytes: 0,
                    cpu_percent: 0.0,
                },
            });
        usage.totals.process_count += 1;
        usage.totals.memory_bytes = usage
            .totals
            .memory_bytes
            .saturating_add(sample.memory_bytes);
        usage.totals.cpu_percent += sample.cpu_percent;
    }

    let rust_binary = grouped
        .get(&ResourceComponent::TauriHostAndEmbeddedCore)
        .map(|usage| usage.totals.clone())
        .ok_or_else(|| "resource profiler did not classify the host process".to_string())?;
    let desktop_total = ResourceTotals {
        process_count: grouped
            .values()
            .map(|usage| usage.totals.process_count)
            .sum(),
        memory_bytes: grouped
            .values()
            .map(|usage| usage.totals.memory_bytes)
            .sum(),
        cpu_percent: grouped.values().map(|usage| usage.totals.cpu_percent).sum(),
    };

    Ok(ResourceProfileSnapshot {
        sampled_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        sample_window_ms: sample_window.as_millis() as u64,
        logical_cpu_count,
        rust_binary,
        desktop_total,
        components: grouped.into_values().collect(),
        attribution_note: "The Rust core runs in-process with the Tauri shell, so their CPU and RAM are reported together. CEF subprocesses are attributed by process role.",
    })
}

fn belongs_to_tree(pid: u32, root_pid: u32, by_pid: &HashMap<u32, &ProcessSample>) -> bool {
    let mut current = Some(pid);
    let mut seen = HashSet::new();
    while let Some(candidate) = current {
        if candidate == root_pid {
            return true;
        }
        if !seen.insert(candidate) {
            return false;
        }
        current = by_pid.get(&candidate).and_then(|sample| sample.parent_pid);
    }
    false
}

fn classify_process(sample: &ProcessSample, root_pid: u32) -> ResourceComponent {
    if sample.pid == root_pid {
        return ResourceComponent::TauriHostAndEmbeddedCore;
    }

    let identity = format!("{} {}", sample.name, sample.command).to_ascii_lowercase();
    if identity.contains("--type=renderer") || identity.contains("renderer") {
        ResourceComponent::CefRenderer
    } else if identity.contains("--type=gpu-process")
        || identity.contains("gpu process")
        || identity.contains("gpu-process")
    {
        ResourceComponent::CefGpu
    } else if identity.contains("--type=utility") || identity.contains("utility") {
        ResourceComponent::CefUtility
    } else if identity.contains("--type=zygote")
        || identity.contains("--type=broker")
        || identity.contains("crashpad")
        || identity.contains("cef")
    {
        ResourceComponent::CefOther
    } else {
        ResourceComponent::OtherChild
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(
        pid: u32,
        parent_pid: Option<u32>,
        name: &str,
        command: &str,
        memory_bytes: u64,
        cpu_percent: f32,
    ) -> ProcessSample {
        ProcessSample {
            pid,
            parent_pid,
            name: name.into(),
            command: command.into(),
            memory_bytes,
            cpu_percent,
        }
    }

    #[test]
    fn classifies_cef_process_roles_without_exposing_commands() {
        let renderer = process(
            2,
            Some(1),
            "OpenHuman Helper (Renderer)",
            "--type=renderer",
            1,
            1.0,
        );
        let gpu = process(
            3,
            Some(1),
            "OpenHuman Helper (GPU)",
            "--type=gpu-process",
            1,
            1.0,
        );
        let utility = process(4, Some(1), "OpenHuman Helper", "--type=utility", 1, 1.0);

        assert_eq!(
            classify_process(&renderer, 1),
            ResourceComponent::CefRenderer
        );
        assert_eq!(classify_process(&gpu, 1), ResourceComponent::CefGpu);
        assert_eq!(classify_process(&utility, 1), ResourceComponent::CefUtility);
    }

    #[test]
    fn aggregates_host_and_descendants_but_excludes_unrelated_processes() {
        let samples = vec![
            process(10, Some(1), "OpenHuman", "OpenHuman", 100, 20.0),
            process(11, Some(10), "Helper", "--type=renderer", 40, 30.0),
            process(12, Some(11), "Helper", "--type=utility", 10, 5.0),
            process(99, Some(1), "unrelated", "unrelated", 1_000, 100.0),
        ];

        let snapshot = build_snapshot(10, 8, Duration::from_millis(250), &samples).unwrap();
        assert_eq!(snapshot.rust_binary.memory_bytes, 100);
        assert_eq!(snapshot.rust_binary.cpu_percent, 20.0);
        assert_eq!(snapshot.desktop_total.process_count, 3);
        assert_eq!(snapshot.desktop_total.memory_bytes, 150);
        assert_eq!(snapshot.desktop_total.cpu_percent, 55.0);
        assert_eq!(snapshot.components.len(), 3);
    }

    #[test]
    fn parent_cycles_do_not_loop_forever_or_join_the_tree() {
        let samples = vec![
            process(10, None, "OpenHuman", "OpenHuman", 100, 1.0),
            process(20, Some(21), "a", "a", 1, 1.0),
            process(21, Some(20), "b", "b", 1, 1.0),
        ];
        let by_pid = samples
            .iter()
            .map(|sample| (sample.pid, sample))
            .collect::<HashMap<_, _>>();

        assert!(!belongs_to_tree(20, 10, &by_pid));
    }
}
