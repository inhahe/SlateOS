### TD-B-PTY-MASTER-CANNOT-BE-INHERITED-ACROSS-SPAWN -- FIXED 2026-08-24

**Where:** `posix/src/spawn.rs`, `build_fd_map`.

`SYS_PROCESS_SPAWN` takes an `fd_handle_type` per inherited descriptor and
has no value that names a pty end, so libc filters master fds out of the
inherited set -- the same treatment it gives `epoll`/`timerfd`/`inotify`
fds. For those three the filter is honest, because they are userspace-only
objects with no kernel identity to pass. For a master it is a lie of
convenience: the master *does* have a kernel identity, and the child simply
does not get it.

The slave needs nothing, and that is not luck: `kind_to_handle_type` maps
`PtySlave` to `CONSOLE`, `CONSOLE` resolves through `current_tty()`, and
`login_tty` has just made the pty the child's controlling terminal, so the
mapping is exact. The master is precisely the end that is *not* anyone's
controlling terminal, so no resolution rule could name it.

**What breaks:** a program where the *child* is the master holder --
`script -f` re-execing itself, a multiplexer that spawns a helper to drive
the pty, sshd's server side. The common shape (parent keeps the master,
child gets the slave) is unaffected, which is why this is a gap rather than
a hole in the feature.

**Proper fix:** kernel-side. An `fd_handle_type` for a pty end plus a
refcount bump in `SYS_PROCESS_SPAWN`. Note for whoever implements it that
this must *not* be spelled as a blind `SYS_PTY_DUP` -- see the next entry
but one for why libc's own `dup` does not call it either.

**Fixed 2026-08-24**, exactly as described. Lane A added
`fd_handle_type::PTY = 7` -- one constant for both ends, because `PtyHandle`
is `(tty_id << 1) | end` and a second constant would only create a place for
the two encodings to disagree -- and spawn dups through `pty::dup()`, which
refcounts the end. libc's side: the filter in `build_fd_map` is gone,
`kind_to_handle_type` maps `PtyMaster` to `PTY`, and the reverse direction
grew `handle_type_to_kind_for(handle_type, handle)`.

**The reverse direction was the part that could have been got wrong.** One
wire type names either end, so `handle_type_to_kind` -- which sees only the
type byte -- cannot decode it. Left as it was, a child would have rebuilt an
inherited master as a `File` and the file layer would have misread the handle
number, which is the precise hazard this entry warned about, merely relocated
from the parent to the child. It now reads the handle's low bit. Rebuilding a
master as a *slave* would have been worse still: the emulator's own keystrokes
would come back to it.

Three things did not change and are pinned by tests so they do not look like
oversights: `PtySlave` still travels as `CONSOLE` (exact, not approximate --
`login_tty` has already made it the child's controlling terminal); nothing on
this path calls `SYS_PTY_DUP`, and `dup` must keep not calling it, because
spawn takes one reference per `fd_map` entry; and the entry is ownership-gated
kernel-side, so a hand-built `fd_map` naming a master the caller does not hold
fails the whole spawn rather than being silently dropped.
