# Herdr Shell Pool: Existing Functionality

This spec describes only behavior implemented in the current code. It is the basis for tests of that behavior, not a claim that those tests already exist. Planned changes are in [PLANNED.md](PLANNED.md); verification tracking is in [IMPLEMENTATION-STATUS.md](IMPLEMENTATION-STATUS.md). Shared terms are in [GLOSSARY.md](GLOSSARY.md).

## Problem Statement

Starting a shell for every new tab or split can delay work while shell startup configuration runs. A Herdr user wants new tabs and splits to become usable quickly and retain the directory of the pane they were using.

## Solution

Hold shells in a dedicated pool workspace. For a new tab or left/down split, select an eligible warm shell, move its pane into the requested location, update its directory and location identifiers, and focus it. Start background replenishment after a successful request.

When the pool lock is busy, no eligible shell exists, or Herdr reports an unchanged move, open the requested destination with a cold shell instead. This recovery is currently silent.

## User Stories

1. As a Herdr user, I want to open a new tab with a warm shell, so that I can avoid shell startup delay.
2. As a Herdr user, I want to open a left split with a warm shell, so that I can work alongside my current pane.
3. As a Herdr user, I want to open a down split with a warm shell, so that I can work below my current pane.
4. As a Herdr user, I want the new left pane to occupy 61% of the width, so that it has the intended working space.
5. As a Herdr user, I want down splits to use Herdr's default size, so that their layout follows normal Herdr behavior.
6. As a Herdr user, I want the destination to receive keyboard focus, so that I can immediately use it.
7. As a Herdr user, I want the destination shell to use my current pane's directory, so that I can continue in the same project.
8. As a Herdr user, I want a moved shell to identify its new workspace, tab, and pane, so that commands inside it act on the correct location.
9. As a Herdr user, I want filling to target two eligible pool shells, so that successive requests can reuse existing shells.
10. As a Herdr user, I want replenishment to run in the background, so that destination opening does not wait for the pool to refill.
11. As a Herdr user, I want panes identified as running another program or assigned to an agent excluded from warm-shell selection, so that those panes are not moved for my request.
12. As a Herdr user, I want cold-shell recovery for certain pool failures, so that my requested tab or split can still open.
13. As a Herdr user, I want an explicit fill action, so that I can prepare the pool.
14. As a Herdr user, I want a status action, so that I can inspect the count of idle pool shells.
15. As a Herdr user, I want actions to use my selected Herdr session, so that they affect the right environment.
16. As a Herdr user, I want pool mutations coordinated, so that concurrent requests do not select the same shell.

## Implementation Decisions

### Actions and source context

- CLI arguments select `new-tab`, `split-left`, `split-down`, `fill`, and `status`. An internal `fill-worker` action requires a worker token. Unknown actions fail with usage information.
- Configured shortcuts invoke `new-tab` with `prefix+c`, `split-left` with `prefix+"`, and `split-down` with `prefix+%`.
- The source pane is selected from `HERDR_ACTIVE_PANE_ID`, falling back to `HERDR_PANE_ID`. Missing identifiers or a source absent from the session snapshot cause an error.
- The destination directory is the source pane's foreground directory, falling back to its recorded directory and then the user's home directory.
- A new tab belongs to the source workspace. A split belongs to the source tab alongside the source pane.

### Pool and selection

- The pool workspace has the reserved label `__herdr_shell_pool__`. Filling creates it if absent without requesting focus. New pool shells start in the user's home directory.
- Filling targets at least two eligible shells. It does not shrink an oversized pool or remove ineligible panes.
- Warm-shell selection and filling require no agent designation and nonempty foreground process information containing only `zsh`, `bash`, or `sh`. Process names are compared case-insensitively by basename after removing a leading login-shell hyphen. Failed process queries reject the pane.
- Setup sends a command and waits for a unique output marker, with a three-second output timeout. The marker is encoded in the input so that echoed input does not contain the literal marker. Setup temporarily disables terminal echo and clears the terminal before restoring echo and printing the marker.
- Selection does not retain proof of completed startup setup. Eligibility is therefore not a guarantee of the glossary's stronger definition of a ready warm shell.
- `status` prints `<count>/2 ready`. Its count uses the idle-shell process check but, unlike selection and filling, does not exclude agent-designated panes.

