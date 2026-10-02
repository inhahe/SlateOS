### [A] RESOLVED -- the leaf check's first real finding: `INOTIFY_TABLE` is not a leaf, and its documented lock order was unenforceable because of it -- 2026-09-17

**In short:** two locks have to be taken in a fixed order or the kernel can
deadlock. A comment says which order, and says no code takes them the other
way. The automatic checker that exists to verify exactly that could not see
this pair -- because the outer lock was declared as the kind of lock that
opts out of the checker. The order was right; nothing was checking it.

The leaf-claim check (dd-949) reported four sites on its first real boot, all
in `fs/notify.rs`: `take_notify_waiters` (330), `create_watch_owned` (394),
`read_events` (440), `close_watch` (481). Those are acquisitions of
`NOTIFY_WAITERS` / `WATCHES`, both `crate::sync::Mutex`. The outer holder,
found by reading the call paths rather than waiting for the report to name
it, is `ipc/inotify`'s `INOTIFY_TABLE`.

`inotify.rs`'s own module doc states the arrangement:

> `INOTIFY_TABLE` is held across calls into `crate::fs::notify` (whose
> `WATCHES` lock is itself a leaf). The ordering is therefore
> `INOTIFY_TABLE` -> `notify::WATCHES`, and no path takes them in the reverse
> order, so there is no cycle.

Every clause is accurate, and together they say `INOTIFY_TABLE` is **not a
leaf** -- held across another lock's acquisition is the definition. Yet it
was a `PreemptSpinMutex`, the type dd-70 reserves for locks nothing nests
inside. And `WATCHES`, which that same sentence calls a leaf, is the tracked
type. **The two lock types were the wrong way round.**

The consequence is not stylistic. *"No path takes them in the reverse order,
so there is no cycle"* is precisely the claim lockdep exists to verify, and
lockdep could not see this pair: `PreemptSpinMutex` does not register, so the
edge was absent from the dependency graph. A real lock order protected by a
comment, unenforced *because* the lock chose the type that opts out of
enforcement. dd-944, and `sync.rs`'s own sentence one level up -- "opting out
of one silently opted out of the other" -- generalised: opting out of
ordering checks for a lock that turns out not to be a leaf leaves the
ordering it actually has unchecked.

**The claim is true today, checked before changing anything.** `fs::notify`
only *mentions* inotify in comments; it never calls into `ipc::inotify`. The
`emit_*` entry points are invoked from `fs/handle.rs` and `fs/vfs.rs`,
neither of which holds `INOTIFY_TABLE`. A reverse acquisition would need
something holding `WATCHES` to take `INOTIFY_TABLE`, and no such path exists.

So this was never a live deadlock, and the fix is not a bug fix. What it
changes is *who* is responsible for the claim staying true: `INOTIFY_TABLE`
is now a named `crate::sync::Mutex`, which puts the documented edge into
lockdep's graph. The order stops depending on nobody adding a reverse path
later, and the module doc now says the clause is checked rather than
asserted.

Named rather than left as the default, for the reason recorded above: 563
locks in this kernel answer to `?`, so an unnamed lock in a violation report
identifies nothing.

**Worth noting what found it.** Not the instrument's report, which said
`"?" acquired while "?" is held` and named neither lock. The *site* was
enough to find the file, and reading the call paths did the rest. A report
that localises the problem to four lines is already most of the value even
when its identity half is broken -- which is an argument for shipping the
site, not an excuse for the `?`.
