# Profile retirement and integration snapshots

This follow-up to #7513 addresses connected-integration snapshots retained
after a trusted ProfileHost retires its local profile holder. It normally
merged canonical `d1efa8bd58`, preserving the earlier cache-scope and epoch
checkpoints.

## Boundary

ProfileHost supplies its exact validated ProfileId to an internal retirement
helper, without resolving the operator's ambient task context. The helper
uses the same encoded storage scope and hash as ordinary cache reads.
Under the cache write lock, it increments the shared scope epoch and removes
that profile's snapshots across config/credential keys. An already-started
warm retaining its epoch Arc cannot publish after this invalidation.

All five slot-removal sites call the helper while holding the profile map
lock: explicit close/release/deprovision, idle eviction, budget eviction,
shutdown, and exact-grant fencing. A reopened slot cannot become visible in
this host before retirement invalidation completes. An obsolete fence grant
does not retire a successor's slot. The lock order is profile map, cache
write, then epoch map; existing cache insertion/invalidation retain their
cache-before-epoch ordering.

## Regression chronology

Test-only checkpoint `9ea9cc1b2d` added six actual ProfileHost lifecycle
controls. They use trusted provisioning/open and the normal file lease
implementation on temporary workspaces, actual profile CoreContexts and the
production cache insertion path. They do not fabricate a lease or call a
real backend.

Session `35495` compiled successfully in 2m03s and then produced
**0 passed, 6 failed, 0 ignored**. Every case observed the retired snapshot
still present after its actual lifecycle operation. The release case also
checks a cold reopen with the same profile/config/credential identity.
The fence case retains the real holder's grant and epoch, then exercises
the actual post-fence insertion guard. Other profiles' held snapshots must
remain intact.

The bounded production correction is `627f422ed4`. Initial postfix session
`9958` failed compilation on two accidentally unborrowed references in
the unchanged reconciliation code. Those original borrows were restored;
the compiler-only result is excluded from behavioral acceptance.
The original six assertions are unchanged. Fresh focused GREEN and
compatibility results are pending.

Logs are retained outside disposable build output:
`.validation/cache-profile-lifecycle-red.log`,
`.validation/cache-profile-lifecycle-green.log` and
`.validation/cache-profile-lifecycle-green-retry.log`.

The focused command is:

```sh
scripts/ci-cancel-aware.sh cargo test --locked -j 1 -p openhuman \
  --no-default-features --features storage-sqlite,mcp,modules,channels \
  --lib integrations::composio::connected_integrations::cache::lifecycle_tests \
  -- --nocapture --test-threads=1
```

## Limits

This retirement invalidation protects cached snapshots and already-started
warm fetches. A retained retired CoreContext beginning a new fetch is not
proved to be denied. This does not implement a cache TTL, distributed
invalidation, arbitrary embedder-scope reclamation, or a storage grant.
It does not claim fresh full-workspace/all-feature, MSRV, coverage, hosted
acceptance or completion of the cloud-storage migration.
