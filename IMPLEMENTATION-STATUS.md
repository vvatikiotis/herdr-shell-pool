# Herdr Shell Pool: Implementation Status

[SPEC.md](SPEC.md) describes existing functionality only and is the contract for characterization tests. [PLANNED.md](PLANNED.md) owns desired changes and future features; those are not requirements for the current tests.

## Migration

- [x] Rust is the active source implementation under `src/`.
- [x] Rust integration tests invoke the executable rather than private helper functions.
- [x] All Go source, tests, and module files are archived unchanged under `legacy/`.
- [x] The same 31 Rust integration tests pass against the Rust executable and a separately built Go reference.
- [x] Build and install the release binary as `herdr-shell-pool-bin` inside this project and update the three Herdr shortcuts in the parent configuration to use it. All 31 integration tests pass against the installed binary. Live smoke-test results are recorded below.

## Verified through controlled external boundaries

These tests use isolated sockets, state directories, API replies, and a fake `herdr` executable. They verify protocol behavior and submitted shell input, not actual Herdr rendering or shell startup.

- [x] Missing and unknown CLI actions fail with usage information; internal workers reject invalid tokens.
- [x] Empty-pool status and idle-process counting, including the existing difference between status and selection for agent-designated panes.
- [x] Warm-shell moves for new tabs, left splits, and down splits.
- [x] Source identifier fallback; foreground, recorded, and home directory selection; quoting paths containing spaces and apostrophes.
- [x] Submitted destination identifiers and directory setup before requesting focus.
- [x] Left split ratio and swap requests, and default down-split destination parameters.
- [x] Exclusion of agent-designated, busy, empty-process, and failed-process-query candidates.
- [x] Existing silent cold-shell recovery for an absent pool, a busy lock, and an unchanged move.
- [x] Existing errors for an absent source, move errors, unsuccessful swaps, shell-input command failure, output-wait failure, and focus failure, without speculative recovery.
- [x] Explicit filling of an absent or depleted pool without requesting focus; no additional shells when two are eligible; successful exit when the pool lock is busy.
- [x] Setup marker is absent from literal shell input and its submitted marker command decodes to the value awaited through the API.
- [x] Explicit socket selection and fallback resolution through home, XDG configuration, and named sessions.
- [x] Malformed API result and error payloads fail rather than silently succeeding.
- [x] Worker retirement signals a controlled older-build test process only when identity matches; same-build, newer, wrong-role, wrong-start-time, and command-mismatch records do not cause signalling.

## Verification limits and remaining coverage

- [x] Live smoke test: explicit `fill` succeeded and `status` reported `2/2 ready`. The user confirmed new-tab, left split, and down split opened in the expected location with keyboard focus and the same directory; left width was approximately 61%. A subsequent `status` again reported `2/2 ready`.
- [x] Confirm a live new-tab action reused a pool terminal: `term_65df6d7098621a9` moved from pool pane `w8R:pP` to destination pane `w8P:p13` in tab `w8P:t8`, with the working directory updated. Pane IDs change across workspaces; terminal identity established reuse for this action.
- [x] Confirm a live left-split action reused a pool terminal: `term_65df6d710036faa` moved from `w8R:pQ` to `w8P:p14` in tab `w8P:t1`, with the working directory updated.
- [x] Confirm a live down-split action reused a pool terminal: `term_65df6dfa3480cab` moved from `w8R:pR` to `w8P:p15` in tab `w8P:t1`, with the working directory updated. Subsequent status reported `2/2 ready`.
- [ ] Verify shell startup and terminal rendering more thoroughly and exact split geometry. Identity checks prove reuse for the observed new-tab, left-split, and down-split actions, not every request or readiness beyond the current checks.
- [ ] Verify macOS and Linux behavior on both platforms. Migration checks were run on macOS only.
- [ ] Add a direct concurrent-request stress test. Busy-lock behavior is covered, but simultaneous requests have not been stress-tested.
- [ ] Add explicit elapsed-time tests for connect/read timeouts and for foreground completion while background replenishment is deliberately blocked. Current tests exercise background replenishment but do not establish a latency bound.
- [ ] Add broader malformed-response, worker-record cleanup, and partial-failure cases as needed. Passing tests are not exhaustive proof of all possible API replies or operating-system failures.

## Checks

- Rust integration tests: 31 passing against each implementation.
- Archived Go helper tests: passing.
- Rust formatting and Clippy with warnings denied: passing.
- Rust release build and all 31 release-mode tests: passing.
- Final post-move run of all 31 Rust tests against a newly built archived Go reference: passing.
- All seven archived Go source/module files match their original Git versions byte-for-byte.
- No Go source files remain outside `legacy/` in this project.
- Diff whitespace check: passing.

## Existing limitations, not new requirements

Eligibility checks recognize only `zsh`, `bash`, and `sh` and do not prove the stronger glossary definition of a ready warm shell. Status counts idle panes without excluding agent-designated panes. Filling can keep adding shells if none become eligible. Locks and worker records are shared across Herdr sessions. Cold-shell recovery and background failures remain silent, and uncertain moves are not reconciled.

See the planned-functionality document for proposed changes; do not make characterization tests require them.
