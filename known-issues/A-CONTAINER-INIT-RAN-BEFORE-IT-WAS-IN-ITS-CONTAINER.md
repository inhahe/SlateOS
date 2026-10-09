### A-CONTAINER-INIT-RAN-BEFORE-IT-WAS-IN-ITS-CONTAINER -- 2026-10-09 -- OPEN (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-09, awaiting a boot on main.

**In short:** starting a container (`container run`, and `container exec`
for a second program) created the program and made it runnable first, and
only then put it inside the container -- its own root directory, its
namespaces, its resource limits. On a machine with more than one CPU,
another CPU could start running the program in that gap, on the host's
files and the host's network, unbilled to the container. If it finished
that fast, its container was also left saying "running" with nothing in it.
Single-CPU boots, which is all the boot tests run, could not show it.

**Where.** `container::run_with_abi` (now `run_with`) and `exec_path_env`
called `spawn::spawn_process` (whose last step admits the first thread to
the scheduler), then `add_process_task` (cgroup, pid/user/net/UTS
namespaces, the root jail, volumes, read-only root), then recorded the init
pid and flipped the container to `Running`. `run`'s own doc said the init
"does not execute until the scheduler next picks it", which on two CPUs is
at once. `notify_init_exit` recognises the init by the recorded pid, so an
init that exited before step 4 left its container `Running` forever.

**How it was found.** The first two-CPU boot with the self-tests on
(2026-10-09, `fastboot.sh` with `SMP=2`): the container `logs` self-test
(Test 19t) holds interrupts off on CPU 0 and assumes the init it starts
cannot run; CPU 1 ran it -- a native test binary forced onto the Linux ABI,
which faulted (`#GP` on its `hlt`) and exited -- and the test's own
teardown then panicked on `"logs init has a thread"`.

**Fix (lane-a-wip).**

- `spawn::spawn_process_suspended` builds the whole process and its first
  thread, unstarted; `SuspendedSpawn::start` admits it, and dropping it
  unstarted undoes it (thread unregistered and killed, entry record freed,
  process destroyed). `spawn_process` is now that, started at once.
- `run` spawns unstarted, binds, records the init and flips the state,
  installs the published ports, and only then starts it -- so its first
  instruction runs inside the container, its ports open, and its exit,
  however soon, finds it recorded as the init. `exec` binds before starting
  too. Step 4 re-checks `Created` under the table lock, so two concurrent
  `run`s of one container cannot both make an init. A start that fails (the
  init killed while it waited) unbinds it and leaves the container as `run`
  found it.
- So that nothing but the start can run such a thread: a task spawned
  suspended is marked `Task::awaiting_admission`, and only `sched::admit`
  starts it. A wake (`wake`, `try_wake`, the deferred drain, an expired
  sleep slot -- each could reach it through a stale wait-queue entry) leaves
  it be, and a `resume` returns it to waiting. One suspended before its
  admission -- a program exec'd into a paused container -- stays suspended
  when admitted, and the unpause runs it. Before, `admit` was a bare `wake`:
  it refused a suspended task, and `thread::admit` then killed the thread.

**Tests.** `spawn::test_spawn_suspended` (an unstarted spawn waits through
a wake, is undone when dropped, runs to its exit when started);
`sched::test_admission` (wake, `try_wake`, deferred wake, suspend/resume
leave a task awaiting admission unstarted; admitted while suspended it stays
suspended; the resume runs it). The container self-tests 17, 19t and 20 pin
their init to the CPU they hold (`run_with`'s `affinity`), which on two
CPUs is what keeps it from running under them.
