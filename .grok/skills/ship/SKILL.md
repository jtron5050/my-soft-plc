---
name: ship
description: >
  Implement a single change on a feature branch, open a draft GitHub PR,
  and run a reviewer agent that posts a PENDING review. Use when the user
  runs /ship, says "ship this", "open a PR for this", or wants one change
  implemented and reviewed on GitHub. Do not use for multi-PR stacks
  (/design + /execute-plan), addressing existing review comments
  (/pr-babysit), or local-only review (/implement, /review --local).
when-to-use: "/ship, ship this, open a PR for this, implement and PR"
argument-hint: "<what to implement>"
metadata:
  short-description: "Implement, draft PR, request review"
---

# /ship — implement, draft PR, request review

You are the implementer for **one** GitHub PR. After the PR exists, follow
the bundled `/review` skill in PR mode. You do not submit the review,
babysit, or merge.

Repo policy (branch prefixes, verify commands, commit style, no-merge) lives
in `AGENTS.md`. Read it. Do not copy those lists here.

## Arguments

`<what to implement>` is the change. If omitted:

- Uncommitted changes exist → ship those. Infer a PR title from the diff.
- Working tree clean → ask what to implement and stop.

If the request is a multi-PR stack, a design doc, or `/execute-plan`, stop
and point the user at `/design` + `/execute-plan --no-graphite --auto-pr`.

## Steps

### 1. Preconditions

- `gh auth status` — if it fails, stop (`gh auth login`).
- Confirm this is a git checkout with `origin`.
- `git fetch origin`
- If the working tree has changes that are **not** part of this request,
  stop and list them. Do not mix unrelated work into the PR.

### 2. Branch

Choose a prefix from `AGENTS.md` (`feature/`, `fix/`, `chore/`):

- `fix/` — bug, broken behavior, regression
- `chore/` — docs, skills, CI, config, deps
- `feature/` — everything else

Slug: lowercase, hyphens, from the request, ≤50 characters.

- If HEAD is the default branch (`main`):
  `git checkout -b <prefix>/<slug> origin/main`. Never commit on `main`.
- If HEAD already starts with `feature/`, `fix/`, or `chore/`: keep it
  only when this request is already that branch's work — shipping these
  uncommitted changes, shipping commits already on the branch, or the
  user said to add to this PR. If the branch is already ahead of
  `origin/main` or already has a PR **and** the argument is a new
  implementation, create `<prefix>/<slug>` from `origin/main` instead.
  Do not pile an unrelated change onto an existing branch or PR.
- If HEAD is detached, or the branch name is anything else (not `main`,
  not a prefix branch): create `<prefix>/<slug>` from `origin/main` and
  cherry-pick or apply the intended work there.

### 3. Implement

Make the change on this branch. Do not spawn `/implement` (that is a local
review loop). Do not edit `main`.

If inspection shows there is nothing to change, stop. Do not open an empty PR.

### 4. Verify

Run the Build & Test commands from `AGENTS.md`. Skip commands that do not
apply (for example skip `cargo` when no Rust files changed; skip
`check-rt-deps.sh` when no RT-path crate changed). If a required check
fails, fix it or stop. Do not push a red tree.

### 5. Commit

- Stage only files that belong to this request.
- Commit with an imperative message per `AGENTS.md`.
- If there is nothing to commit and the branch is already ahead of
  `origin/main` with the intended work, continue.
- If there is nothing to commit and the branch matches `origin/main`, stop.

### 6. Push

```bash
git push -u origin HEAD
```

Never `git push --force`. Use `git push --force-with-lease` only if this
branch already has a remote and you rebased onto `origin/main` in this run.

### 7. Draft PR

```bash
gh pr view --json number,url,isDraft
```

- PR exists for this branch → reuse it. Do not convert a ready PR back to draft.
- No PR → `gh pr create --draft --fill`

Record `PR_NUMBER` and `PR_URL`.

### 8. Review

Query the authenticated user's PENDING review on this PR
(`GET /repos/{owner}/{repo}/pulls/{n}/reviews` — PENDING rows are only
returned for the current user):

- Same `commit_id` as HEAD → skip `/review`.
- Different `commit_id` → `DELETE` that pending review
  (`DELETE /repos/{owner}/{repo}/pulls/{n}/reviews/{id}`), then run
  `/review`.
- No PENDING review for the current user → run `/review`.

Do not treat another user's PENDING review as a skip.

When not skipping, load the bundled `/review` skill and execute it as
`/review --pr <PR_NUMBER>`. Do not invent a parallel review path. Do not
pass `capability_mode: read-only` to the reviewer (that skill explains why).

### 9. Hand off

Print exactly this shape, filled in:

```
Shipped: <PR_URL>
Review is PENDING (visible only to you until submitted).
Submit: <PR_URL>/files  →  Finish review → Submit review
Then: /pr-babysit add <PR_NUMBER>
```

If `/review` found no issues and posted nothing, say so and still print
the PR URL and the babysit command.

Do not run `/pr-babysit`. Do not `/loop`. Do not merge. Leave HEAD on the
feature branch.

## Rules

- Single PR only. Stacks go to `/execute-plan`.
- You implement; `/review` reviews. Do not self-review instead of `/review`.
- Never merge. Never submit the GitHub review (`event` stays unset).
- Never commit or push `main`.
- Policy details stay in `AGENTS.md`.
