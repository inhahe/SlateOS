## B-INOTIFYWAIT-REPORTS-FILESYSTEM-EVENTS-THAT-NEVER-HAPPENED (lane B, 2026-09-12)

`userspace/inotify` provides `inotifywait` and `inotifywatch`. Both call
`generate_simulated_events`, which invents them:

    fn generate_simulated_events(paths: &[String], mask: u32) -> Vec<InotifyEvent> {
        // Simulate common filesystem events.
        if mask & IN_CREATE != 0 {
            events.push(InotifyEvent { name: "newfile.txt", .. })

It is not a demo mode. It is called from the main path of both
personalities, at lines 527 and 654, so

    inotifywait -e create /watched

reports that `newfile.txt` was created, immediately, whatever is or is not
happening in that directory.

**This is the invented-answer class at its worst so far.** `blkzone` and
`systemd-cgtop` fabricated *readings*, which mislead a reader.
`prlimit` fabricated an *action*, which misleads a program. This
fabricates an *event*, which makes a program act. The canonical use is

    while inotifywait -e create /watched; do rebuild; done

and against this implementation that loop never stops rebuilding.

There is also a test, `test_generate_simulated_events`, asserting the
invented events -- the third test in this tree found pinning a
fabrication in place, after `systemd-cgls` and `systemd-cgtop`.

### Why this one is implementable rather than deletable

Unlike `blkzone`, which needed ioctls this build does not issue, **the
kernel already has inotify**. `kernel/src/ipc/inotify.rs` implements it and
`kernel/src/syscall/linux.rs` registers the calls at the standard numbers:

| call | number |
|---|---|
| `inotify_init` | 253 |
| `inotify_init1` | 294 |
| `inotify_add_watch` | 254 |
| `inotify_rm_watch` | 255 |

So §1006's "delete every fabricating command" does not settle it: that rule
is for commands that cannot work, and this one can. The userspace side was
simply never wired to the kernel side that exists.

### What the fix is

`init1`, `add_watch` per path, then read the fd and parse the event stream.
The syscall layer follows the pattern already in `userspace/arp` and
`userspace/prlimit` -- real `syscall` instruction under
`target_vendor = "slateos"`, ENOSYS on a development host, and the tool
saying it cannot watch rather than inventing something. The event buffer
itself is `struct inotify_event { wd, mask, cookie, len, name[len] }`,
which is a pure parse over bytes and is where the tests belong: variable
length names, NUL padding, and a buffer holding several events.

**Not started.** Recorded now because the diagnosis is complete and the
defect is worse than the ones already fixed today; the implementation is
more than a single change.
