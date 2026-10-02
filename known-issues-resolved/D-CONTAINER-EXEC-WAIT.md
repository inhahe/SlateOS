### D-CONTAINER-EXEC-WAIT. Real in-container `docker exec` + synchronous wait — RESOLVED (all four steps landed)

**Status (2026-07-01): steps 1–4 done and boot-validated.** `container
exec` is no longer a net_ns-switch facade — it launches a genuine process
inside the container and (foreground) blocks until it exits, printing the
exit status. Step 4 (healthchecks) now landed too: the OCI `Healthcheck`
config is parsed, stored on the container, and driven by a periodic
non-blocking supervisor that surfaces health in `inspect`/`ps`.

**What landed:**
1. `container::wait_process(pid) -> KernelResult<i32>`
   (`kernel/src/container.rs`): the generalised block-on-exit primitive.
   Parks the caller on an arbitrary spawned global pid via
   `pcb::set_wait_task` + `sched::block_current`, woken by the
   zombie-transition path (`remove_thread` hands back the registered
   wait-task). Lost-wakeup-safe (re-check after register + scheduler
   `pending_wake`). On zombie it reads `pcb::exit_code(pid)` and reaps via
   `pcb::try_reap`, so an exec'd non-init child never lingers unreaped.
2. `container::exec_path(id, guest_cmd, argv) -> KernelResult<ExecSpawn>`:
   resolves `guest_cmd` under the container rootfs (`resolve_in_rootfs`,
   `..` cannot escape), reads the ELF, `spawn_process`es it, and
   `add_process_task`s it into the container's cgroup + PID/user/network
   namespaces + rootfs jail (the `run` wiring, minus flipping state /
   recording `init_pid`). Rolls the spawn back on bind failure. Stdio is
   left at the console default (foreground output appears live).
3. Shell `container exec [-d] <id> <cmd> [args...]`
   (`kernel/src/kshell.rs`, cmd_container "exec" arm): builds argv from the
   tokens, calls `exec_path`; foreground → `wait_process` + print exit
   status + `remove_process_task` cleanup; `-d` → print pid and return.

**Root-cause fix bundled in:** cgroup task-count accounting was previously
decremented **only** by an explicit `set_task_cgroup`/`remove_process_task`
while the task was still alive; a task that simply *exited* while assigned
to a non-root cgroup left a stale `nr_tasks` count forever (the task is
gone from the scheduler table before anyone can move it back to root).
`sched::reap_dead_tasks` now auto-detaches a reaped task from its cgroup
(skipping the root group; `detach_task` is saturating so a
detach-then-die can't underflow). This makes teardown accounting robust
for *any* exiting task, not just exec'd ones.

**Validation:** boot self-test `[container]   exec + wait
(exec_path/wait_process): OK` — creates a Running container with a real
rootfs, stages `/bin/hello`, execs it, yields until it zombifies, and
asserts: exit code 0 captured, process reaped (`pcb::state` is `None`),
cgroup billed +1 while alive then 0 after reap, plus the error paths
(exec on a non-Running container → InvalidArgument, missing binary →
NotFound, `wait_process(bogus)` → NoSuchProcess). BOOT_OK, hello's stdout
observed once in the serial log.

**Step 4 (healthchecks) — landed:** `oci::HealthcheckConfig`
(`kernel/src/oci.rs`) parses the OCI `Healthcheck` (test-token +
interval/timeout/retries/start_period, CMD vs CMD-SHELL). Each container
stores the probe plus its live health state
(`health_status`/`health_fail_streak`/`health_started_ns` and the
in-flight probe pid/task/deadline). The pure state machine
`container::apply_probe_result` implements the Docker semantics
(start-period grace does not count failures while `Starting`; a
`retries`-long failure streak → `Unhealthy`; any pass → `Healthy` + reset
streak) and is unit-covered by boot self-test `19k2h`.

The probes are driven by a **non-blocking** supervisor: a persistent
repeating `hrtimer` (250 ms tick, `start_health_monitor`, armed just
before `BOOT_OK` so it can't perturb the hrtimer self-test's exact
`pending_count` assertion) fires in ISR context, submits `health_tick_job`
to the shared `workqueue`, and `health_tick` polls every container.
Critically it **never blocks the single workqueue worker**: each probe is
launched via `exec_path`, then *polled* for its zombie transition on
subsequent ticks (never `wait_process`-blocked), reaped via the
`wait_process` fast path once dead, scored via `apply_probe_result`, and a
probe that overruns its timeout is `kill_process_threads`'d and scored as
a failure. The tick uses snapshot-under-lock → act-outside-lock (exec /
reap / kill / remove all take the table lock internally) → write-back.
Health is surfaced in `inspect` (JSON `health` field + human Health line
with failing streak) and `ps` (a `(healthy)`/`(unhealthy)`/`(health:
starting)` sub-state on the status column). Boot self-test `19k2s` drives
a real `/bin/hello` CMD probe deterministically to `Healthy`.

**Discovered/documented:** 2026-07-01 (while surveying the next container
increment after `docker network`). All four steps landed same day.
