//! Host overlay on the tinyflows node-kind catalog (P1.2 / audit finding F2).
//!
//! The portable, model-level contracts live in the tinyflows crate
//! ([`tinyflows::catalog`]) — config fields, ports, examples, and the
//! structural gotchas that are true of the DSL everywhere. This module is the
//! **thin host layer**: it takes those contracts and appends the facts only
//! *this* host knows — what a `tool_call` slug resolves to (a Composio action
//! slug or an `oh:` native tool), that a Composio result is wrapped in `data`,
//! how an `agent` node receives its data (`input_context`), and which trigger
//! kinds actually dispatch here. Keeping the vendor-specific knowledge here
//! preserves tinyflows' host-agnostic invariant.
//!
//! The `list_node_kinds` / `get_node_kind_contract` agent tools and the
//! `propose_workflow` description all read the *overlaid* view via
//! [`all_node_kind_contracts`] / [`node_kind_contract`].

pub use tinyflows::catalog::{ConfigField, NodeKindContract, PortSpec, NODE_KINDS};

/// Appends this host's vendor-specific caveats to a portable tinyflows
/// contract. One arm per kind that has host-owned facts; the rest pass through
/// unchanged.
fn apply_host_overlay(contract: NodeKindContract) -> NodeKindContract {
    match contract.kind.as_str() {
        "trigger" => contract
            .with_note(
                "In THIS host only manual / schedule / app_event actually dispatch today; the \
                 other kinds save but never self-run (flows_validate warns).",
            )
            .with_note(
                "trigger_kind=app_event also needs config.toolkit + config.trigger_slug (the \
                 Composio app + event to match).",
            ),
        "agent" => contract
            .with_note(
                "Data reaches the agent via config.input_context — an explicit =-binding: \
                 \"=item\" (direct predecessor), \"=items\" (all inputs, for a fan-in), or \
                 \"=nodes.<id>.item.json\" (a specific upstream node). The agent has NO automatic \
                 access to the upstream item.",
            )
            .with_note(
                "config.prompt must be PLAIN natural-language text — no leading = and no .item \
                 woven into the prose. A prompt written as a =expression built from prose silently \
                 resolves to null and hands the agent an EMPTY prompt (rejected by the \
                 binding-resolvability gate).",
            )
            .with_note(
                "execution=per_item runs a FULL harness agent (own model context, own tool loop) \
                 per input item, so it is far more expensive than a per_item tool_call — fan out \
                 over a list you have already narrowed, not a raw fetch. In THIS host \
                 simultaneous harness turns are additionally capped process-wide (8 by default, \
                 OPENHUMAN_FLOWS_MAX_PARALLEL_AGENTS): a higher config.concurrency is throttled \
                 to that ceiling, never rejected, so the run still completes.",
            ),
        "tool_call" => contract
            .with_note(
                "config.slug is a real Composio action slug (from search_tool_catalog, e.g. \
                 GMAIL_SEND_EMAIL) OR oh:<tool_name> for a native OpenHuman tool (e.g. \
                 oh:web_search). A hallucinated/typo'd slug is a hard reject.",
            )
            .with_note(
                "Before wiring, call get_tool_contract { slug }: wire EVERY required_arg into \
                 config.args using the input_schema's REAL property names (a guessed key is \
                 rejected). Composio actions also need config.connection_ref for the account; oh: \
                 tools do not.",
            )
            .with_note(
                "A Composio tool_call's output is wrapped in `data` (ComposioExecuteResponse) — \
                 bind downstream as =nodes.<id>.item.json.data.<field>, NOT .item.json.<field>. To \
                 split_out over its result list, use get_tool_contract's primary_array_path \
                 prefixed with `json.` (e.g. \"json.data.messages\").",
            ),
        "http_request" => contract.with_note(
            "config.connection_ref is an http_cred:<name> credential for authentication.",
        ),
        "code" => contract.with_note(
            "A code node's output is NOT `data`-wrapped (unlike a Composio tool_call) — bind \
             downstream as =nodes.<id>.item.json.<field>.",
        ),
        "split_out" => contract.with_note(
            "For a Composio source whose get_tool_contract primary_array_path is null, do NOT \
             default the path to \"json.data\" (that targets the whole payload container and \
             yields one item) — probe the real array path with get_tool_output_sample instead.",
        ),
        "memory" => contract.with_note(
            "scope: \"flow\" reads/writes the SAME per-flow memory namespace the \
             flow_memory_recall / flow_memory_remember agent tools use — a memory[remember] \
             node and a flow_memory_remember tool call inside an agent node on the same flow \
             see each other's writes. For exact \"process each item once\" dedup, use a dedup \
             node instead — semantic recall is similarity-ranked, not an exact membership check, \
             so a recall→condition graph cannot safely express it.",
        ),
        "dedup" => contract
            .with_note(
                "Commit is run-LEVEL, not node-level: the host settles every dedup node in the \
                 flow off the run's single terminal FlowRunFinished status. Only \
                 completed/completed_with_warnings unions this run's tentative keys into \
                 committed for EVERY dedup node that ran; EVERY other status — \
                 failed/cancelled/interrupted, unknown, or any status this host doesn't \
                 recognize yet — releases tentative for ALL of them (untouched committed), so \
                 the whole run's items retry next time — one node failing mid-run releases every \
                 dedup node's tentative in that run, not just the failing one's.",
            )
            .with_note(
                "Canonical placement: split_out → dedup [key=\"=item.id\"] → …action…, i.e. one \
                 dedup node right after the items are produced, keyed on a stable id that exists \
                 at that point (issue number, message id, url), placed BEFORE the action. It \
                 already marks a key seen only after the run succeeds, so don't also wire a \
                 separate memory remember/condition dedupe graph alongside it.",
            ),
        "loop" => contract
            .with_note(
                "Every pass through the body costs what the body costs. A loop whose body \
                 contains an agent node runs a full agent turn per iteration, so max_iterations \
                 is a spend bound as much as a correctness one — prefer the smallest cap that \
                 can finish the job, and an `=`-expression `condition` so the loop stops as soon \
                 as the work is done rather than always running to the cap.",
            )
            .with_note(
                "A sub_workflow node is the other way to repeat work, and the two bound \
                 differently: this node's max_iterations counts passes within one run (default \
                 25), while nested sub_workflow runs are bounded by max_sub_workflow_depth on \
                 the ROOT graph's trigger (default 8). Reach for a loop to repeat a section, \
                 and for sub_workflow to reuse a whole flow.",
            ),
        "spawn" => contract
            .with_note(
                "config.slug for target=tool follows the SAME rule as a tool_call: a real \
                 Composio action slug (with config.connection_ref for the account) or \
                 oh:<tool_name> for a native OpenHuman tool. Call get_tool_contract first and \
                 wire every required_arg into config.args.",
            )
            .with_note(
                "THIS host wires a TaskRunner, so spawned work genuinely overlaps. It is \
                 in-process only: tickets do not survive a core restart, so a spawn whose gate \
                 would only be reached after one is a spawn whose result is lost — keep the \
                 spawn and its gate inside the same run.",
            ),
        "gate" => contract.with_note(
            "wait_mode=\"suspend\" interrupts the run, and in THIS host an interrupted flow run \
             is resumed through flows_resume against the durable checkpointer — so a suspended \
             gate survives a restart where a polling one does not. Prefer it for anything \
             waiting longer than seconds.",
        ),
        "scatter" => contract.with_note(
            "Lanes multiply everything inside the region, including COST: a lane body \
             containing an agent node runs a full harness turn per lane. This host additionally \
             caps simultaneous harness turns process-wide (8 by default, \
             OPENHUMAN_FLOWS_MAX_PARALLEL_AGENTS), so a 200-lane scatter over an agent node \
             queues rather than running 200 wide — correct, but not the throughput the lane \
             count suggests. Use config.lanes to chunk deliberately.",
        ),
        _ => contract,
    }
}

