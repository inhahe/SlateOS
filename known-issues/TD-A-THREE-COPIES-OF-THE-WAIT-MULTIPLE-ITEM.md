## TD-A-THREE-COPIES-OF-THE-WAIT-MULTIPLE-ITEM (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**In short:** three of lane A's ring-3 programs each carry their own copy of
the 24-byte record `SYS_WAIT_MULTIPLE` takes, and of the kind numbers and
`poll(2)` bits that go in it, copied by hand from a kernel doc comment. A
change to the kernel's layout would have to be found and made three times
more, and nothing would say a copy was missed.

**Where.** `linuxdrv/kit/src/wait.rs`, `services/devprobe/src/lib.rs` and
`services/netstack/src/wait.rs` (`WaitItem`, `KIND_CHANNEL`,
`KIND_SERVICE`, `POLLIN`, `POLLNVAL`); the kernel's own `WaitItem` is
private to `kernel/src/syscall/`, its layout stated in
`SYS_WAIT_MULTIPLE`'s doc in `kernel/src/syscall/number.rs`.

**The proper fix.** One definition of the syscall ABI's records and
constants that the kernel and every program build against -- a small
`no_std` crate of the kernel's syscall interface (the record, the kind
numbers from `cap::ResourceType`, the event bits), the kernel's
`WaitItem` itself among its users, with a size and offset assertion the
compiler checks. Found by the review of `bed8d16dc` (Opus and Fable, both),
which added the third copy.
