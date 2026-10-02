## TD-B-TOUCH-CANNOT-STAMP-A-PATH-IT-CANNOT-OPEN (lane B, 2026-08-22) — OPEN, host-only

**In short:** `touch` sets a file's "last changed" date. On SlateOS it does that
by naming the file — which works even for things you are not allowed to *open*,
like a directory, a file whose permissions are set to deny everyone, or a
socket. On the Windows machine we develop on there is no way to set a date by
name; you must open the file first. So on that machine, and only on that
machine, `touch` fails on those few kinds of file. Nothing shipped is affected —
the real OS takes the good path — but it means a test for those cases cannot be
written on the dev machine, so none exists.

### Where

`userspace/coreutils/src/fsattr.rs` → `set_times`. **Moved there by `cf63fda74`**, “one path-based timestamp write, in `coreutils::fsattr`”; this entry said `touch.rs` → `stamp_path` until 2026-09-12, by which time neither the file nor the function was where it pointed. Found by `check-stale-blockers.py`'s third pass, which flagged this entry because `touch.rs` had moved under it — the pass's first catch after it was written. Two arms:

| Arm | How it stamps | Reaches |
|---|---|---|
| `#[cfg(unix)]` — **what ships** | `utimensat(AT_FDCWD, path, times, 0)` | everything a path can name |
| `#[cfg(not(unix))]` — the dev host | open a handle, `SetFileTime` | everything a handle can name |

### What the gap actually is

Measured on Linux 6.6 (glibc), running the create-open and the stamp in the
order `touch_one` does:

| Path | create-open | `utimensat` |
|---|---|---|
| ordinary file | ok | ok |
| a directory | `Is a directory` | ok |
| a file of mode 000 you own | `Permission denied` | ok |
| a FIFO with no reader | `No such device or address` | ok |
| a unix-domain socket | `No such device or address` | ok |

Four of those five cannot be opened in *any* mode — a mode-000 file refuses
`O_RDONLY` exactly as it refuses `O_WRONLY`, and `open` on a socket fails
outright. The unix arm handles all five. The Windows arm handles the first two
(a directory works there because the handle asks for `FILE_WRITE_ATTRIBUTES`
with `FILE_FLAG_BACKUP_SEMANTICS`); the other three have no Windows analogue
worth chasing, since Windows has neither a mode-000 file nor a unix socket.

### Why this is filed as debt rather than fixed

There is nothing to fix in the shipping code — it already does the right thing.
What is missing is **coverage**: the four interesting rows above are exactly the
rows the host cannot execute, so the host suite proves the *logic* around the
stamp (order of operations, which error is reported, `-c`/`-a`/`-m`/`-r`) and
not the stamp itself on those file types.

The C probe that produced the table above is the evidence that exists today. It
was run once, by hand, under WSL. That is better than reasoning and worse than
a test.

### What the correct fix looks like

A ring-3 self-test on the target, next to the existing `fastpy-settimes` one in
`kernel/src/proc/spawn.rs` (which already exercises `SYS_FS_SET_TIMES` end to
end). It should create the five paths above in a scratch directory, run the
shipped `touch` on each, and assert exit 0 and a moved timestamp for all five.
That is the only place all five can exist at once.

Trigger: whenever the next ring-3 coreutils self-test is written — do not build
a boot-test harness solely for this.

### What must NOT be done about it

Do not "fix" the divergence by making the unix arm open a handle too, so that
both hosts behave alike. That trades a correct program for a testable one: it
would break `touch` on four real file types on the only OS this is for, to make
the dev host's limitation universal. The asymmetry is the right outcome.
