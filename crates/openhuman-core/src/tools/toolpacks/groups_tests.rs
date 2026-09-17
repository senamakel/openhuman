use super::*;

#[test]
fn the_default_withholds_every_compiled_in_group() {
    // Every compiled-in group withheld: the fail-closed floor.
    //
    // This deliberately no longer claims to pin "no behaviour change". It did,
    // and that reading is what let a regression through: the default equalled
    // the pre-`ToolGroups` table only while the packs held nothing a host used,
    // and every family that moves into a pack silently narrows what a host with
    // no posture of its own receives. What this pins is the floor itself —
    // that the default never drifts *open* — not that the floor is inert.
    let g = ToolGroups::default();
    for id in ToolGroups::ids() {
        assert_eq!(g.mode(id), GroupMode::Withheld, "group `{id}` drifted");
    }
}

#[test]
fn presets_are_uniform() {
    for id in ToolGroups::ids() {
        assert_eq!(ToolGroups::advertised().mode(id), GroupMode::Advertised);
        assert_eq!(ToolGroups::none().mode(id), GroupMode::Off);
        assert_eq!(ToolGroups::packed().mode(id), GroupMode::Withheld);
    }
}

#[test]
fn with_sets_one_group_and_leaves_the_rest() {
    let g = ToolGroups::none().with("documents", GroupMode::Advertised);
    assert_eq!(g.mode("documents"), GroupMode::Advertised);
    assert_eq!(g.mode("crypto"), GroupMode::Off);
}

#[test]
fn an_unknown_group_id_is_ignored_not_fatal() {
    // A group id is data. A build that compiled a family out should not
    // panic a host whose config still names it.
    let g = ToolGroups::none().with("no-such-group", GroupMode::Advertised);
    for id in ToolGroups::ids() {
        assert_eq!(g.mode(id), GroupMode::Off);
    }
}

#[test]
fn a_tool_in_no_group_is_never_withheld() {
    // `mode_for_tool` is consulted for every tool in the belt, most of
    // which belong to no pack. Reporting anything but `Advertised` there
    // would withhold the baseline surface.
    assert_eq!(
        ToolGroups::none().mode_for_tool("apply_patch"),
        GroupMode::Advertised
    );
    assert_eq!(
        ToolGroups::none().mode_for_tool("shell"),
        GroupMode::Advertised
    );
}

#[test]
fn mode_for_tool_follows_its_pack() {
    let g = ToolGroups::default().with("system", GroupMode::Off);
    assert_eq!(g.mode_for_tool("doctor_health"), GroupMode::Off);
    // A different pack is untouched.
    assert_eq!(g.mode_for_tool("wallet_status"), GroupMode::Withheld);
}

#[test]
fn every_group_id_is_reachable_by_name() {
    // `ids()` is what an embedder enumerates; `index_of` is what `with`
    // resolves. A pack id that round-trips through neither would be
    // unselectable from the library surface.
    for id in ToolGroups::ids() {
        assert!(ToolGroups::index_of(id).is_some(), "`{id}` is unselectable");
    }
    assert_eq!(ToolGroups::ids().count(), GROUP_COUNT);
}

#[test]
fn a_scoped_context_outranks_the_process_default() {
    // Multi-tenant dispatch passes a context per call; the process default must
    // never override it, or one embedder's posture would decide another's.
    let resolved = super::resolve(Some(ToolGroups::none()), Some(ToolGroups::advertised()));
    assert_eq!(resolved, ToolGroups::none());
}

#[test]
fn the_process_default_answers_when_no_context_is_scoped() {
    // The gap this exists to close: a host that establishes no `CoreContext`
    // could previously only resolve to `Default` — every group withheld — and
    // so lost a tool family every time one moved into a pack.
    let resolved = super::resolve(None, Some(ToolGroups::advertised()));
    assert_eq!(resolved, ToolGroups::advertised());
}

#[test]
fn with_neither_set_the_floor_is_still_fail_closed() {
    // Unchanged for every existing host: no context, no process default, every
    // pack withheld.
    assert_eq!(super::resolve(None, None), ToolGroups::default());
    assert_eq!(super::resolve(None, None), ToolGroups::packed());
}
