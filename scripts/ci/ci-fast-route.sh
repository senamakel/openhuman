#!/usr/bin/env bash
# Is this PR commit being run on the EX63 by CI Fast (ci-fast.yml)?
#
# Writes outsider=true|false and source=ci-fast|fallback to $GITHUB_OUTPUT: false when ci-fast.yml routed
# the commit to the EX63 (an org member's PR), true otherwise. Used by
# ci-fast-hosted.yml (run the hosted lanes for outsiders and drafts).
# ci-gate.yml then requires whichever of them ran.
#
# Env: GH_TOKEN, REPO, HEAD_SHA, AUTHOR, ACTOR, ASSOCIATION. Optional: DRAFT
# (the PR is a draft) and SINCE (the PR's updated_at for this event).
#
# Draft PRs never run on the EX63 (ci-fast.yml routes them to GitHub-hosted
# runners), so a draft is an outsider here without asking ci-fast.yml.
set -euo pipefail
# ci-fast.yml decides who is an org member: it runs from main with
# the CI_MEMBERSHIP_TOKEN secret, so it also sees private members.
# Fork `pull_request` runs get no secrets and see those members only
# as COLLABORATOR, so deciding here as well would run the PR twice.
# Instead, find ci-fast.yml's run for this head commit and read its
# outcome: EX63 lanes skipped means outsider, anything else means
# the EX63 is handling it.
decide_from_ci_fast() {
  local run_id="" lanes="" since=""
  # SINCE is spliced into the jq filter, so accept only an ISO timestamp.
  if [[ "${SINCE:-}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
    since="${SINCE}"
  fi
  for _ in $(seq 1 36); do
    # Only a run started by this event: marking a draft ready re-runs
    # ci-fast.yml on the same head commit, and the draft's earlier run (EX63
    # skipped) must not answer for it.
    run_id="$(gh api "repos/${REPO}/actions/workflows/ci-fast.yml/runs?head_sha=${HEAD_SHA}&per_page=5" \
      --jq '[.workflow_runs[] | select(.event == "pull_request_target" and .created_at >= "'"${since}"'")] | sort_by(.created_at) | last | .id // empty' 2>/dev/null || true)"
    if [ -n "${run_id}" ]; then
      # A member run materialises "Lanes / CI Fast (EX63)"; an
      # outsider run leaves the skipped call job "Lanes" alone.
      lanes="$(gh api "repos/${REPO}/actions/runs/${run_id}/jobs?per_page=50" \
        --jq 'if any(.jobs[]; .name == "Lanes / CI Fast (EX63)") then "member"
              elif any(.jobs[]; .name == "Lanes" and .conclusion == "skipped") then "outsider"
              else "" end' 2>/dev/null || true)"
      route="$(gh api "repos/${REPO}/actions/runs/${run_id}/jobs?per_page=50" \
        --jq '[.jobs[] | select(.name | startswith("Route"))][0].conclusion // empty' 2>/dev/null || true)"
      if [ "${route}" = "success" ]; then
        case "${lanes}" in
          outsider) echo "true ci-fast run ${run_id}: EX63 lanes skipped"; return 0 ;;
          member) echo "false ci-fast run ${run_id}: EX63 lanes running"; return 0 ;;
          *) ;;  # jobs not materialised yet
        esac
      fi
    fi
    sleep 5
  done
  return 1
}
if [ "${DRAFT:-false}" = "true" ]; then
  outsider=true
  reason="draft PR: GitHub-hosted runners"
elif verdict="$(decide_from_ci_fast)"; then
  outsider="${verdict%% *}"
  reason="${verdict#* }"
else
  # No ci-fast.yml decision within 3 minutes: fall back to the event's
  # author association (private members may then run here too).
  outsider=true
  if { [ "${ASSOCIATION}" = "OWNER" ] || [ "${ASSOCIATION}" = "MEMBER" ]; } && [ "${ACTOR}" = "${AUTHOR}" ]; then
    outsider=false
  fi
  reason="fallback: no ci-fast.yml decision, association=${ASSOCIATION}"
fi
echo "[ci][route] author=${AUTHOR} actor=${ACTOR} outsider=${outsider} (${reason})"
echo "outsider=${outsider}" >> "$GITHUB_OUTPUT"
# Whether ci-fast.yml's own run decided (ci-fast) or the association fallback
# did (fallback), or the PR is a draft (draft).
case "${reason}" in fallback*) echo "source=fallback" ;; draft*) echo "source=draft" ;; *) echo "source=ci-fast" ;; esac >> "$GITHUB_OUTPUT"
