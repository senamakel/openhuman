use super::*;

#[test]
fn sections_sum_to_the_whole_prompt() {
    let text = "preamble line\n# Title\nbody\n## Sub\nmore body\n### Deep\ntail\n";
    let sections = split_sections(text);
    let total: usize = sections.iter().map(|s| s.bytes).sum();
    assert_eq!(
        total,
        text.len(),
        "section bytes must account for every byte of the prompt; \
             a table that does not sum is worse than no table"
    );
}

#[test]
fn preamble_is_kept_when_text_starts_before_the_first_heading() {
    let sections = split_sections("loose text\n# Title\nbody\n");
    assert_eq!(sections[0].heading, "(preamble)");
    assert_eq!(sections[0].bytes, "loose text\n".len());
}

#[test]
fn no_preamble_entry_when_the_prompt_opens_on_a_heading() {
    let sections = split_sections("# Title\nbody\n");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].heading, "# Title");
}

#[test]
fn deeper_headings_do_not_split() {
    // `####` is body text as far as this report is concerned.
    let sections = split_sections("## Sub\na\n#### Deeper\nb\n");
    assert_eq!(sections.len(), 1);
}

#[test]
fn a_bare_hash_is_not_a_heading() {
    // No trailing space: a shell comment in an example block, not a section.
    assert!(!is_heading("#!/usr/bin/env bash\n"));
    assert!(!is_heading("#hashtag\n"));
    assert!(is_heading("## Real\n"));
}

#[test]
fn tools_are_ranked_by_cost_not_registration_order() {
    let dumped = DumpedPrompt {
        agent_id: "t".into(),
        toolkit: None,
        mode: "session",
        model: "m".into(),
        workspace_dir: std::path::PathBuf::from("/tmp"),
        text: "# A\nbody\n".into(),
        tool_names: vec!["small".into(), "big".into()],
        skill_tool_count: 0,
        tool_specs: vec![
            serde_json::json!({"name": "small", "description": "s", "parameters": {}}),
            serde_json::json!({
                "name": "big",
                "description": "a much longer description than the other one",
                "parameters": {"type": "object", "properties": {"a": {"type": "string"}}}
            }),
        ],
    };
    let report = PromptSizeReport::from_dump(&dumped);
    assert_eq!(report.tools[0].name, "big");
    assert_eq!(report.tool_count, 2);
    assert_eq!(
        report.fixed_prefix_bytes,
        report.prompt_bytes + report.tool_bytes
    );
    assert!(report.tools[1].parameters_bytes > 0, "`{{}}` is two bytes");
}
