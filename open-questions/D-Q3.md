## D-Q3 — [D] Programs cannot share memory, message queues or named semaphores with each other. Where should the shared ones live? — Status: OPEN (raised 2026-09-26)

**In short:** Unix programs often cooperate through things they open by name:
a block of shared memory, a queue of messages, a named counter that makes one
program wait for another. Here each of those is private to the program that
opened it — two programs opening the same name each get their own — and the
system cannot yet give two programs the same writable memory at all. So a
database whose worker programs share memory (PostgreSQL works exactly this
way) cannot run, and a message one program queues is never seen by another.
Making them shared needs a home outside any one program; which home?

**What exists now**, all of it in the C library (libc: the library every
program links for these calls), per program:

| Family | Calls | State |
|---|---|---|
| POSIX shared memory | `shm_open` + `mmap(MAP_SHARED)` | a file under `/dev/shm`, but the kernel refuses writable shared file mappings (`ENOSYS`, design-decisions §23) |
| POSIX named semaphores (counters programs wait on) | `sem_open` | a table inside libc |
| POSIX message queues | `mq_open`, `mq_send` | a table inside libc: 8 queues of 32 small messages |
| System V (the older Unix interface for all three) | `shmget`, `msgget`, `semget` | tables inside libc |
| Locks placed in shared memory | `PTHREAD_PROCESS_SHARED` | refused (`ENOTSUP`): the kernel's futex (its wait/wake primitive) cannot wake across programs yet — requested of lane A |

**The question.** Every option below first needs the kernel to share writable
memory between programs — anonymous and file-backed `MAP_SHARED` (a mapping
two programs see the same bytes through). That part is lane A's and not in
question. What is in question is where the *named objects* live:

| Option | *What changes:* |
|---|---|
| **A.** In the kernel, as Linux does | All six families work between programs; each call is a kernel call. The kernel gains three new kinds of object. |
| **B.** In a service program (an `ipcd`) | All six work between programs; libc asks the service over the system's message channels, so every send or receive costs a round trip through it. The kernel gains nothing beyond shared memory. |
| **C.** In libc, over shared files — glibc's own design for named semaphores | All six work between programs; each object is a file under `/dev/shm`, mapped into every program that opens it, waits sleep on shared futexes. No new kernel objects and no service; file permissions decide who may open what. A program that dies halfway through an update can leave that one queue stuck, as a crashed lock holder can anywhere. |
| **D.** Leave it | Programs that use these only within themselves keep working; nothing can share them. |

**If never answered:** safe for everything in the tree today (nothing here
shares these between programs), but it blocks every port that does —
PostgreSQL, anything using `sem_open` between programs, daemons built on
message queues. It does not get worse on its own.

**Claude's recommendation:** **C.** It is how glibc already builds named
semaphores, it keeps the kernel as small as the design asks (only scheduling,
memory, IPC primitives and capabilities in the kernel), and its only kernel
needs — shared writable memory and cross-program futexes — are needed by every
option anyway. A stays available for System V message queues if a port turns
out to need kernel-side behaviour C cannot give. Meanwhile lane D keeps the
single-program versions correct.

**Where it bites:** `posix/src/mqueue.rs`, `posix/src/semaphore.rs`
(`sem_open`), `posix/src/sysv_*.rs`, `posix/src/mman.rs` (`shm_open`); lane A:
`kernel/src/mm` (shared mappings) and `kernel/src/ipc/futex.rs`
(`requests/d-a-futexes-keyed-by-physical-page-for-process-shared-objects.md`).
