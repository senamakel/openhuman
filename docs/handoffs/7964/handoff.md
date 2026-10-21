# Epic 7964 handoff — stopped by user, 2026-10-21

Implementation is stopped. Resume only on a new instruction. All intermediate work belongs on senamakel forks, base epic/7964-channel-v2. Use Luna workers, retain every commit, and close fork issues only after full acceptance and merged PRs.

24/28 original leaves and discovered OpenHuman #547 are complete. Remaining: TinyChannels #14; OpenHuman #135, #137, #139.

## Recoverable unfinished PRs

- senamakel/tinychannels#120: DingTalk/QQ owners; branch epic-7964-14-dingtalk-qq-owners, source a45ef8273f1bdbea7517b3b9edd55876bfe822f1. Coverage passed all measured files >=90%, QQ media90.68%; new QQ36 and DingTalk35+1ignored pass. Current-source check/strict/fmt pending. OH checkpoint434a39ac2dd6362e99fbb2f57171beb2b50b08e0 on same branch.
- senamakel/tinychannels#121: Lark/Mattermost owners; branch epic-7964-14-lark-mattermost-owners, source8a453bd69720e2900946f3e744826fcab2136a19. Earlier full package2149 pass+1ignored/check/strict/fmt; coverage r1 only Larkmedia211/247=85.4251%. New audio2 and upload1 pass, current coverage/final gates pending. OH checkpointa5865b91f3877d1b39af970fe174b2821b4ab061 on same branch.
- senamakel/openhuman#596: branch epic-7964-137-host-handles, source785bbbe38972ab88670e44cb853aec0def7c12bf. Status roundtrip still returns Turn:idle. Temporary cfg(test) MarkTurn guard/result tracing is committed. Latest R8 compiled but selected0 due wrong selector; NOT PASS. Correct binary selector: channels::routes::tests::telegram::handle_runtime_command_telegram_new_status_and_sessions_round_trip. Binary: target/debug/deps/openhuman_core-9745245c013cd262 in its worktree. Typing R6 actually1PASS11.53s with real loopback OpenAI SSE/typing-before-successful-reply. R4 full channels276PASS2FAIL predates typing fix; final whole suite/static gates pending. Source reviews2/2 spent; known failed-gate diagnosis/fixes allowed, no fresh third review.
- senamakel/openhuman#597: branch epic-7964-139-final-docs. Reconcile final owner pins/paths; run pnpm docs:generate and pnpm docs:check; account for English/Chinese pages and wiki/matrix. Wiki patch saved separately in that branch; apply only at final upstream stage.
- This branch epic-7964-135-final-acceptance holds preparation and historical audit only; no final integrated acceptance has run.

## #135 acceptance

The user explicitly revoked the fixed ~4500-line target. Require verified file-by-file ownership boundaries and ALL acceptance tests. Line counts are informational. The accompanying historical audit is not current acceptance; prep-439 manifest records39 production Rust files/9327physical lines at439bc623 and excludes test support.

Integrate merged #137 plus the final merged TinyChannels owner tip, build matching module artifacts (verify source/digests/loaded mappings), then run full web_chat and JSON-RPC acceptance and >=80% changed-line coverage. Existing historical299web passes and old81%coverage are NOT final union proof.

Prepared commands:
- cargo test -p openhuman web_chat with the product features needed by the suite.
- bash scripts/test-rust-e2e.sh --suite json_rpc_e2e
- OUT=target/135-final/coverage/lcov-core.info OH_COV_TUI=1 bash scripts/ci/rust-coverage.sh
- bash scripts/ci/self-hosted/diff-cover.sh target/135-final/coverage 6e99ddca4d88832787913a3e919be68099cc02cd target/135-final/coverage-report

Use isolated mock configuration and actual tinyhumans boot. Prepared downloaded fixtures are predecessor artifacts and do not prove final owner source.

## Evidence and workflow

Local ledger: /home/enamakel/work/openhuman/.git/implementing-an-epic/tinyhumansai-openhuman-7964.json, continuation_status.root_resume_2026_10_21.
Local evidence: /home/enamakel/work/openhuman/.git/implementing-an-epic/evidence.
Source reviews1 used for each new owner split; known source findings fixed. Before merge take allowed formal feedback snapshot, refund empty snapshots, never exceed2.
TinyChannels: all13 actual current-head SUCCESS + fresh MERGEABLE, normal --merge --match-head-commit. OpenHuman: current local gates green then pinned --merge --admin. No squash/rebase/reset/amend.
Use superproject worktrees only. Primary main divergence was preserved; do not reset it. Include explicit cd to absolute worktree in commands. Long gates use scripts/ci-cancel-aware.sh; never CARGO_TARGET_DIR. Private TMPDIR under .git/implementing-an-epic/scratch; recreate before commands, avoid heredocs (/tmpquota). One heavy OpenHuman compiler at a time.
Do not touch unrelated8088/8089/8092/8131/tinysweeper work or shared build caches.

Current merged fork OH tip439bc623 viaPR566; TC tipcdcb632e via externalPR115. TC112/117/118 externally merged with checks pending but all13 later verified successful. TC115 still11/13 at stop (two macOS pending), must verify independently before counting all green. TC114/119 root merged only after13SUCCESS.

After every leaf is accepted and merged, verify fork union, normally merge current upstream defaults into each epic branch and preserve both intents. Read-only comparison at stop showed substantial drift (OH5478behind, TA1053behind; TC24behind, TF21behind); these counts are informational and need refresh. Final upstream PRs: TinyChannels/TinyAgents/TinyVoice/TinyBus/TinyFlows, then OH draft while owner dependencies outstanding. Never merge upstream.
