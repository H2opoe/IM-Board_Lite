# macos-owned-process

Production held-Child owner extracted from current IM-Board main. The original
Rust state machine and SDK C probe are retained. The public APIs accept an owned
blocking/Tokio Child, and never construct a target from a bare PID or PGID.

Before spawn, hosts call `check_supported()` and set `process_group(0)` on the
command. After transferring the Child through `from_blocking` or `from_async`,
the owner is the sole observer and reaper. `observe_exit`/`wait_*` use WNOWAIT;
`confirm_exit` reaps only after the original group has no live descendants.
`terminate` and `request_kill` keep the original anchor and strict fail-closed
identity rules. `pid()` is read-only evidence for the host's existing API.

`test-support` is disabled by default. It exposes only the no-argument
`unconfirmed_fixture()` with pid0, no Child and identity/signals disarmed. It
does not provide arbitrary target construction.

The app keeps its SIGCHLD/LocalRuntimeSupervisor integration test. Eight owner
property tests live here; the C compilation and libproc link belong to this
crate after host integration. The host supplies stdin/stdout/stderr and stage
budgets. A successful group cleanup does not prove a privileged helper or
escaped descendant has completed, so the signing host must retain its own
durable blocker for those cases.
