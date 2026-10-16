# Herdr Shell Pool

Rust implementation of the existing behavior described in [SPEC.md](SPEC.md). The original Go source, helper tests, and module files are preserved in `legacy/` as a reference. Desired changes are separate in [PLANNED.md](PLANNED.md).

## Build and run

Requires Rust with edition 2024 support, macOS or Linux, and Herdr. Shell setup invokes the `herdr` executable from `PATH`.

```sh
cargo build --release
install -m 755 target/release/herdr-shell-pool ./herdr-shell-pool-bin
./herdr-shell-pool-bin status
```

Actions: `fill`, `status`, `new-tab`, `split-left`, `split-down`.

Herdr's new-tab and left/down split bindings point to `herdr-shell-pool-bin` in this directory. Rebuild and install it with the commands above after source changes. The binary is ignored by Git.

Do not overwrite an executable currently running; install a replacement through a temporary file and rename it if needed. Startup integration is not added.

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

Run the same Rust tests against the archived Go implementation:

```sh
reference_dir=$(mktemp -d)
(cd legacy && go build -o "$reference_dir/herdr-shell-pool" .)
HERDR_POOL_TEST_BIN="$reference_dir/herdr-shell-pool" cargo test
(cd legacy && go test ./...)
```

Unset `HERDR_POOL_TEST_BIN` to test Rust again. See [IMPLEMENTATION-STATUS.md](IMPLEMENTATION-STATUS.md) for evidence and remaining verification limits. Passing simulated protocol tests does not prove real Herdr rendering, layout, or shell startup behavior.
