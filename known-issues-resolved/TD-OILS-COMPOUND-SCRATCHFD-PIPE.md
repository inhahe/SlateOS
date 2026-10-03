### TD-OILS-COMPOUND-SCRATCHFD-PIPE. A compound command's `2>&N` dup through a scratch fd aliased to a *pipe* (command substitution) leaks stderr to the real terminal — 2026-07-19 — RESOLVED 2026-07-28

**Where:** `userspace/oils/src/interp.rs` — the compound-command redirect path
(the `saved_fds`/`SavedFd` + `stderr_stack` mechanism, redirect function ~line
1700+) vs. the simple-command paths (`run_external` ~line 4219, `run_builtin`
~line 6459) which now share `install_extra_fds`/`restore_extra_fds`.

**What:** for a *simple* command, `{ … } 3>&1 2>&3` inside `$( … )` correctly
routes stderr into the capture pipe (fixed 2026-07-19, test
`same_command_fd_dup_resolves`). But the *compound* form
`r=$({ echo o; echo e >&2; } 3>&1 2>&3)` still sends the inner `>&2` output to
the shell's real stderr instead of the command-substitution pipe:

```
bash:  r == "o\ne"
osh :  r == "o"   (and "e" leaks to the terminal)
```

The base compound case without a scratch fd (`{ echo e >&2; } 2>&1` in a
command sub) works — the gap is specifically the interaction of a scratch fd
aliased to an `Out::Pipe` (`3>&1` where fd 1 is the capture pipe) with the
compound path's separate stderr routing (`stderr_stack`), which does not
consult `open_write_fds[3]` the way the simple-command paths do.

**Re-verified 2026-07-20** (still OPEN, genuine — not stale). Root cause
pinned: `install_extra_fds`'s `AliasStd(n)` branch only special-cases
`(1, Out::Pipe(w)) => pipe_writer_to_file(w)`; when the enclosing `out` is
`Out::Capture(buf)` (a command substitution's in-memory `Vec<u8>`, which has
no OS fd) it falls through to `snapshot_std_fd(1)` and aliases the scratch fd
to the shell's *real* stdout instead of the capture. Both the stderr-route
(`r=$({ echo o; echo e >&2; } 3>&1 2>&3)` → osh `r="o"`, "e" leaks) and the
direct-write (`r=$({ echo hi >&3; } 3>&1)` → osh `r=""`, "hi" leaks) variants
still fail; the `Out::Pipe` pipeline form (`… 3>&1 2>&3 | cat`) works. The
impedance mismatch is fundamental: `open_write_fds` maps fd→`Arc<File>` but a
capture is a buffer, so a scratch fd can't be stored there.

**Why deferred:** rare edge-of-an-edge (a brace/subshell group that dups
stderr through a user scratch fd, *and* is itself inside a capturing command
substitution). The simple-command case — the common one — is fixed. A narrow
buffer-table patch (parallel `open_write_bufs: HashMap<i32, Arc<Mutex<Vec<u8>>>>`
consulted by every `>&N`/`exec_dup_source` write site + merged into the capture
after the body) would work but is another band-aid on the collapsed-`RedirPlan`
model — deliberately avoided per the "watch for band-aid accumulation" rule.
The alternative uniform fix (back command substitution with a real OS pipe +
drainer thread so fd 1 always has an OS fd) risks deadlock/regression on the
hottest, most-tested path (large-output command subs). Neither is worth it for
this obscure case ahead of the real refactor below.

**Proper fix:** unify the compound redirect executor onto the same
`install_extra_fds`/`restore_extra_fds` + `open_write_fds`-consulting stderr
routing the simple paths use, so a compound `2>&N` resolves N against the
materialised scratch fd (pipe-aware) rather than falling through to the real
stderr. Ultimately this is another symptom of the collapsed-`RedirPlan`
order-loss; the long-term proper fix is an ordered fd-op executor shared by
all command kinds.

