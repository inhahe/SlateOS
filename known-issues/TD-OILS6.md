### TD-OILS6. `osh` `read` builtin: positive `-t N` timeout not honored (`-t 0`, `-u` resolved) — OPEN (low priority, target-blocked)

**Where:** `userspace/oils/src/interp.rs` (`builtin_read`).

**What:** `read` now supports `-r` (raw/no-escape), `-a array` (split into
an indexed array), `-p prompt` (prompt to stderr), `-s` (silent — no-op for
non-tty input), `$IFS`-aware field splitting (whitespace-vs-non-whitespace
IFS, last variable gets the raw remainder, backslash escaping without `-r`),
and — as of the latest change — `-d delim` (read up to an alternate
delimiter; `-d ''` ⇒ NUL), `-n N` (stop after N characters *or* the
delimiter, whichever comes first), and `-N N` (read exactly N characters,
ignoring the delimiter). `-d`/`-n`/`-N` use a byte-level `read_record`
helper (UTF-8-aware character counting) dispatched over the same
`StdinSrc`/`RedirPlan` sources as `read_line`, and set the exit status
correctly (0 on delimiter/count reached, 1 on a short read at EOF).
**`-u fd` — RESOLVED 2026-07-19.** `read -u N` (N ≥ 3) now reads from a
descriptor opened by `exec N< file` via the per-shell `open_fds` table (see
TD-OILS14); `-u 0` falls back to normal stdin, and an unopened fd is a
status-1 `read: N: bad file descriptor`.

**`-t 0` — RESOLVED 2026-07-20.** The separable non-consuming poll is
implemented via `input_available_now` (interp.rs): `read -t 0` returns 0 iff a
read would proceed without blocking (data queued, or the source is at EOF —
EOF counts as "available") and non-zero otherwise, assigning no variables and
consuming no input. Verified: `read -t 0 x < /dev/null` → 0, `read -t 0 x <<<
hi` → 0, and a following `read` still sees the un-consumed data. (On the
non-Windows/SlateOS build the underlying probe is the documented conservative
fallback — see TD-OILS-READ-T0-POLL: a non-tty inherited stdin is treated as
ready, an interactive tty as would-block, and only already-buffered bytes
count for a live upstream pipe.)

**Still missing: positive `-t N`** (wait up to N seconds for input, then time
out with a >128 status saving any partial input). Its option-argument is
parsed and validated (`invalid timeout specification` on a bad value) but a
positive value is otherwise ignored — the read blocks normally.

**Why deferred (target-blocked):** a correct positive timeout requires
per-byte deadline gating (bash times the *whole* read, not just the first
byte, and saves partial input on expiry). That gating needs a real
`poll(2)`/`select`-style "wait until readable *or* deadline" primitive on the
input fd. Only the Windows dev host currently has the non-consuming peek
(`handle_readable_now`); the **SlateOS target has no such probe wired** (see
TD-OILS-READ-T0-POLL), so its `stdin_readable_now` fallback reports an
interactive tty as *never* ready. Implementing per-byte gating against that
fallback would make an interactive `read -t 5 answer` on SlateOS *always* time
out (it would never observe the user's keystrokes as "ready") — strictly worse
than today's "ignore the timeout and read normally." So positive `-t N` is
correctly blocked on the same SlateOS readiness-poll infrastructure as
TD-OILS-READ-T0-POLL, and should be implemented together with it. Low priority
regardless — scripts use `-r`/`-a`/`-n`/`-d`/`-u`/`-t 0` far more than a
positive read timeout.
