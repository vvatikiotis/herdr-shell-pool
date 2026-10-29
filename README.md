# Herdr Shell Pool

Herdr Shell Pool keeps eligible shells ready in a dedicated pool workspace so new tabs and splits can reuse them instead of waiting for shell startup. It also supports creating a workspace with a warm shell through sessionizer. Requires macOS or Linux, Rust with edition 2024 support to build, and Herdr at runtime. Shell setup invokes the `herdr` executable from `PATH`.

## Build and install

From this directory, build and replace the executable:

```sh
cargo build --release &&
cp target/release/herdr-shell-pool ./herdr-shell-pool-bin.new &&
chmod 755 ./herdr-shell-pool-bin.new &&
mv -f ./herdr-shell-pool-bin.new ./herdr-shell-pool-bin
```

Herdr's new-tab and left/down split bindings must invoke `herdr-shell-pool-bin` in this directory. The temporary-file-and-rename sequence avoids overwriting an executable that may be running. Automatic pool preparation at Herdr startup is not implemented; run `./herdr-shell-pool-bin fill` to prepare the pool as needed.

## CLI

```sh
./herdr-shell-pool-bin fill
./herdr-shell-pool-bin status
./herdr-shell-pool-bin new-tab
./herdr-shell-pool-bin split-left
./herdr-shell-pool-bin split-down
./herdr-shell-pool-bin new-workspace --cwd <absolute-path> --label <name>
```

`fill` targets two eligible idle shells; it does not shrink an oversized pool. `status` reports `<count>/2 ready`. Tabs and splits reuse an eligible shell when possible and otherwise silently open a cold shell if the pool is busy, empty, or the move makes no change. Other move, setup, or focus failures return an error rather than automatically retrying with a cold shell. Replenishment runs in the background after a successful request and may fail silently.

`new-workspace` is intended for sessionizer's unopened-project selection. It requires an enterable absolute directory and label, and moves a warm shell into a workspace whose initial tab has the same label. Unlike tabs and splits, it has no cold-shell fallback: pool contention, no eligible shell, or an operation failure stops workspace creation; an error after moving may leave a created destination.

Shell history behavior: after setup, the tool submits a separate space-only line to advance history before focus. This supports zsh with `HIST_IGNORE_SPACE` and bash with `HISTCONTROL=ignorespace` or `ignoreboth`; other shells and custom history hooks are not generalized.

Herdr socket selection uses `HERDR_SOCKET_PATH` when set; otherwise it resolves the configured socket, including `HERDR_SESSION` when supplied. Runtime state uses `XDG_STATE_HOME`, falling back to the user's local state directory. Configuration uses `XDG_CONFIG_HOME`, falling back to the user's configuration directory.

## Sessionizer integration

Sessionizer looks for this project's `herdr-shell-pool-bin` beside its plugin directory. Existing-workspace selection, Ctrl+f, and selected-directory output remain unchanged. Build and install compatible versions of both tools together, and relink sessionizer's updated manifest to enable its pane-exit hook for Ctrl+d close-focus recovery. Sessionizer can run without shell pool: a missing executable preserves ordinary workspace creation, while a failing executable stops creation without a blind retry. The shell-pool tool does not manage close-focus behavior. See the [sessionizer README](../herdr-sessionizer/README.md).

The sessionizer pane-exit fix has been checked with isolated Herdr scenarios, but full interactive picker-to-warm-workspace verification remains pending. Simulated protocol checks do not establish live Herdr rendering, layout, or shell-startup behavior.
