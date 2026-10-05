## 729. One kernel-error → errno table, and a test that reads the kernel's enum, rather than two hand-maintained mirrors and a comment saying they must agree

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** the kernel numbers its errors (-500 is "not found", -700 is
"connection refused"). C programs expect a different set of numbers, called
`errno`. Something has to translate, and that something was two separate
hand-typed copies of the kernel's list, one in `errno.rs` and one in
`socket.rs`. Both had fallen behind — nine kernel errors were missing from the
first and eighteen from the second — and neither could tell, because both end
with "anything I don't recognise is an I/O error", which is also a legitimate
answer. The decision is to keep one table, make the socket one delegate to it,
and add a test that reads the kernel's source and fails if a number is missing.

### The problem

`posix/src/errno.rs` carried:

```
/// MUST stay in sync with kernel/src/error.rs — any mismatch causes
/// wrong errno values throughout the entire POSIX layer.
```

That is a correct and urgent statement with no mechanism behind it. Lane A
added `CrossDevice` (-512), `StaleHandle` (-513) and the seven-code `-700`
network range at various points; none reached this file. `socket.rs`'s
`translate_net_error` was worse — a second transcription that had never
included the network range *at all*, despite that range existing solely for
sockets and each of its kernel doc comments naming the errno it is for.

The observable cost: a non-blocking `connect` that is still handshaking
returns `EINPROGRESS` on every other Unix and returned `EIO` here. A client
library reads the first as "call `poll` and check `SO_ERROR`" and the second
as "this socket is finished". `bind` to a taken port said `EIO` instead of
`EADDRINUSE`; `mv` across a mount point would have said `EIO` instead of the
`EXDEV` it reads to decide it must copy-then-delete.

### The options

**A. Update both tables and leave the comment.**
*What changes:* the nine errnos become right today, and the next code lane A
adds is wrong again on the same schedule.

**B. Update both tables; add a test that pins the current mappings.**
*What changes:* the nine become right, and a regression that *alters* a row is
caught. A row that is *absent* is still not caught — the test would enumerate
what the table already says, which is the thing that was never the problem.

**C. One table; a test that reads `kernel/src/error.rs` and requires every
discriminant to be accounted for.** (Chosen.)
*What changes:* adding an error to the kernel and not to this file becomes a
red `cargo test -p posix`, naming the variant and its number.

**D. Generate the table from the kernel enum at build time, or share a crate.**
*What changes:* drift becomes structurally impossible rather than tested-for.

### Why C rather than D

D is the better shape and is not available. The kernel crate is `no_std` and
builds only for `x86_64-slateos`; `posix` builds for the host as well, because
that is how its 20,000 tests run. Making `posix` depend on `kernel` would drag
a kernel build into every host test run; a `build.rs` that parses the enum
would move the same text-scraping into the build graph, where a failure is a
build error with worse diagnostics and no way to say "this code deliberately
never crosses the syscall boundary". A third crate holding the enum is the
genuinely right answer and is a cross-lane change to a file lane A owns; it is
worth filing if the enum keeps moving, and is not worth blocking nine wrong
errnos on today.

C keeps the check in the place a lane-B reader will see it fail, and its
failure message says exactly what to type.

### Why one table rather than two updated ones

`translate_net_error` differed from `translate` in two rows. Only one of them —
`AlreadyExists` → `EADDRINUSE` — was actually about sockets: the only thing a
socket call creates by name is a local address binding, so `EEXIST` is never
the right answer there. The other, `InvalidCapability` → `EACCES`, was not a
socket decision at all; it was simply present in one table and absent from the
other, which is what drift looks like from the inside. Two tables that differ
in one deliberate row and one accidental row cannot be told apart by reading
them.

So `translate_net_error` is now that one row plus a delegation, and keeps its
name and its ~30 call sites. The cost is one extra call in the socket error
path, which is not a hot path — it runs once per failed syscall.

### The part that is easy to get wrong

The accounting test accepts a *comment* as accounting: `// PageFault = -102
(not typically returned to userspace)` satisfies it, because the bar is that
someone looked at the number, not that they mapped it. That generosity had a
hole. Section dividers read `// --- Filesystem (500 range: -500 to -513) ---`,
so a divider permanently satisfies the codes at its own endpoints — and a
range grows at its end, so those are exactly the codes most likely to be new.
Deleting `STALE_HANDLE` and watching the test pass is how this was found.
Dividers are now excluded from the searched text; nothing else is.

The general form: a sync check that has never been made to fail is not known
to be a sync check. Both tests here were run against a deliberately broken
tree before being believed.

### Where

`posix/src/errno.rs` (`mod native`, `errno_for`, `translate`, and the two
accounting tests at the end of the file); `posix/src/socket.rs`
(`translate_net_error`). Written up as
`B-NINE-KERNEL-ERROR-CODES-REACHED-USERSPACE-AS-EIO` in `known-issues.md`.
Answers the errno half of
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`.
