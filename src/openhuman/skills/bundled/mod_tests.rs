//! Tests for the compiled-in skill table and its materialisation.

use super::*;

const A: BundledFile = BundledFile {
    path: "WORKFLOW.md",
    contents: "---\nname: sample\ndescription: a sample\n---\n\nbody\n",
};
const B: BundledFile = BundledFile {
    path: "references/detail.md",
    contents: "detail\n",
};

const SAMPLE: BundledSkill = BundledSkill {
    dir_name: "sample",
    files: &[A, B],
};

#[test]
fn every_shipped_bundle_is_valid() {
    // The one assertion that covers the real table. `validate` exists because
    // a bad `dir_name` or a `..` in a file path would write outside the
    // workspace; a shipped build never runs it, so this is where it runs.
    for skill in BUNDLED {
        skill
            .validate()
            .unwrap_or_else(|e| panic!("bundled skill `{}` is invalid: {e}", skill.dir_name));
    }
}

#[test]
fn shipped_bundle_names_are_unique() {
    // Two entries with one `dir_name` would write over each other, and which
    // one won would depend on table order.
    let mut seen = std::collections::HashSet::new();
    for skill in BUNDLED {
        assert!(
            seen.insert(skill.dir_name),
            "two bundled skills both claim `{}`",
            skill.dir_name
        );
    }
}

#[test]
fn the_digest_covers_paths_as_well_as_contents() {
    // The length-prefix rule from `digest`'s docs: moving a byte between two
    // files, or renaming a file, must change the digest. Without prefixing,
    // both of these collide with SAMPLE.
    let moved = BundledSkill {
        dir_name: "sample",
        files: &[
            BundledFile {
                path: "WORKFLOW.md",
                contents: A.contents,
            },
            BundledFile {
                path: "references/detail.md",
                contents: "detai",
            },
        ],
    };
    let renamed = BundledSkill {
        dir_name: "sample",
        files: &[
            A,
            BundledFile {
                path: "references/other.md",
                contents: B.contents,
            },
        ],
    };
    assert_ne!(SAMPLE.digest(), moved.digest());
    assert_ne!(SAMPLE.digest(), renamed.digest());
}

#[test]
fn a_traversal_path_is_rejected() {
    // Checked against the free function rather than through `validate`,
    // because `BundledSkill::files` is `&'static [_]` — a table compiled into
    // the binary cannot be assembled from loop variables, which is itself part
    // of why the table is safe.
    for bad in [
        "../escape.md",
        "refs/../../escape.md",
        "/etc/passwd",
        "refs//x.md",
        "C:/windows/system32",
        ".hidden.md",
        "",
    ] {
        assert!(
            validate_relative_path("sample", bad).is_err(),
            "`{bad}` must be rejected as a bundled file path"
        );
    }
    for good in ["WORKFLOW.md", "references/expressions.md", "scripts/run.py"] {
        assert!(
            validate_relative_path("sample", good).is_ok(),
            "`{good}` must be accepted"
        );
    }
}

#[test]
fn a_bad_dir_name_is_rejected() {
    for bad in ["", ".hidden", "a/b", "a\\b"] {
        let skill = BundledSkill {
            dir_name: bad,
            files: &[A],
        };
        assert!(
            skill.validate().is_err(),
            "`{bad}` must be rejected as a dir_name"
        );
    }
}

#[test]
fn a_bundle_with_no_manifest_is_rejected() {
    // Discovery only loads a directory holding WORKFLOW.md / SKILL.md /
    // skill.json. A bundle without one would be written out and then silently
    // never appear — the worst failure mode available, because nothing errors.
    let skill = BundledSkill {
        dir_name: "sample",
        files: &[BundledFile {
            path: "references/detail.md",
            contents: "x",
        }],
    };
    assert!(skill.validate().is_err());
}

#[test]
fn install_writes_the_files_and_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = builtin_root(tmp.path());

    assert!(install_one(&root, &SAMPLE).expect("first install"));
    let body = std::fs::read_to_string(root.join("sample").join("WORKFLOW.md")).expect("body");
    assert_eq!(body, A.contents);
    let detail =
        std::fs::read_to_string(root.join("sample").join("references/detail.md")).expect("detail");
    assert_eq!(detail, B.contents);

    // Second call must not rewrite — the digest matches.
    assert!(!install_one(&root, &SAMPLE).expect("second install"));
}

