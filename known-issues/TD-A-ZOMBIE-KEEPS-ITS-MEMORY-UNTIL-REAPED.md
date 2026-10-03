## TD-A-ZOMBIE-KEEPS-ITS-MEMORY-UNTIL-REAPED (lane A, 2026-10-02) — OPEN

**Status:** OPEN

**In short:** when a Linux process exits, its memory is freed immediately.
What remains until the parent waits for it (a zombie, "a finished process
whose exit status nobody has collected yet") is a small record. Here, a
process's memory is freed only when the parent collects it. So a zombie whose
parent never waits keeps every page it had. Since 2026-10-02 zombies appear in
`/proc` (`fs/procfs.rs` `pid_dir_exists`), so this also shows: a zombie's
memory files describe memory Linux would already have freed.

## What is wrong

- `pcb::destroy` → `finish_process` releases the VMAs' backing and the page
  tables. It runs at the reap (`wait4`, or `teardown_fixture` in a self-test),
  not when the last thread exits (`pcb::remove_thread`, the zombie
  transition).
- Linux frees the address space in `do_exit` → `exit_mm`, before the process
  is a zombie.

What a reader of a zombie's `/proc/<pid>` sees, against Linux:

| File | Here | Linux |
|---|---|---|
| `maps` | its old mappings | empty |
| `statm` | its old sizes | all zeros |
| `stat` fields 23–24 (vsize, rss) | its old sizes | 0 |
| `status` `Vm*` lines | present, old sizes | absent |
| `cmdline`, `environ` | the spawn-time argv/envp snapshot | empty (no `mm`) |
| `auxv` | the saved vector | empty |

## The proper fix

Free the address space at the zombie transition, as `exit_mm` does. The
memory-backed files then read empty for nothing more than the absence of an
address space. Points to watch:

- **Killed threads.** A thread killed while another CPU ran it may still be on
  the page tables (`pcb::note_killed_on_cpu`,
  `free_address_space_when_unused`). That machinery already defers the free
  to a CPU switch, and must apply at the earlier point too.
- **Readers after exit.** The robust-list and ctid walks in
  `thread_clone::on_thread_exit_hook` run before `detach_address_space`, so
  they are safe. Check every later reader of a zombie's memory or VMAs. A
  `wait4` rusage field such as `ru_maxrss` must be captured before the free.
- **`cmdline`/`environ`.** These read pcb snapshots, not memory, so they need
  an explicit "zombie reads empty" rule (or a dropped snapshot) to match
  Linux.

## Where

- `kernel/src/proc/pcb.rs`: `remove_thread` (the zombie transition),
  `destroy`, `finish_process`, `destroy_process_resources`.
- `kernel/src/proc/thread.rs`: `on_thread_exit`.
- `kernel/src/fs/procfs.rs`: `gen_pid_maps`, `gen_pid_statm`,
  `gen_pid_cmdline`, `gen_pid_environ`, `gen_pid_auxv`, `build_pid_stat`,
  `build_pid_status`.