**Update 2026-07-28.** The "parallel buffer table" half of this is now much
cheaper than it was when it was rejected above. TD-OILS-BG-SINK-OUTLIVES-SUBSHELL
replaced the bare `Arc<Mutex<Vec<u8>>>` with a `CaptureSink` handle that already
carries a writer count and a `share()`/drop protocol, so an `open_write_fds`
entry could hold a *sink* alongside a `File` (one enum, not a second map merged
after the body) and a scratch fd aliased to the capture would both write to it
and keep it open, with no band-aid merge step. That does not by itself fix the
`RedirPlan` order-loss, but it removes the "a capture is a buffer, so a scratch
fd can't be stored there" impedance mismatch that made the narrow fix ugly. The
same change would close the residual noted under
TD-OILS-BG-SINK-OUTLIVES-SUBSHELL (`exec 3>&1; ( echo held >&3 & )`).

**RESOLVED 2026-07-28.** Fixed as described in that update — the uniform
version, not the band-aid. A new `WriteFd` enum

```rust
enum WriteFd { File(Arc<File>), Capture(CaptureSink) }
```

is what `open_write_fds`, `exec_stdout` and `exec_stderr` now hold, so a
descriptor that names a capture is representable everywhere a descriptor is,
and there is no second table and no post-hoc merge. The pieces:

* **One resolver for `N>&1`/`N>&2`.** `Shell::alias_std_write_fd(n, out)`
  resolves an alias against the *ambient* fd 1 (`Out::Capture` → the sink,
  `Out::Pipe` → the pipe, otherwise `snapshot_std_fd`). Both the scoped path
  (`install_extra_fds`) and the persistent one (`exec 3>&1`, via
  `exec_dup_source`) go through it, so the two forms can no longer disagree.
  Reaching the persistent path meant threading the ambient `Out` through
  `resolve_redirects` → `resolve_one_redirect` → `apply_persistent_redirect` →
  `apply_persistent_dup_out` → `exec_dup_source`; that plumbing is the reason
  `exec 3>&1` inside `$( … )` used to alias the terminal.
* **`snapshot_std_fd` returns a `WriteFd`**, so `exec 1>&N` onto a capture is
  a representable state rather than a case with nowhere to go.
* **Sinks per direction.** `WriteFd::as_stderr_target()` gives the fd-2 form
  (`2>&N` on a capture becomes a second writer on the sink, interleaving in
  write order); the compound path's fd-1 form runs the body with the sink as
  its ambient `Out` (`stdout_capture_fd`), which is what a dup of fd 1 means.
  `write_to_write_fd` / `child_out_from_write_fd` are the write and
  spawn-a-child forms — a child gets a pipe the parent drains into the sink,
  since a buffer cannot back a live OS descriptor.
* **Counted vs. uncounted aliases.** `clone_for_subshell` aliases capture
  entries *uncounted* (a synchronous subshell finishes before the `$( … )`
  reads), while a clone that outlives its parent — a `&` job, a coproc — calls
  `Shell::share_write_fds()` to upgrade every inherited entry (including
  `exec_stdout`/`exec_stderr`) to a counted `CaptureSink::share()`. That is
  what closes the TD-OILS-BG-SINK-OUTLIVES-SUBSHELL residual: bash's job holds
  a dup of the substitution's pipe on *every* descriptor it inherited it on, so
  `x=$( exec 3>&1; ( echo held >&3 & ) >/dev/null )` collects `held`.

Note this did **not** require fixing the `RedirPlan` order-loss — the ordered
fd-op executor is still the long-term want (see the "proper fix" above), but
the leak was the impedance mismatch, not the ordering.

**Coverage:** `tests/corpus/redirect-scratch-fd-capture.sh` (new, byte-identical
to bash 5.2 — scoped and persistent aliases, `2>&3`, externals, pipeline
stages, a fd-3 alias taken *outside* the substitution, `3>&1 >u.txt` ordering,
two `&`-job hold shapes, and `3>&-`), plus unit tests
`scratch_fd_alias_inside_a_substitution_reaches_the_capture`,
`persistent_exec_alias_inside_a_substitution_reaches_the_capture` and
`background_job_holds_a_capture_inherited_through_fd_three`.
