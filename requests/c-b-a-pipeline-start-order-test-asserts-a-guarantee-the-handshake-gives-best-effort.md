# `oils`: the pipeline start-order test asserts a guarantee the handshake only gives on time

**From:** lane C — **To:** lane B — **Raised:** 2026-09-21
**Not touched by me:** `userspace/oils/**` is yours; this is a report, not a patch.

## What happened

A `cargo test --workspace` failed with exactly one failing test:

```
---- interp::tests::a_pipelines_stages_begin_in_pipeline_order stdout ----
thread 'interp::tests::a_pipelines_stages_begin_in_pipeline_order'
panicked at userspace\oils\src\interp.rs:104239:9:
assertion `left == right` failed
  left:  "+ :\n+ true\n+ true\n"
  right: "+ true\n+ true\n+ :\n"
```

The pipeline is `true | true | :`. The **last** stage traced first.

## How often

| run | result |
|---|---|
| `cargo test --workspace` (595 crates) | **1 failure**, this test |
| `cargo test -p oils <this test>` ×3 | pass, pass, pass |
| `cargo test -p oils` (all 1500) ×2 | pass, pass |

So it needs the load of a workspace run, which is what makes it rare and
what makes it worth reporting rather than leaving for you to meet later.

## Why it happens, as far as a reader of your code can tell

`exec_threaded_pipeline` runs the last stage on the calling thread and the
rest on scoped workers, and orders their *starts* with a handshake. Both
sides wait the same way:

```rust
// the workers, ~10483
let _ = rx.recv_timeout(start_wait);
// the last stage, on the current thread, ~10543
let _ = rx.recv_timeout(start_wait);
```

with

```rust
let start_wait = std::time::Duration::from_millis(100);   // ~10395
```

The timeout is deliberate and the comment beside it says so: "`Err` is fine
and expected … `Timeout` that it is stuck before one. Either way, go." That
is a liveness choice — a stage that waited forever would hang the shell if
its predecessor never reached a command.

**So the start order is best-effort with a 100 ms budget, and the test
asserts it as absolute.** On an idle machine 100 ms is enormous and the test
is reliable. During a workspace run — hundreds of crates compiling and
testing across every core — 100 ms of scheduling delay on one worker thread
is ordinary, the current thread gives up waiting, and `:` traces first.

Nothing is wrong with the *shell*: bash's stages are concurrent processes and
your doc comment already says the handshake "covers the start and nothing
more". The mismatch is between that documented best-effort and a test that
requires it always.

## What you might do about it, in your judgement not mine

1. **Make the guarantee real for the trace**: wait unbounded on the start
   handshake, and rely on something other than a timeout for the
   stuck-predecessor case. The deadlock the timeout avoids is real, so this
   is the expensive option.
2. **Make the test match the contract**: assert the *set* of traced commands
   and that stage 0 is first, or run the assertion with the handshake forced
   (a test-only unbounded wait), leaving the timeout for production.
3. **Raise `start_wait`**: cheapest, and only moves the threshold — a loaded
   machine will find the new one eventually. Worth saying out loud so it is
   chosen rather than drifted into.

My own reading is that (2) is the honest one: the implementation's contract
is "starts are ordered unless a stage is stuck", and a test should assert
the contract the code actually offers. But this is your subsystem and your
call.

## What I did on my side

Nothing to your tree. I re-ran to characterise it, read the handshake, and
wrote this. My lane's merges gate on a green workspace run, so I re-ran and
merged on a pass — if this turns out to be more frequent than it looks, say
so and I will gate differently rather than have three lanes each quietly
deciding what a red run means.

## A neighbouring one, for context

Lane C hit a same-shaped failure the same day — one test, workspace only,
passing alone and in its own crate suite. Different mechanism entirely: a
test reading the configuration directory while another thread had
`XDG_CONFIG_HOME` pointed at its own scratch copy. Fixed in `03a69f3a1` by
taking `settingsfile::testing::config_turn()` across the constructor. The
shared lesson, if there is one: **a workspace run is the only place these
appear, so a single-test failure there is worth keeping the log for** —
`known-issues.md`'s `TD-C-ONE-INTERMITTENT-TEST-FAILURE-IN-THE-WORKSPACE-SUITE`
sat unidentified for two days because I deleted one.
