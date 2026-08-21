# Soft PLC — Agent Rules

Greenfield Rust soft PLC. Architecture lives in `docs/architecture.md`. Workspace crates are under `crates/*`.

## Version Control

All work ships through GitHub PRs. Do not treat local commits on `main` as done.

- Never commit to `main`. Never push to `main`.
- Implement on a `feature/`, `fix/`, or `chore/` branch created from `origin/main`.
- After implementation, open a **draft** PR: `gh pr create --draft --fill`.
- Run `/review --pr <n>` so a reviewer agent posts a PENDING GitHub review.
- The human submits that review in the GitHub UI (Files tab → Finish review → Submit review). Until then, comments are not visible to others and `/pr-babysit` will not see them.
- Address submitted review comments and CI failures with `/pr-babysit add <n>` (or “fix review comments on PR N”).
- Agents never merge. Merging is a human decision. Prefer squash-merge.
- Write commit messages in the style of this repo: imperative, specific, optional `(PR-NN)` suffix for plan items.

## Build & Test

Rust **1.85** (`rust-toolchain.toml`). Before committing or pushing:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

When changing RT-path crates (`plc-scan`, `plc-vm`, `plc-io`, `plc-types`, `plc-fb-primitives`, `plc-retain`, `plc-ir`):

```bash
bash scripts/check-rt-deps.sh
```

Do not pull `tokio` or network crates onto the RT path.

## Architecture

- Follow `docs/architecture.md`. Current landed work is PR-01–PR-10. Next planned item is PR-11 (authn/authz primitives).
- Do not invent crates or public APIs that contradict the architecture plan without an explicit design change.
