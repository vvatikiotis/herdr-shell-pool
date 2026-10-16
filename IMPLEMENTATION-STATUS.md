# Herdr Shell Pool: Implementation Status

[SPEC.md](SPEC.md) describes existing functionality only and is the contract for characterization tests. [PLANNED.md](PLANNED.md) owns desired changes and future features; those are not requirements for the current tests.

## Migration

- [x] Rust is the active source implementation under `src/`.
- [x] Rust integration tests invoke the executable rather than private helper functions.
- [x] All Go source, tests, and module files are archived unchanged under `legacy/`.
- [x] At migration, the same 31 Rust integration tests passed against the Rust executable and a separately built Go reference.
- [x] Build and install the release binary as `herdr-shell-pool-bin` inside this project and update the three Herdr shortcuts in the parent configuration to use it. At that stage, all 31 integration tests passed against the installed binary. Live smoke-test results are recorded below.

## Workspace creation and sessionizer integration

- [x] Add `new-workspace --cwd <absolute-path> --label <name>`, moving an eligible warm shell directly into a new workspace with a matching initial-tab name, destination context, directory, focus, and background replenishment.
- [x] Verify the workspace action through the executable and controlled Herdr API, without requiring source-pane identifiers. Reject an unusable directory before any creation attempt.
- [x] Integrate sessionizer's unopened-directory selection with the sibling shell-pool executable. Its executable test verifies delegation, selected-directory output, and history. Existing sessionizer tests continue to pass.
- [x] Build and install both updated release binaries at `herdr-shell-pool-bin` and the sibling sessionizer's `herdr-sessionizer`, replacing each through a temporary file and rename. Installed files match their release outputs. Workspace creation itself needed no new key binding or plugin hook.
- [x] Verify Ctrl+d recovery with real isolated Herdr 0.9.5/macOS sessions. Both ordinary creation and the shell-pool `new-workspace` action reproduced native fallback to the pool; the updated sessionizer hook returned focus to the original user workspace. The retained opt-in regression uses ordinary creation and a reserved-label stand-in, so no shell-pool installation is required.
- [x] Install the updated sessionizer binary and relink its manifest to register `pane.exited` → `after-pane-exit`. Other plugin registrations were unchanged; no server restart was needed.
- [ ] Verify the full live sessionizer picker flow: selection, preserved pool terminal identity, selected directory, workspace/tab labels, history, and focus. The isolated exit checks do not establish all of these.
- Workspace recovery and notifications remain in `PLANNED.md`. The source stops on tool failure without a creation retry; a missing tool retains ordinary creation without the planned notification.

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
- [x] Add a deterministic two-request overlap test: A's move response is held pending while B must complete silent cold-tab recovery. Verify one original warm terminal moved, one distinct cold destination, two original terminals still pooled, correct setup/focus requests, and both detached refills exiting. The server remains responsive while A is paused.
- [x] Check concurrency-test sensitivity: a disposable build with pool locking disabled fails with a duplicate warm-shell move while A is pending. Production source and the installed binary were not modified.
- [ ] Deferred optional coverage: broader concurrent-request stress testing remains unverified. The controlled new-tab overlap does not cover every action combination or scheduling pattern.
- [x] Verify foreground completion while replenishment is deliberately blocked: hold the refill worker's `tab.create` response, confirm the new-tab CLI exits successfully while its refill worker and launcher remain alive, then release the response and verify replenishment finishes with two pooled shells. A disposable synchronous-refill build fails the test because the foreground CLI cannot exit before release.
- [ ] Deferred optional coverage: explicit elapsed-time tests for API connect/read timeouts. The blocked-refill gate proves foreground/refill independence, not a general latency bound.
- [ ] Deferred optional coverage: broader malformed-response, worker-record cleanup, and partial-failure cases. Passing tests are not exhaustive proof of all possible API replies or operating-system failures.

## Checks

- Current Rust integration tests: 35 passing, including workspace creation and unusable-directory rejection. The 33-test pre-workspace baseline passed against both implementations; new workspace behavior and updated usage text are not supported by archived Go.
- Sessionizer Rust tests: 20 passing in debug and release modes, including delegated creation, standalone operation without shell pool, history exclusion, and pane-exit focus recovery.
- Sessionizer's opt-in real-Herdr Ctrl+d regression: passing on Herdr 0.9.5/macOS, including three consecutive runs. The original warm-workspace reproduction also passed after the fix.
- Focused overlap test: 20 consecutive passing runs; also passes against the archived Go reference. A no-lock disposable build fails as expected.
- Focused blocked-refill test: 20 consecutive passing runs; also passes against the archived Go reference. A synchronous-refill disposable build fails as expected.
- Archived Go helper tests: passing.
- Rust formatting and Clippy with warnings denied: passing.
- Rust release build and all 35 release-mode tests: passing.
- Historical post-move run of the original 31 Rust tests against a newly built archived Go reference: passed.
- All seven archived Go source/module files match their original Git versions byte-for-byte.
- No Go source files remain outside `legacy/` in this project.
- Diff whitespace check: passing.

## Existing limitations, not new requirements

Eligibility checks recognize only `zsh`, `bash`, and `sh` and do not prove the stronger glossary definition of a ready warm shell. Status counts idle panes without excluding agent-designated panes. Filling can keep adding shells if none become eligible. Locks and worker records are shared across Herdr sessions. Cold-shell recovery and background failures remain silent, and uncertain moves are not reconciled.

See the planned-functionality document for proposed changes; do not make characterization tests require them.
