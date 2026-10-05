## 47. Container auto-restart (`--restart`) and auto-remove (`--rm`) run through the kernel workqueue as a deferred reaper, driven off the init-exit hook

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for Docker's
`--restart`/`--rm` lifecycle automation (increments 48–54). These are
implementation choices (where the respawn/delete work runs, and how the Docker
policy state machine is encoded), not an operator policy fork, so they are
resolved autonomously and recorded here.

**Context.** A real container runtime must react to a container's init process
*exiting*: `--restart` policies relaunch it, and `--rm` deletes the container.
The only place the kernel learns an init has died is `notify_init_exit(pid,
code)`, which is called from the **generic process-exit (zombie-transition)
path** — a context that holds scheduler state and cannot safely allocate a new
address space, read the VFS, or tear down an overlay. So the reaction cannot run
inline there.

**The decision.**
1. **Deferred reaper on the kernel workqueue.** `notify_init_exit` only updates
   container *state* under the table lock (→`Stopped`, records the exit code),
   decides whether a restart/remove is due, then `workqueue::submit`s a callback
   (`do_container_restart` / `do_container_autoremove`) that runs in the
   `kworker` task context where spawning, VFS reads, and overlay teardown are
   safe. This mirrors the existing `sched::supervisor` task-restart precedent. A
   full queue drops the action with a logged warning rather than blocking or
   spawning on the exit path.
2. **Docker restart-policy semantics encoded as a pure decision function**
   (`should_auto_restart(policy, exit_code, user_stopped, restart_count)`): `no`
   never restarts; `always`/`unless-stopped` restart on any exit;
   `on-failure[:N]` restarts only on non-zero exit, capped at N (0 = unlimited).
   `unless-stopped` is identical to `always` in our single-session model (there
   is no daemon restart to replay a "don't auto-start on boot" distinction).
   Being pure, it is exhaustively unit-tested without spawning anything.
3. **`user_stopped` gate distinguishes a graceful stop from a kill.** A user
   `stop()` sets `user_stopped=true`, which suppresses *every* restart policy
   (Docker: a `docker stop` is intentional and must not fight the user). A
   `kill()` does **not** set it — Docker still honours the restart policy after a
   kill. The flag is cleared on every (re)launch.
4. **`restart_count` is incremented in `notify_init_exit` (when it schedules an
   auto-restart) and reset to 0 only on a *manual* `start`/`restart`** — never on
   the internal `run_path`/auto-restart path. This makes an `on-failure:N` series
   actually terminate after N attempts instead of looping forever, while a human
   intervention re-arms the budget.
5. **Restart tears down a running container stop-before-kill.** `relaunch_recorded`
   calls `stop(id)` (leaves `Running`) *before* `kill(id)`, so when the old
   init's death reaches `notify_init_exit` the container is no longer `Running`
   and cannot trigger a spurious nested restart. This closes a self-restart race
   the naive kill-first order would open.
6. **`--rm` yields to `--restart`.** In `notify_init_exit` the auto-remove branch
   is an `else` of the restart branch: a container that is going to restart is
   never removed. Deletion is deferred identically because it touches the
   VFS/overlay; the container is already `Stopped` by reaper time, so `delete()`
   (which refuses a `Running` container) succeeds.

**Alternatives considered.**
- *React inline in `notify_init_exit` / from a softirq.* Rejected: the exit path
  and softirqs run in restricted contexts that cannot spawn a process or touch
  the VFS. The workqueue is the established "defer to full task context" channel.
- *A dedicated container-reaper kernel thread polling for dead inits.* Rejected as
  redundant — the workqueue already provides the task-context execution and a
  wakeup; a bespoke thread would duplicate it and add a polling loop.
- *Set `user_stopped` on `kill()` too (treat kill as a user stop).* Rejected —
  it contradicts Docker, where `docker kill` still triggers the restart policy.
- *Reset `restart_count` whenever the recorded command is replayed.* Rejected —
  it would make `on-failure:N` loop forever, defeating the cap.

**Update (increment 57): exponential restart back-off implemented.**
Auto-restart no longer fires immediately: `notify_init_exit` now schedules the
restart through an hrtimer with an exponential crash-loop back-off
(`restart_backoff_ns`: 100 ms, 200 ms, 400 ms, … doubling per consecutive
attempt, capped at 30 s), so an `always`-policy container that crashes instantly
can't spin the CPU in a tight respawn loop. The timer fires in ISR context and
hands the actual relaunch to the kworker via a trampoline
(`restart_backoff_fire` → `workqueue::submit(do_container_restart)`) — spawning
inline on the timer/exit path is unsafe. The back-off is derived from the
(already-incremented) `restart_count`; it is *not* reset after a period of
successful running (Docker resets after ~10 s up), because `restart_count`
doubles as the `on-failure:N` cap and resetting it would defeat the cap. In
practice the monotonic back-off is strictly safer (a flaky container backs off
more, never less).

**Update (increment 56): lifecycle event log.** A bounded (256-entry) ring
records create/start/die/stop/kill/pause/unpause/restart/destroy events
(`record_event`/`events_snapshot`, surfaced by `container events`). `record_event`
is lock-local (event-log lock only, never the container table), so it is safe
from within `with_table` closures and the process-exit path.

**Limitations / deferred.** Restart back-off is *not* reset after a successful
run window (see the increment-57 update above for why — it shares the
`on-failure:N` counter). `unless-stopped` collapses to `always` because there is
no persistent daemon to replay boot-time start decisions. `container ls -n/-l`
order by a monotonic per-table creation sequence (`created_seq`), added because
slot ids are reused and so are not creation order.

**Where it bites.** `kernel/src/container.rs` (`RestartPolicy` +
`parse_restart_policy`/`should_auto_restart`; `Container`/`ContainerConfig`/
`ContainerInfo` gain `restart_policy`/`restart_count`/`user_stopped`/
`auto_remove`/`created_seq`; `ContainerTable::next_seq`; `notify_init_exit`
rewrite; `do_container_restart`/`do_container_autoremove` workqueue callbacks;
`relaunch_recorded` stop-before-kill; `set_restart_policy`; self-tests
19u/19v/19w); `kernel/src/kshell.rs` (`container create restart=`/`rm`,
`update --restart`, `ls -a`/`-n`/`-l` + newest-first ordering, `inspect --json`).
