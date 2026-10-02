## 1521. Descriptors travel over Unix-domain sockets as kernel-held references, released through a queue, and garbage-collected as Linux collects them

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a Linux program can now hand an open file to another over a
Unix-domain socket (`SCM_RIGHTS`), which Wayland, D-Bus, PipeWire, tmux and
every multi-process browser rely on. While a descriptor travels, the kernel
holds one reference to its object, as one more process holding it would; the
receiver gets it as a new descriptor, or, if never received, the reference is
let go. Sockets that only keep each other alive this way are found and
released, as Linux does. Along the way, `pidfd_getfd` stopped leaking a
reference when a process took back a descriptor it already shared, and
`close` stopped being able to release one reference twice.

**What changed:**
- `ipc::passed`: a `Passed` is the reference (taken with each kind's `dup`),
  a `Bundle` the descriptors one message carries. Dropping one queues its
  release; `passed::drain` performs the queue, called wherever one may drop,
  once no lock is held. A user's descriptors in flight are counted; one
  already over its `RLIMIT_NOFILE` sends no more (`ETOOMANYREFS`, root
  exempt), as Linux's `too_many_unix_fds`.
- Datagrams carry their bundle. On a stream the bundle is a *mark* on the
  bytes its send wrote (`stream_socket`), and a receive that reaches the mark
  takes it and stops at the stretch's end, as Linux's
  `unix_stream_read_generic` stops after the skb whose descriptors it took. A
  peek hands up a copy, from which new references are taken.
- `unix_socket::collect_garbage`: Linux's `unix_gc` -- candidates are
  sockets every holder of which is a reference queued somewhere; those a
  queue outside the candidates leads to are kept; the rest have their queues
  emptied, which ends them. Run on every close while any socket is in
  flight, under `TABLE`, which every take of descriptors off a queue also
  holds.
- The receiving process holds one reference per object however many
  descriptors name it (`close` releases with the last): installing a passed
  descriptor whose object the process holds already keeps the descriptor and
  drops the reference, in one step under the process table's lock
  (`pcb::linux_fd_install_passed`). `pidfd_getfd` goes the same way, and
  `close`, `dup2` and `close_range` judge "last" in the same step as the
  removal (`pcb::linux_fd_take_last`).

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **Release through a queue, drained once no lock is held (chosen)** | a dropped descriptor is let go a moment later, by the drain | a release can end a Unix socket, which takes `TABLE` -- and drops happen under `TABLE` and `PAIRS`; the queue also flattens a chain of sockets each carrying the next into a loop instead of a recursion as deep as an attacker makes it | every path that can drop one must drain; a missed drain delays a release until the next drain anywhere (never loses it) |
| Release inline in `Drop` | immediate | simplest to read | deadlocks on `TABLE`, recurses without bound |
| No `Drop`, explicit release everywhere | -- | no hidden work | a forgotten release is a leak, silently |

| | What changes | For | Against |
|---|---|---|---|
| **A stream's descriptors as marks in `stream_socket`, under its own lock (chosen)** | bytes and descriptors move together | the offset a send's bytes land at and the mark are recorded atomically; a plain read and a marked read cannot disagree | `stream_socket` learns of a type from `ipc::passed` |
| A side table in `unix_socket` keyed by stream offset | `stream_socket` unchanged | -- | the offset and the bytes are under different locks, so a reader could take bytes whose mark it had not seen |

| | What changes | For | Against |
|---|---|---|---|
| **Collect garbage, as Linux (chosen)** | a socket sent to itself and closed is freed | the Linux behaviour programs and fuzzers assume; without it a loop of sends pins kernel memory without limit | a scan of every socket's queue on each close while any socket is in flight |
| Refuse to pass Unix sockets | no cycles possible | no collector | D-Bus, systemd's socket hand-over and fd-store pass sockets |
| Limit nesting depth only | -- | cheap | does not stop a one-step cycle (a socket sent to itself) |

| | What changes | For | Against |
|---|---|---|---|
| **One reference per object per process, the duplicate dropped at install (chosen)** | a descriptor received for an object the process holds shares that reference | `close` already releases only with the last descriptor; keeping a second reference would hold a pipe open after its last descriptor closed | the install must check and install in one step (it does) |
| A reference per descriptor | simpler install | -- | would change `close`, `dup` and fork's accounting everywhere |

**Not done** (`known-issues.md`): native programs cannot pass descriptors
(`A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS`); small differences from Linux
(`A-SCM-RIGHTS-DIFFERENCES`).

**Revisit** if the collector's scan shows in a profile -- Linux moved to an
incremental graph in 6.10 for the same reason -- or when the native door is
built (a `Passed` would then hold a native handle as well as an `FdEntry`).
