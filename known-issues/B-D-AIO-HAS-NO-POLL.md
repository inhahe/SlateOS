### [D] B-D-AIO-HAS-NO-POLL — 2026-09-26 — OPEN

**Where:** `posix/src/linux_aio_abi.rs`, `prepare`.

**What it is.** `IOCB_CMD_POLL` (Linux 4.18) asks for a completion when a
descriptor becomes ready. It is refused with `EINVAL` -- fs/aio.c's own
answer, "same as no support for IOCB_CMD_POLL", which is what a program
probing for it (ScyllaDB's reactor does) takes as "use epoll instead". A poll
that is ready at once could complete at once; one that is not has to be
completed later, while the program may be in no call of ours at all -- it
may be waiting on the eventfd it asked to be signalled, or reaping the ring
itself -- so only something running beside the program can complete it.

**The proper fix.** A helper thread, started on the first pending poll: it
waits (`poll`) on every pending request's descriptor plus a wake-up eventfd,
and completes the ready ones into their rings, signalling their eventfds and
waking `io_getevents` as a completion does now. `io_cancel` then has
something to find -- a cancelled poll completes with `res` 0 and
`io_cancel` answers `EINPROGRESS` -- and `io_destroy` cancels its context's
polls before it waits. glibc runs its POSIX AIO the same way. Trigger: a port
that submits `IOCB_CMD_POLL` and has no fallback.