/// Every node-kind contract with this host's overlay applied, in
/// [`NODE_KINDS`] order.
pub fn all_node_kind_contracts() -> Vec<NodeKindContract> {
    tinyflows::catalog::all_contracts()
        .into_iter()
        .map(apply_host_overlay)
        .collect()
}

/// The overlaid contract for one node kind, or `None` when `kind` is not one
/// of [`NODE_KINDS`].
pub fn node_kind_contract(kind: &str) -> Option<NodeKindContract> {
    tinyflows::catalog::contract_for(kind).map(apply_host_overlay)
}

/// Renders the compact, one-line-per-kind node-kind enumeration used to keep
/// `propose_workflow`'s description honest against the typed contracts (drift
/// test). Format: `kind [required config.a/config.b; optional config.c] —
/// summary`, joined by ` | `.
pub fn render_node_kinds_line() -> String {
    all_node_kind_contracts()
        .iter()
        .map(|c| {
            let required: Vec<&str> = c
                .config_fields
                .iter()
                .filter(|f| f.required)
                .map(|f| f.name.as_str())
                .collect();
            let optional: Vec<&str> = c
                .config_fields
                .iter()
                .filter(|f| !f.required)
                .map(|f| f.name.as_str())
                .collect();
            let mut cfg = String::new();
            if !required.is_empty() {
                cfg.push_str(&format!("required config.{}", required.join("/config.")));
            }
            if !optional.is_empty() {
                if !cfg.is_empty() {
                    cfg.push_str("; ");
                }
                cfg.push_str(&format!("optional config.{}", optional.join("/config.")));
            }
            if cfg.is_empty() {
                format!("{} ({})", c.kind, c.summary)
            } else {
                format!("{} [{}] — {}", c.kind, cfg, c.summary)
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_preserves_every_kind() {
        // Counted from NODE_KINDS rather than a literal: the overlay must keep
        // pace with the engine's catalog, and pinning a number here only ever
        // reported "tinyflows added a kind", which is not this test's job.
        assert_eq!(all_node_kind_contracts().len(), NODE_KINDS.len());
        for kind in NODE_KINDS {
            assert!(node_kind_contract(kind).is_some(), "missing {kind}");
        }
        assert!(node_kind_contract("not_a_kind").is_none());
    }

    #[test]
    fn memory_overlay_adds_flow_memory_coherence_facts_and_redirects_dedup_to_its_own_node() {
        let c = node_kind_contract("memory").unwrap();
        let notes = c.notes.join("\n");
        assert!(notes.contains("flow_memory_recall"), "{notes}");
        assert!(notes.contains("flow_memory_remember"), "{notes}");
        assert!(notes.contains("SAME per-flow memory namespace"), "{notes}");
        // The recall→condition dedupe recipe stays gone (P1 review fix):
        // semantic recall cannot express exact "have I seen this key"
        // membership, so the overlay must not teach that pattern.
        assert!(!notes.contains("Canonical dedupe pattern"), "{notes}");
        assert!(!notes.contains("item.json.found"), "{notes}");
        // The "deferred to a dedicated primitive" note is gone now that the
        // dedup node exists — the memory overlay redirects to it instead.
        assert!(
            !notes.contains("deferred to a dedicated primitive"),
            "{notes}"
        );
        assert!(notes.contains("use a dedup node instead"), "{notes}");
    }

    #[test]
    fn dedup_overlay_teaches_run_level_commit_semantics_and_placement() {
        let c = node_kind_contract("dedup").unwrap();
        let notes = c.notes.join("\n");
        assert!(notes.contains("FlowRunFinished"), "{notes}");
        assert!(notes.contains("completed_with_warnings"), "{notes}");
        assert!(notes.contains("failed/cancelled/interrupted"), "{notes}");
        // CodeRabbit (PR #5265): the release path is really "every status
        // other than the two success strings" — `unknown` and any future
        // status must be documented alongside the known failure statuses.
        assert!(notes.contains("unknown"), "{notes}");
        assert!(notes.contains("split_out → dedup"), "{notes}");
    }

    #[test]
    fn tool_call_overlay_adds_host_composio_facts() {
        let c = node_kind_contract("tool_call").unwrap();
        let notes = c.notes.join("\n");
        // Host facts that must NOT live in the portable crate.
        assert!(notes.contains("Composio"), "{notes}");
        assert!(notes.contains("oh:"), "{notes}");
        assert!(notes.contains("data"), "{notes}");
        assert!(notes.contains("get_tool_contract"), "{notes}");
    }

    #[test]
    fn agent_overlay_adds_input_context_guidance() {
        let c = node_kind_contract("agent").unwrap();
        assert!(c.notes.iter().any(|n| n.contains("input_context")));
    }

    #[test]
    fn trigger_overlay_names_the_host_dispatch_set() {
        let c = node_kind_contract("trigger").unwrap();
        assert!(c.notes.iter().any(|n| n.contains("app_event")));
    }

    #[test]
    fn merge_has_no_overlay_and_stays_portable() {
        // A kind with no host facts is byte-identical to the portable contract.
        assert_eq!(
            node_kind_contract("merge").unwrap(),
            tinyflows::catalog::contract_for("merge").unwrap()
        );
    }

    #[test]
    fn rendered_line_covers_every_kind_and_required_field() {
        let line = render_node_kinds_line();
        for c in all_node_kind_contracts() {
            assert!(
                line.contains(&c.kind),
                "rendered line missing kind {}",
                c.kind
            );
            for f in c.config_fields.iter().filter(|f| f.required) {
                assert!(
                    line.contains(&format!("config.{}", f.name)),
                    "rendered line missing required field config.{} for {}",
                    f.name,
                    c.kind
                );
            }
        }
    }
}

#[cfg(test)]
mod prompt_index_tests {
    use super::*;

    /// The `workflow_builder` prompt, as compiled into the binary.
    const BUILDER_PROMPT: &str = include_str!("agents/workflow_builder/prompt.md");

    /// Every node kind must appear in the prompt's index table.
    ///
    /// The prompt used to carry ~20 KB enumerating each kind's config fields,
    /// ports and gotchas — a duplicate of what `get_node_kind_contract` serves,
    /// and one the prompt itself flagged as such ("when it and the contract
    /// tool disagree, the tool wins"). That detail is gone; what remains is a
    /// one-line-per-kind index so the model knows what exists without a tool
    /// call.
    ///
    /// An index is only useful while it is complete. A kind added to the
    /// catalog and not to the table is invisible to the builder unless it
    /// happens to call `list_node_kinds`, which is exactly the failure a
    /// summary is supposed to prevent.
    #[test]
    fn the_prompt_index_lists_every_node_kind() {
        let table = BUILDER_PROMPT
            .split("### The node kinds")
            .nth(1)
            .expect("the prompt carries a node-kind index");
        let missing: Vec<&str> = NODE_KINDS
            .iter()
            .copied()
            .filter(|kind| !table.contains(&format!("`{kind}`")))
            .collect();
        assert!(
            missing.is_empty(),
            "node kinds missing from the workflow_builder prompt index: {missing:?}. \
             Add a row to the table in `agents/workflow_builder/prompt.md`."
        );
    }

    /// …and must not list a kind the catalog does not have.
    ///
    /// The opposite drift: a kind removed upstream leaves a row advertising a
    /// node the validator will reject, which is worse than no row at all.
    #[test]
    fn the_prompt_index_lists_no_kind_the_catalog_lacks() {
        let table = BUILDER_PROMPT
            .split("### The node kinds")
            .nth(1)
            .and_then(|rest| rest.split("\n### ").next())
            .expect("the index table is delimited by the next subsection");
        let known: Vec<&str> = NODE_KINDS.to_vec();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let kind = line
                .trim_start_matches("| `")
                .split('`')
                .next()
                .unwrap_or_default();
            assert!(
                known.contains(&kind),
                "the prompt index lists `{kind}`, which is not in the node-kind catalog"
            );
        }
    }
}

#[cfg(test)]
mod render_size_probe {
    use super::*;
    #[test]
    #[ignore = "measurement probe, not an assertion"]
    fn print_rendered_line_size() {
        let line = render_node_kinds_line();
        println!("RENDERED_BYTES={}", line.len());
        println!("{line}");
    }
}
