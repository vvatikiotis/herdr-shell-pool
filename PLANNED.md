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

## Next feature to implement

Warm shells for newly created workspaces. Until implemented, Herdr handles workspace creation normally.

Before implementation, agree on the entry point, directory selection, destination setup, recovery behavior, and interaction with Herdr's initial workspace pane.

## Implementation and testing approach

Work one behavior at a time. Agree on any additional testing boundary, write a failing behavior test, implement the smallest change, and verify before proceeding. Test through CLI actions where applicable; startup integration and notifications may require additional agreed boundaries.

These planned requirements have not been implemented or verified by writing this document. Do not publish them to an issue tracker yet.
