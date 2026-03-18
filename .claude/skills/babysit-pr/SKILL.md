---
description: Use this skill when the user asks to monitor a PR, watch CI, auto-merge a PR, or babysit a pull request in evot-web. Trigger phrases: "babysit pr", "watch ci", "monitor pr", "auto-merge", "/babysit-pr".
---

# babysit-pr — Monitor PR & Auto-merge

Monitors a GitHub PR's CI checks, resolves issues, and enables auto-merge when green.

## Workflow

This skill also applies to monitoring the main branch directly (no PR). When the user says "main branch", monitor the latest CI run on main instead of a PR.

### For a PR:
1. Get the PR number (ask if not provided).
2. Get the latest run ID for the PR: `gh run list --repo <repo> --branch <branch> --limit 1 --json databaseId --jq '.[0].databaseId'`
3. Poll using the loop below.
4. On failure → fetch logs immediately and analyze root cause.
5. On success → enable auto-merge with `gh pr merge <number> --auto --squash`.

### For main branch (no PR):
1. Get the latest run ID: `gh run list --repo <repo> --branch main --limit 1 --json databaseId --jq '.[0].databaseId'`
2. Poll using the loop below.
3. On failure → fetch logs immediately and analyze root cause.
4. On success → report green.

### Polling loop

Poll every 30 seconds. On ANY job failure, stop immediately — do NOT wait for the run to complete.

```bash
RUN_ID=<run_id>
REPO=<owner/repo>

while true; do
  DATA=$(GH_HOST=github.com gh run view $RUN_ID --repo $REPO --json jobs \
    --jq '[.jobs[] | {name:.name, status:.status, conclusion:.conclusion}]')

  # Fail-fast: stop as soon as any job has conclusion=failure
  FAILED=$(echo "$DATA" | python3 -c "
import sys, json
jobs = json.load(sys.stdin)
failed = [j['name'] for j in jobs if j['conclusion'] == 'failure']
print('\n'.join(failed))
")
  if [ -n "$FAILED" ]; then
    echo "FAILED: $FAILED"
    break
  fi

  # All done and no failures = success
  OVERALL=$(GH_HOST=github.com gh run view $RUN_ID --repo $REPO --json status --jq '.status')
  if [ "$OVERALL" = "completed" ]; then
    echo "ALL PASSED"
    break
  fi

  # Print current status snapshot
  echo "$(date '+%H:%M:%S') in progress..."
  echo "$DATA" | python3 -c "
import sys, json
for j in json.load(sys.stdin):
    print(f'  {j[\"status\"]:12} {j[\"conclusion\"] or \"-\":10} {j[\"name\"]}')
"
  sleep 30
done
```

### Fetching logs after failure

When a job fails, use job-level log commands — these work as soon as the job is done, even if the overall run is still in_progress:

```bash
# Get the failed job's ID
JOB_ID=$(GH_HOST=github.com gh run view $RUN_ID --repo $REPO --json jobs \
  --jq '.jobs[] | select(.conclusion=="failure") | .databaseId')

# --log-failed at job level works immediately after the job finishes
GH_HOST=github.com gh run view --job $JOB_ID --repo $REPO --log-failed 2>&1 | tail -60
```

After getting the logs, analyze the root cause and report findings to the user.

## Commands

Check CI status:
```bash
GH_HOST=github.com gh run list --repo <owner/repo> --branch <branch> --limit 5
```

Enable auto-merge (squash):
```bash
GH_HOST=github.com gh pr merge <number> --auto --squash --repo <owner/repo>
```

Check for merge conflicts:
```bash
GH_HOST=github.com gh pr view <number> --repo <owner/repo> --json mergeable,mergeStateStatus
```

Rebase onto main to resolve conflicts:
```bash
git fetch origin main
gh pr checkout <number>
git rebase origin/main
git push --force-with-lease
```

## Gotchas

- Always prefix `gh` commands with `GH_HOST=github.com` in this repo — the remote is SSH-based and gh needs the hint.
- `--log-failed` only works after the run is fully `completed`. If the run is still `in_progress`, use `--log` and grep for errors.
- `conclusion` is an empty string `""` while a job is running — only becomes `"failure"` or `"success"` when the job finishes. The poll loop above handles this correctly.
- Do NOT use `gh run watch` — it blocks until the entire run completes, defeating fail-fast.
- Do NOT use `gh pr merge --merge` (creates a merge commit). Always use `--squash`.
- Before enabling auto-merge, confirm the PR branch is up to date with `main`.
- `--force-with-lease` is safer than `--force` when rebasing.
