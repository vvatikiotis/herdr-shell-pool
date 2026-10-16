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

Run the same Rust tests against the archived Go implementation:

```sh
reference_dir=$(mktemp -d)
(cd legacy && go build -o "$reference_dir/herdr-shell-pool" .)
HERDR_POOL_TEST_BIN="$reference_dir/herdr-shell-pool" cargo test
(cd legacy && go test ./...)
```

Unset `HERDR_POOL_TEST_BIN` to test Rust again. See [IMPLEMENTATION-STATUS.md](IMPLEMENTATION-STATUS.md) for evidence and remaining verification limits. Passing simulated protocol tests does not prove real Herdr rendering, layout, or shell startup behavior.