### Moving and recovery

- Move the pane containing the warm shell without restarting its shell process. Request the move without focus.
- Left placement uses a right split with ratio 0.61 followed by a pane swap. Down placement uses Herdr's default ratio.
- After a successful warm move, export `HERDR_PANE_ID`, `HERDR_ACTIVE_PANE_ID`, `HERDR_WORKSPACE_ID`, and `HERDR_TAB_ID` for the destination, change directory, and wait for the setup marker before requesting focus.
- A nonblocking filesystem lock coordinates pool mutation. A busy lock causes a destination request to use cold-shell recovery. A fill worker unable to obtain the lock exits without filling.
- Cold-shell recovery also occurs when no eligible candidate exists or Herdr reports that the move made no change. Create the destination directly with the selected directory and the same layout and focus behavior. No warm-shell failure notification is emitted.
- Move API errors, setup failures, swap failures, and focus failures return errors rather than automatically creating another shell. There is no uncertain-move reconciliation or rollback of partially completed operations.
- After a successful request, launch detached replenishment. Opening the destination does not wait for that replenishment. Launch failures are silent and detached output is discarded.

### Runtime and worker coordination

- `fill` launches a registered worker. Workers identify their executable generation and register a token, process identity, and creation time.
- A newer worker can signal an older worker from another build only after matching its recorded process start time and exact executable, action, and token. Same-build workers are not retired by this mechanism.
- `HERDR_SOCKET_PATH` overrides socket selection. Otherwise resolve the socket under Herdr's configuration directory, including the named session when `HERDR_SESSION` is provided. Configuration defaults to the user's configuration directory when `XDG_CONFIG_HOME` is absent.
- Store locks and worker records under `XDG_STATE_HOME`, falling back to the user's local state directory. These paths are shared across Herdr sessions rather than separated by session.
- Support macOS and Linux through Herdr's socket API, the `herdr` executable for shell input, and operating-system process and locking facilities.
- CLI failures print a prefixed error to standard error and exit unsuccessfully.

## Testing Decisions

- Use this spec to characterize existing behavior, including current recovery conditions and error behavior. Do not import desired behavior from the planned-functionality document into these tests.
- Test through executable CLI actions, using controlled Herdr socket and shell/process boundaries to inspect destination and pool state, output, errors, and process behavior.
- Prefer observable results over private helper calls or internal call counts. Use independently specified expected outcomes rather than repeating implementation calculations.
- Cover each destination action; source and directory fallback; context updates; layout and focus; selection exclusions; fill and status; lock contention; existing cold-shell recovery; returned errors; and background replenishment.
- For overlapping new-tab requests, hold the first move response pending while the second request completes. The second request must use existing busy-lock cold-shell recovery; only the first may move the selected warm shell. Keep the fake API responsive during the pause so the test cannot pass through server-side serialization.
- To verify background replenishment does not delay foreground completion, hold the refill worker's new-shell API response and require the destination-opening CLI to exit successfully before releasing it. Confirm refill processes remain alive during the pause and finish after release; a bounded wait is a hang safeguard, not a performance guarantee.
- Controlled boundaries can check protocol behavior without operating on the user's live session. Real terminal rendering, shell startup, and Herdr layout require integration checks.
- Archived Go tests provide helper-level prior art for quoting, markers, destination construction, pane filtering, and worker-retirement eligibility. Rust integration tests characterize executable behavior against both the Rust implementation and the archived Go reference.
- Track current test results and remaining verification limits separately in the implementation-status checklist.

## Out of Scope

- Any behavior not implemented in the current code. Desired changes and future features belong in the planned-functionality document.
- Sessionizer behavior, Vim navigation, unrelated Herdr settings, and changes to shell configuration or Herdr itself.
- Guarantees of zero delay, unlimited capacity, or success despite unavailable external services.

## Further Notes

This is a description of existing functionality, including its limitations, not an endorsement of every current failure mode. Changes to those behaviors require a separate implementation task and corresponding tests.
