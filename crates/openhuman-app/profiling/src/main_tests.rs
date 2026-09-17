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
fn classifies_cef_roles() {
    assert_eq!(
        classify_process(
            &process(2, Some(1), "OpenHuman Helper", "--type=renderer", 1, 1.0),
            1
        ),
        Component::CefRenderer
    );
    assert_eq!(
        classify_process(
            &process(3, Some(1), "OpenHuman Helper", "--type=gpu-process", 1, 1.0),
            1
        ),
        Component::CefGpu
    );
    assert_eq!(
        classify_process(
            &process(4, Some(1), "OpenHuman Helper", "--type=utility", 1, 1.0),
            1
        ),
        Component::CefUtility
    );
}

#[test]
fn groups_only_host_process_tree() {
    let processes = vec![
        process(10, Some(1), "OpenHuman", "OpenHuman", 100, 20.0),
        process(11, Some(10), "Helper", "--type=renderer", 40, 30.0),
        process(12, Some(11), "Helper", "--type=utility", 10, 5.0),
        process(99, Some(1), "unrelated", "unrelated", 1_000, 100.0),
    ];

    let grouped = group_process_tree(10, &processes).unwrap();
    assert_eq!(grouped.len(), 3);
    assert_eq!(
        grouped.iter().map(|value| value.memory_bytes).sum::<u64>(),
        150
    );
    assert_eq!(
        grouped.iter().map(|value| value.cpu_percent).sum::<f32>(),
        55.0
    );
}

#[test]
fn report_separates_rust_binary_from_desktop_total() {
    let samples = vec![TimeSample {
        elapsed_ms: 250,
        components: vec![
            ComponentSample {
                component: Component::TauriHostAndEmbeddedCore,
                process_count: 1,
                memory_bytes: 100,
                cpu_percent: 20.0,
            },
            ComponentSample {
                component: Component::CefRenderer,
                process_count: 2,
                memory_bytes: 50,
                cpu_percent: 30.0,
            },
        ],
    }];
    let report = build_report(ProfileCapture {
        host_pid: 10,
        duration: Duration::from_secs(1),
        interval: Duration::from_millis(250),
        logical_cpu_count: 8,
        samples,
        rust_cpu_modules: Vec::new(),
        cpu_stack_report: None,
        cpu_stack_error: None,
    })
    .unwrap();

    assert_eq!(report.rust_binary.mean_memory_bytes, 100);
    assert_eq!(report.desktop_total.mean_memory_bytes, 150);
    assert_eq!(report.desktop_total.peak_process_count, 3);
    assert!(render_markdown(&report).contains("Tauri host + embedded Rust core"));
}

#[test]
fn parser_requires_pid_and_clamps_cpu_interval() {
    assert!(parse_args(Vec::<String>::new()).is_err());
    let args = parse_args([
        "--pid".into(),
        "123".into(),
        "--duration".into(),
        "2".into(),
        "--interval-ms".into(),
        "1".into(),
        "--no-stacks".into(),
    ])
    .unwrap();
    assert_eq!(args.pid, 123);
    assert_eq!(args.duration, Duration::from_secs(2));
    assert_eq!(args.interval, MINIMUM_CPU_UPDATE_INTERVAL);
    assert!(!args.capture_stacks);
}

#[test]
fn parses_recursive_stack_counts_into_openhuman_modules() {
    let sample = r#"
Total number in stack (recursive counted multiple, when >=5):
    81 openhuman_core::agent::run  (in OpenHuman) + 10
    34 <openhuman_core::agent::Tool as core::future::Future>::poll  (in OpenHuman) + 2
    17 openhuman_core::memory::search  (in OpenHuman) + 4
     9 openhuman::core_process::ensure_running  (in OpenHuman) + 1
   200 tokio::runtime::park  (in OpenHuman) + 3

Sort by top of stack, same collapsed (when >= 5):
"#;
    assert_eq!(
        parse_rust_module_cpu(sample),
        vec![
            RustModuleCpu {
                module: "openhuman_core::agent".into(),
                recursive_samples: 115,
            },
            RustModuleCpu {
                module: "openhuman_core::memory".into(),
                recursive_samples: 17,
            },
            RustModuleCpu {
                module: "openhuman::core_process".into(),
                recursive_samples: 9,
            },
        ]
    );
}
