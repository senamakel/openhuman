use super::*;

fn profile(id: &str, dedicated: bool) -> AgentProfile {
    let mut p = crate::agent::spec::blank_profile(id);
    p.dedicated_memory = dedicated;
    p
}

#[test]
fn dedicated_memory_keys_transcripts_by_agent_id() {
    let ws = Path::new("/r/workspace");
    let layout = AgentLayout::resolve(ws, &profile("alpha", true), PathBuf::from("/r/a"));
    assert_eq!(layout.home, Path::new("/r/workspace/personalities/alpha"));
    assert_eq!(
        layout.skills,
        Path::new("/r/workspace/personalities/alpha/skills")
    );
    assert_eq!(
        layout.transcripts,
        Path::new("/r/workspace/session_raw-alpha")
    );
    assert_eq!(layout.action_dir, Path::new("/r/a"));
}

#[test]
fn shared_memory_uses_the_workspace_transcript_root() {
    let ws = Path::new("/r/workspace");
    let layout = AgentLayout::resolve(ws, &profile("alpha", false), PathBuf::from("/r/a"));
    assert_eq!(layout.transcripts, Path::new("/r/workspace/session_raw"));
}

#[test]
fn default_action_dir_is_a_workspace_sibling_or_a_profile_dir() {
    assert_eq!(
        AgentLayout::default_action_dir(Path::new("/r"), Path::new("/r/action"), false, "a"),
        Path::new("/r/agents/a/action")
    );
    assert_eq!(
        AgentLayout::default_action_dir(
            Path::new("/home/u/.openhuman"),
            Path::new("/home/u/OpenHuman/projects"),
            true,
            "a"
        ),
        Path::new("/home/u/OpenHuman/projects/profiles/a")
    );
}
