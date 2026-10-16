## D-Q3 — [D] Programs cannot share memory, message queues or named semaphores with each other. Where should the shared ones live? — Status: OPEN (raised 2026-09-26; your concerns of 2026-10-09 answered below, with a fifth option, E, now the recommendation)

**In short:** Unix programs cooperate through things they open by name: a
block of shared memory, a queue of messages, a named counter that makes one
program wait for another. Here each of those is private to the program that
opened it, so a database whose worker programs share memory (PostgreSQL works
exactly this way) cannot run. On 2026-10-09 you asked whether making them
files would be slow, said you dislike making things files that are not really
files, and asked whether it would make them depend on file permissions instead
of SlateOS's own permissions for sharing memory. A fifth option, **E** -- a
small program that keeps the names and hands out the kernel's own
shared-memory permissions -- answers all three, and is the recommendation now.

## Your three concerns, answered

1. **Speed.** Under C, the file is touched only when a program first opens
   the object. Every send, receive, wait and post after that is ordinary
   memory the programs share, with a kernel call only to put a program to
   sleep or wake one. That makes C the fastest of A, B and C: A is a kernel
   call for every operation, B a round trip through another program. E has
   C's speed.
2. **"Not really files."** Under C nothing ever reads or writes them as
   files, but they do appear in `/dev/shm` as files -- the part you dislike.
   E has no files at all.
3. **SlateOS's own permissions.** Native SlateOS programs keep the kernel's
   shared-memory permissions (capabilities: permissions a program holds as
   unforgeable tokens) and its message channels whatever is chosen. The Unix
   objects are, by the Unix standard's definition, one system-wide set of
   names under every option; the only question is what checks a name when a
   program opens it -- file permissions under C, the kernel under A, a
   service under B and E. Under E the program that opens a name is handed a
   kernel shared-memory capability after the check, so the object *is* one of
   SlateOS's own, like any other.

## The options

What exists now, all of it in the C library (libc: the library every program
links for these calls), per program:

| Family | Calls | State |
|---|---|---|
| POSIX shared memory | `shm_open` + `mmap(MAP_SHARED)` | a file under `/dev/shm`; shared mappings of a file are being built by lane A (§1566) |
| POSIX named semaphores (counters programs wait on) | `sem_open` | a table inside libc |
| POSIX message queues | `mq_open`, `mq_send` | a table inside libc: 8 queues of 32 small messages |
| System V (the older Unix interface for all three) | `shmget`, `msgget`, `semget` | tables inside libc (System V shared memory is the kernel's shared memory since 2026-10-07) |
| Locks placed in shared memory | `PTHREAD_PROCESS_SHARED` | the kernel's waits work across programs since 2026-09-27 |

| Option | *What changes:* |
|---|---|
| A. In the kernel, as Linux does | All six families work between programs; every operation is a kernel call. The kernel gains three new kinds of object. |
| B. In a service program that does every operation | All six work between programs; every send or receive is a round trip through the service. |
| C. In libc, over shared files -- glibc's own design for named semaphores | All six work between programs at memory speed; each object is a file under `/dev/shm`, and file permissions decide who may open what. |
| D. Leave it | Programs that use these only within themselves keep working; nothing can share them. |
| **E. A name-keeper service, with the work done in shared memory** | All six work between programs at memory speed, with no files: a small service maps each name to a kernel shared-memory capability, checks the opener's permission, and hands it the capability; everything after that happens in the shared memory, with the kernel's waits. The cost is the service itself. |

- **A** is simplest for programs to reason about, but every operation pays
  for a kernel call, and the design keeps the kernel to scheduling, memory,
  messaging primitives and permissions.
- **B** keeps the kernel small, but is the slowest: every message crosses
  two programs.
- **C** is fast and needs no new program, but its objects are files (your
  concern 2), and file permissions decide access (your concern 3).
- **E** is fast, has no files, and its objects are kernel capabilities; a
  program that crashes mid-update can leave one queue stuck, as under C and
  as a crashed lock holder can anywhere. Lane A suggested it; lane D agrees.

**Claude's recommendation:** **E.** It keeps C's speed and A's use of the
kernel's own permissions, without files. Its parts exist: kernel shared
memory and waits across programs. What is new is the name-keeper (a small
service, lane D's) and the C library's use of it.

**If never answered:** safe for everything in the tree today (nothing here
shares these between programs), but it blocks every port that does --
PostgreSQL, anything using `sem_open` between programs, daemons built on
message queues. It does not get worse on its own.

**Where it bites:** `posix/src/mqueue.rs`, `posix/src/semaphore.rs`
(`sem_open`), `posix/src/sysv_*.rs`, `posix/src/mman.rs` (`shm_open`); under
E, a new service in `services/`.
