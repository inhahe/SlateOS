## B-A-SURVEY-OF-FLAGS-WE-ACCEPT-AND-DO-NOT-HONOUR (lane B, 2026-09-13)

**In short:** after fixing three separate bugs in one day that were all the same
shape — the library said yes and did nothing — I went looking for the rest
instead of waiting to trip over them. There are at least nine more, and one of
them can corrupt a database.

### How the search was done

The three fixed today (argv over 64 KiB discarded, argv past 512 truncated,
`aio_resfd` accepted and ignored) were each documented **accurately**, in a
module's own `## Limitations` list, in the tone of a design note. So the search
was for that tone: 24 modules under `posix/src/` carry a Limitations section,
and their bullets were read for the tell — *accepted but*, *is ignored*,
*no-op*, *always succeeds*, *unenforced*.

This distinguishes the dangerous kind from the safe kind. "Not implemented,
returns `EINVAL`" is fine: the caller is told. "Accepted but ignored" means the
caller asked for something, was told it got it, and did not.

### What is there, worst first

| where | what is accepted and not done | what a caller loses |
|---|---|---|
| `fcntl_ops.rs` | `F_SETLK`/`F_SETLKW`/`F_GETLK` — advisory record locking | **two processes both hold the same exclusive lock.** Verified in the code, not just its comment: `F_GETLK` unconditionally writes `l_type = F_UNLCK` ("no conflicting lock") and `F_SETLK` returns success without taking one |
| `sysv_shm.rs` | `SHM_RDONLY` — accepted but unenforced | a read-only shared mapping is writable; a process that attached read-only can corrupt the segment |
| ~~`sysv_msg.rs`~~ | ~~`MSG_COPY` — "treated as a normal receive"~~ | **FIXED 2026-09-13.** `MSG_COPY` is now a real peek: `msgtyp` is a 0-based queue index in `seq` order, `IPC_NOWAIT` is required and `MSG_EXCEPT` refused (it selects by type, so one call cannot mean both), and the message stays queued for whoever it was sent to |
| `sysv_sem.rs` | `SEM_UNDO` — accepted but ignored | a process that dies holding a semaphore never releases it; everyone waiting deadlocks |
| ~~`pthread.rs`~~ | ~~`pthread_cancel`~~ | **NOT A DEFECT — row withdrawn.** The code is `pthread_cancel(_thread) -> i32 { errno::ENOSYS }`. It refuses, which is correct; only the module doc still said "accepted but never actually cancels" |
| `sysv_shm.rs` | `SHM_REMAP`, `SHM_RND`, caller-supplied `shmaddr` | the segment lands somewhere other than where it was asked to |
| ~~`ioctl.rs`~~ | ~~`TIOCSWINSZ` on Console fds~~ | **NOT A DEFECT — row withdrawn.** `handle_tiocswinsz` issues `SYS_PTY_SET_WINSIZE` for Console too (via `CTTY`). Its own function doc said so; the module's summary list did not |
| `syslog.rs` | `openlog` facility ignored | entries are filed under the wrong facility |
| `fts.rs` | `FTS_COMFOLLOW` partial; `FTS_LOGICAL` re-stat is a no-op | weakest of the set: `FTS_COMFOLLOW` **is** read (`fts.rs:952`), so this is a documented partial implementation rather than a flag ignored. Listed for completeness, not as a defect |

`fcntl_ops.rs` is the one that matters most and is not fixable here. SQLite —
which CPython links — uses POSIX advisory record locks as its **entire**
cross-process correctness mechanism. On a libc where `F_SETLK` always succeeds,
two SQLite connections both believe they have the write lock and interleave
writes into one file. It needs a kernel-side lock table keyed by inode, so it is
filed as `requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`.

### Second pass — the list is much shorter than it looked

Having withdrawn two rows, I went back and read the code behind the rest rather
than the bullet describing it. The honest triage:

| item | verdict |
|---|---|
| `F_SETLK`/`F_GETLK` | **Real and severe.** Verified in code. Filed to lane A; not fixable here |
| ~~`SEM_UNDO`~~ | **WITHDRAWN on a third look.** `sysv_sem` makes **zero syscalls** — the whole set lives in this process's static memory, as its own doc says ("Single-process only"). `SEM_UNDO` exists to stop a dead process wedging a semaphore *other processes* are waiting on, and there are no other processes sharing this pool. Ignoring it cannot cost anything |
| `MSG_COPY` | **Was real. Fixed** (see Progress) |
| `SHM_RDONLY` | Real gap, but **documented with its reason** — "we have no per-mapping permission machinery" — and **not enforceable in this design at all**: one static pool address serves every attacher, so a read-only and a read-write attacher cannot be told apart. Refusing the flag would punish correct callers, who pass it and never write |
| `SHM_RND`, `SHM_REMAP`, `shmaddr` | **Low.** Documented; `shmat` returns the address it used, and a correct caller uses the return value rather than assuming its hint was taken |
| `SHM_LOCK` / `SHM_UNLOCK` | **Not a defect.** "Accepted as no-ops; our memory is never swapped" — the guarantee is vacuously satisfied, which is the right answer, not a missing one |
| `openlog` facility | **Weak.** `do_syslog` writes to **stderr**; there is no daemon, so a facility has nowhere to be routed to. Nothing is lost that could have been delivered |
| `FTS_COMFOLLOW` | **Not a defect.** Read at `fts.rs:952`; a documented partial, not an ignored flag |

So the honest count is **one** open defect, not nine: `F_SETLK`, severe, out of
my lane and filed. Plus one fixed today.

**A third look removed the second one too.** `SEM_UNDO` fell to the same test
that saved me from "fixing" `SHM_RDONLY`: all three SysV IPC modules
(`sysv_sem`, `sysv_shm`, `sysv_msg`) make **zero syscalls** — against 54 in
`ioctl.rs` — so every one of them is process-local static memory, and each says
"Single-process only" in its own header. A flag whose purpose is to protect
*other processes* cannot be a defect where there are none.

And the limitation bites nothing today: the only match for SysV IPC across all
of `userspace/` and `services/` is `time_cmd.rs`, where `msgsnd`/`msgrcv` are
**`struct rusage` fields** — message counts reported by `getrusage` — not calls.
Nothing we ship uses SysV IPC at all.

**Why the first pass read as nine.** "Accepted but ignored" covers two very
different things, and the phrase does not distinguish them: a promise the
implementation *could* keep and does not, versus a request the design *cannot*
express an answer to, documented as such. Only the first is a defect. The three
bugs that started this sweep — argv, argv again, `aio_resfd` — were all the
first kind, and I generalised from them to every bullet that sounded similar.

**The method that would have worked** is the one used on the second pass and on
`F_SETLK` in the first: read the code, not the bullet. It is slower and it is
the only part of either pass that produced a claim worth acting on.

### Correction — the survey caught its own disease

**Two of the nine rows were wrong, and both for the reason the survey exists.**
The sweep was built by grepping *documentation* — the `## Limitations` lists —
and documentation is precisely the artifact this session has spent all day
proving unreliable. I hunted stale comments with a method that trusted comments.

- `pthread_cancel` does not accept-and-ignore. It is
  `pthread_cancel(_thread) -> i32 { errno::ENOSYS }` — an honest refusal, and
  the right answer.
- `TIOCSWINSZ` is not a Console no-op. It issues `SYS_PTY_SET_WINSIZE` for
  every terminal kind.

In both cases the **function-level** doc was accurate and the **module-level**
summary was stale. `handle_tiocswinsz`'s own doc even says the operation "now
goes to the kernel rather than being swallowed here" — the word *now* marking a
fix whose author updated the doc beside the code and not the list at the top of
the file. Both module docs are corrected in the same change as this note.

**So a Limitations list goes stale in both directions**, and the second is the
one I walked into: it can describe a defect as a design note (the original
finding, three instances), and it can describe a *fixed* defect as still
present. The first misleads a user; the second misleads the reader who believes
it — here, a survey row published with confidence.

The six rows that survive were re-checked against code rather than prose: the
flag's constant is defined and its only other occurrences are in tests. That is
a weaker claim than reading the logic, but it is falsifiable, and it is what the
remaining rows now rest on.

### Progress

