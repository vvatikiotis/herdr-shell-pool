# Herdr Shell Pool: Planned Functionality

These are agreed changes, not existing behavior and not requirements for tests characterizing the current implementation. [SPEC.md](SPEC.md) describes existing functionality only.

## Improvements to tabs and splits

- Prepare the shell pool automatically when Herdr starts.
- Aim to keep two ready warm shells available. Readiness means startup setup has finished and the shell is idle at a prompt; current eligibility checks do not establish this guarantee.
- Treat inability to supply a warm shell as a warm-shell failure, not a normal alternative path.
- Recover by opening the requested destination with a cold shell and show a descriptive Herdr notification. State that warm-shell reuse failed and that a new shell was started, including the reason when known.
- After an uncertain move response, inspect session state before creating a cold shell. If the warm shell arrived, do not create another destination.
- Show descriptive Herdr notifications for preparation and replenishment failures, including background failures.
- Preserve existing directory selection, destination identifiers, layout, focus, and background replenishment behavior while making these changes.

## Decisions required before implementation

- Choose the startup integration, readiness mechanism, and notification delivery mechanism.
- Decide how status should report the stronger readiness definition.
- Decide how to handle unavailable inspection and partially completed moves, setup, or focus. Arrival alone does not establish successful destination setup.
- Define recovery beyond the current cold-shell recovery conditions without risking duplicate destinations.
- Choose retry or notification deduplication behavior only if needed for the agreed requirements.

## Remaining workspace integration work

The successful sessionizer-to-pool creation path is now described in [SPEC.md](SPEC.md), not as a planned feature here. Deployment and live verification remain tracked in [IMPLEMENTATION-STATUS.md](IMPLEMENTATION-STATUS.md).

- When the shell-pool tool is unavailable, retain sessionizer's ordinary-creation path but add a descriptive Herdr notification explaining that a cold shell was started. The existing silent path does not yet meet that requirement.
- Add descriptive notifications for failures when the tool runs. Sessionizer already stops without retrying creation on a nonzero tool exit, but this is currently a terminal error, not a Herdr notification.
- Cold-shell recovery is allowed only when no destination was created. If the outcome cannot be established, stop and report the error rather than risk creating a duplicate workspace.
- Define how to establish the outcome after failed or partially completed moves and how to recover when a warm shell is unavailable but no destination was created. A nonzero exit alone is not proof that creation did not occur.
- Choose notification delivery before implementing these recovery changes.

## Implementation and testing approach

Work one behavior at a time. Agree on any additional testing boundary, write a failing behavior test, implement the smallest change, and verify before proceeding. Test through CLI actions where applicable; startup integration and notifications may require additional agreed boundaries.

The successful workspace-creation path is implemented. Workspace cold-shell recovery and notifications remain planned, along with the tab/split improvements above. Sessionizer's history exclusion and Ctrl+d focus recovery are existing functionality documented in its own spec, not additional work in this plan. Verification gaps in the implementation-status checklist do not, by themselves, authorize more test work. Do not publish to an issue tracker yet.