#[test]
fn a_version_bump_removes_a_file_the_new_version_dropped() {
    // The reason `install_one` deletes instead of overwriting. A stale
    // reference doc left behind would keep answering `read_workflow_resource`
    // after the skill stopped shipping it.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = builtin_root(tmp.path());
    assert!(install_one(&root, &SAMPLE).expect("install v1"));

    let v2 = BundledSkill {
        dir_name: "sample",
        files: &[A],
    };
    assert!(install_one(&root, &v2).expect("install v2"));
    assert!(
        !root.join("sample").join("references/detail.md").exists(),
        "the dropped reference file must not survive the upgrade"
    );
}

#[test]
fn an_interrupted_install_is_redone() {
    // The digest is written last on purpose. Simulate the interruption by
    // removing it: the next install must rewrite rather than trust the
    // directory.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = builtin_root(tmp.path());
    assert!(install_one(&root, &SAMPLE).expect("install"));
    std::fs::remove_file(root.join("sample").join(".digest")).expect("remove digest");
    assert!(
        install_one(&root, &SAMPLE).expect("reinstall"),
        "a bundle with no digest must be rewritten"
    );
}

#[test]
fn install_reports_rather_than_fails_the_boot() {
    // `install` is called from workspace init. A workspace it cannot write
    // must not stop the core from starting.
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = install(tmp.path());
    assert!(
        report.failed.is_empty(),
        "clean workspace must install: {report:?}"
    );
    assert_eq!(
        report.written.len() + report.unchanged.len(),
        BUNDLED.len(),
        "every bundled skill must be accounted for"
    );
}

#[test]
fn a_materialised_bundle_is_discoverable_and_readable_end_to_end() {
    // The test that proves the whole mechanism, because every other test here
    // stops at the filesystem. A bundle can be written correctly and still be
    // useless: discovery could skip the builtin root, the scope could be
    // rejected, the frontmatter could fail to parse, or the resource reader
    // could refuse a path under a root it does not recognise. This walks the
    // real path the model walks — install, discover, read a page.
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path();
    let report = install(workspace);
    assert!(report.failed.is_empty(), "install failed: {report:?}");

    for skill in BUNDLED {
        let found = crate::openhuman::skills::discover_workflows(None, Some(workspace), false);
        let entry = found
            .iter()
            .find(|w| w.dir_name == skill.dir_name)
            .unwrap_or_else(|| {
                panic!(
                    "`{}` was written but discovery did not find it; discovered: {:?}",
                    skill.dir_name,
                    found.iter().map(|w| &w.dir_name).collect::<Vec<_>>()
                )
            });
        assert_eq!(
            entry.scope,
            crate::openhuman::skills::ops_types::WorkflowScope::Builtin,
            "a materialised bundle must carry the Builtin scope"
        );
        assert!(
            !entry.description.is_empty(),
            "`{}` frontmatter did not parse — discovery found the directory but \
             not its metadata",
            skill.dir_name
        );

        // `home_dir = None` above; the reader resolves through the real
        // discovery pipeline including `dirs::home_dir()`, which is fine here:
        // it can only ADD skills, and we look up ours by name.
        for file in skill.files {
            if file.path == super::super::ops_types::WORKFLOW_MD {
                continue;
            }
            let body = crate::openhuman::skills::read_workflow_resource(
                workspace,
                skill.dir_name,
                std::path::Path::new(file.path),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "reading `{}` of `{}` failed: {e}",
                    file.path, skill.dir_name
                )
            });
            assert_eq!(
                body, file.contents,
                "`{}` read back different bytes than it ships",
                file.path
            );
        }
    }
}

#[test]
fn a_user_skill_of_the_same_name_shadows_the_builtin() {
    // The precedence promise from the module docs, and the one that makes
    // shipping a bundle safe: adding a builtin can never take a name away from
    // a workspace that already used it.
    let Some(first) = BUNDLED.first() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path();
    install(workspace);

    // A legacy-scope skill (`<workspace>/skills/`) is the lowest non-builtin
    // scope, so if even that wins, every other scope does.
    let user_dir = workspace.join("skills").join(first.dir_name);
    std::fs::create_dir_all(&user_dir).expect("mkdir");
    std::fs::write(
        user_dir.join("SKILL.md"),
        format!(
            "---\nname: {}\ndescription: the user's own version\n---\n\nmine\n",
            first.dir_name
        ),
    )
    .expect("write");

    let found = crate::openhuman::skills::discover_workflows(None, Some(workspace), false);
    let entry = found
        .iter()
        .find(|w| w.dir_name == first.dir_name)
        .expect("found");
    assert_eq!(
        entry.description, "the user's own version",
        "the user's skill must win the name; got scope {:?}",
        entry.scope
    );
}