`MSG_COPY` was taken first because it is the clearest case of the property that
makes one of these safe to fix on sight: **nothing can be relying on the broken
behaviour.** No caller benefits from a peek that destroys what it looked at, and
the flag's own name says so. The queue already carried a per-message `seq`, so
"the message at index *n*" was well-defined without inventing an ordering.

The rest still need that question asked one at a time. `F_SETLK` notably fails
it — programs run today *because* the lie lets them through.

### Why this is a survey and not nine fixes

Several of these are one-line refusals and could be changed today, and that is
exactly why they need thought rather than speed. Turning `F_SETLK` from a lie
into `ENOLCK` is honest, and it also breaks every program that currently runs
because the lie let it through — which on this image includes anything CPython
does with SQLite. **"Refuse instead of lying" is the right default and is still
a user-visible behaviour change**, so the ones with live callers want the
operator or a boot test, not a quiet commit at the end of a session.

The three fixed today were all cases where nothing could have been relying on
the broken behaviour: no program benefits from losing its arguments. That is
what made them safe to fix on sight, and it is the property to check before
fixing each of the nine.

### The sweep that did work, and its numbers

After the constant-based idea failed at 93% noise, a narrower one succeeded. The
signature is not *a constant nobody reads* — a libc exports those by the
thousand — but **a parameter we were handed and chose not to look at, in a
function that then reports success**:

| filter | count |
|---|---|
| exported `pub extern "C"` functions in `posix/src` | 1203 |
| …ignoring at least one `_`-prefixed parameter | 168 |
| …where that parameter is flag/mode-like | 17 |
| …**and the function returns success** | **13** |

That last row is what makes it usable. `mkfifo`, `mkfifoat`, `dbm_open` and
`open_by_handle_at` all ignore a mode or flag and are *correct* to: they return
`ENOSYS` after validating, so nothing was created and nothing was promised.

Of the 13 survivors, exactly **one** was a defect: `siginterrupt`, fixed. The
other twelve, each checked against code:

- `dlopen` returns null — nothing is loaded, so `RTLD_*` has nothing to affect.
- `mq_open`, `shmat` — single-process implementations, where a permission or
  attach flag cannot mean anything (the same reasoning that withdrew
  `SHM_RDONLY` and `SEM_UNDO`).
- `openlog`'s facility — `do_syslog` writes to stderr; there is no daemon to
  route to.
- the seven `__*_chk` fortify wrappers — the ignored parameter is the fortify
  *level*, and they always apply `maxlen.min(slen)`, the stricter of the two
  bounds. Ignoring a level while taking the strict path is safe by
  construction.

**One real defect from 13 candidates**, against nine claimed and one real from
the documentation sweep. The difference is entirely that this reads what the
code does with its arguments, and a doc comment reads what someone believed at
the time they wrote it.

### Why there is no gate for this, measured

The obvious follow-up is a standing check: **a flag constant that production
code never reads**. That is the mechanical signature behind every real instance
here — `aio_resfd`, `aio_rw_flags`, `MSG_COPY`, `SHM_RDONLY`, `SEM_UNDO` were
all defined, referenced in tests, and read nowhere else. It reads code rather
than prose, so it would not have made the mistakes the first pass made.

It was measured before being built, and it does not work:

    public integer consts in production code: 50008
    defined but never read in production:     46736

**93%.** The reason is structural and not fixable by tuning: `posix` is a
**libc**, and a libc's job is to export constants for *callers* to use. Not
reading `KEY_LEFTSHIFT`, `TCSANOW` or `EADDRINUSE` is the correct state for
almost every constant in the tree. The signature that identified five real bugs
by hand is, mechanically, the normal condition of the codebase.

What actually distinguished the real ones is narrower: the constant names a
flag **passed into a function we implement**, and that function does not branch
on it. Detecting that needs to know which constants are inputs to which
functions, which is not recoverable from the text. Noted so the next reader
weighs the same idea against the same number rather than building it.

### The general rule this session produced

A `## Limitations` bullet is a defect report whenever it describes something the
caller **asked for**. If the caller cannot express the request, a limitation is
just a boundary and is fine to document. The grep that finds them is the phrase
*accepted but*, and there are nine.
