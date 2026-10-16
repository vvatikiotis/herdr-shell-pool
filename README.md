# Herdr Shell Pool

Rust implementation of the existing behavior described in [SPEC.md](SPEC.md). The original Go source, helper tests, and module files are preserved in `legacy/` as a reference. Desired changes are separate in [PLANNED.md](PLANNED.md).

## Build and run

Requires Rust with edition 2024 support, macOS or Linux, and Herdr. Shell setup invokes the `herdr` executable from `PATH`.

```sh
cargo build --release &&
cp target/release/herdr-shell-pool ./herdr-shell-pool-bin.new &&
chmod 755 ./herdr-shell-pool-bin.new &&
mv -f ./herdr-shell-pool-bin.new ./herdr-shell-pool-bin
./herdr-shell-pool-bin status
```

Actions: `fill`, `status`, `new-tab`, `split-left`, `split-down`, and `new-workspace --cwd <absolute-path> --label <name>`.

The new-workspace action is the first workspace-support slice: it moves a warm pane directly into a new workspace, names its initial tab, updates shell context, focuses it, and replenishes. It does not yet recover with a cold shell or deliver failure notifications.

Herdr's new-tab and left/down split bindings point to `herdr-shell-pool-bin` in this directory. Rebuild and install it with the commands above after source changes. The binary is ignored by Git.

The temporary-file-and-rename sequence avoids overwriting an executable that may be running. Automatic pool preparation at Herdr startup is not implemented.

## Sessionizer integration

Sessionizer looks for this project's `herdr-shell-pool-bin` beside its own plugin directory. Selecting an unopened project delegates creation to `new-workspace`; existing-workspace selection, Ctrl+f, and selected-directory output are unchanged. Workspace creation itself needs no new plugin hook or key binding.

Sessionizer separately excludes the pool workspace from visit history and jump-back. Its `pane.exited` hook corrects Herdr's Ctrl+d fallback when closing a workspace leaves the pool focused. That fix requires the updated sessionizer manifest to be relinked; the shell-pool tool does not manage close-focus behavior. See the [sessionizer README](../herdr-sessionizer/README.md).

Build and install compatible versions of both tools together. Both updated binaries are installed, and sessionizer's pane-exit hook is registered. Ctrl+d recovery passed real isolated Herdr checks, including a workspace created by the shell-pool action. Full interactive picker-to-warm-workspace verification remains pending.

Sessionizer works without shell pool installed. A missing executable preserves ordinary workspace creation; a failing executable stops creation without a blind retry. Descriptive notifications and workspace cold-shell recovery remain planned.

## Test

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Rust integration tests invoke the executable against isolated Unix sockets, state directories, controlled Herdr replies, and a fake `herdr` shell-input executable. Worker tests exercise real process identity checks with disposable test processes. Tests do not use the live Herdr socket.

Run the controlled concurrency tests on their own:

```sh
cargo test --test concurrency
```

The overlap test pauses request A's move response, then requires overlapping request B to finish cold-shell recovery before A is released. The fake server keeps accepting requests during the pause. Assertions check one warm-shell move, one cold tab, distinct destinations, working-context setup, and focus. This is current busy-lock behavior, not a requirement that both requests get warm shells.

The blocked-refill test starts with two pool shells, moves one to a new tab, then holds the refill worker's new-shell response. The foreground CLI must exit while that response remains blocked and the refill worker and launcher remain alive. Releasing the response lets the pool return to two shells.

Both tests use a private copy of the selected executable, an isolated stateful API, and a `ps` shim that delegates identity checks to real `ps`. They wait for detached refill launchers/workers to exit, with identity-checked cleanup on failure. The fake `herdr` logs shell input rather than executing setup; actual shell startup and rendering are not covered. Event gates establish ordering; deadlines bound hangs, not performance.

The archived Go implementation does not support `new-workspace` or the updated usage text. Its pre-workspace parity checks remain historical evidence; do not expect the full current suite to pass against it. To run the unchanged concurrency tests against Go:

```sh
reference_dir=$(mktemp -d)
(cd legacy && go build -o "$reference_dir/herdr-shell-pool" .)
HERDR_POOL_TEST_BIN="$reference_dir/herdr-shell-pool" cargo test --test concurrency
(cd legacy && go test ./...)
```

Unset `HERDR_POOL_TEST_BIN` to test Rust again. See [IMPLEMENTATION-STATUS.md](IMPLEMENTATION-STATUS.md) for evidence and remaining verification limits. Passing simulated protocol tests does not prove real Herdr rendering, layout, or shell startup behavior.
