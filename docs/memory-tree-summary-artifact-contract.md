# Memory Tree Summary Artifact Contract

Issue: `#1447`

## New summaries

New summary rows continue to use the canonical `summary_id` grammar already emitted by
`tree_source::registry::new_summary_id`:

- `summary:{13-digit-ms}:L{level}-{8-hex-tail}`

New on-disk summary filenames are derived from that id by an explicit mapping, not by the
generic filename sanitizer as an implementation side effect:

- `summary:{ms}:L{level}-{tail}` -> `summary-{ms}-L{level}-{tail}.md`

This keeps one supported basename story for new data across source, global, and topic trees.

## Legacy compatibility

Older vaults can already contain legacy summary ids such as:

- `summary:L{level}:{rest}` -> `summary-L{level}-{rest}.md`

Compatibility plan:

- Existing summary reads and tag rewrites remain DB-pointer-driven via `content_path`; they do not
  reconstruct paths from `summary_id`, so mixed vaults keep resolving without silent 404s.
- We do not rename legacy files in this change.
- `reset_tree` remains the coarse rebuild path for operators who want a fully normalized
  `wiki/summaries/` subtree, because it deletes and regenerates derived summaries from the current
  core build.

## Provenance metadata

Summary markdown frontmatter now carries:

- `openhuman_core_version`
- `memory_artifact_format`

New summary writes stamp both fields at compose time. Summary tag rewrites also upsert them, so
existing files gain provenance opportunistically without changing the body bytes or the stored body
SHA contract.
