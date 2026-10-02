## B-EVERY-PROGRAM-HAD-ITS-OWN-MONOTONIC-CLOCK-AND-HALF-GOT-THE-UNIT-WRONG (lane B) — FIXED 2026-09-05

**In short:** Two live defects in one hour — `sshd` refusing every login and
`inetd`'s flood limiter never firing — were the same mistake, made independently
in programs that share no code: a number read from the kernel's monotonic clock
was carried around in a plain `u64` whose *unit* was recorded only in the
variable's name, and the name was wrong. Fixing both instances left the cause
untouched, because nothing stopped the third program from doing it again. This
entry records the structural fix: a tiny crate, `monoclock`, in which a clock
reading and a span of time are distinct types, so a mismatched unit is a
compile error rather than a comment nobody reads.

### The audit

Every lane-B program that reads `SYS_CLOCK_MONOTONIC` was checked. Four
hand-rolled wrappers, and the split is the interesting part:

| Program | Wrapper | Verdict |
|---|---|---|
| `userspace/sshd` | `clock_monotonic_ms` | **Wrong.** Login grace compared µs against seconds; no client could authenticate. |
| `userspace/inetd` | `clock_monotonic_ms` | **Wrong.** The "60 second" rate window was 60 µs; `MaxRate` was never exceeded. |
| `userspace/dig` | `clock_monotonic_us` | Misnamed but harmless — the value was only hashed. Deleted, along with the hash it fed (see the entry above; that turned out to be a security defect for an unrelated reason). |
| `services/init` | `clock_monotonic` | **Correct.** |
| `services/ticker` | `clock_monotonic` | **Correct.** |

The two that are correct are correct for a reason worth stating: `init` and
`ticker` suffix *every* variable, field and constant `_ns` — `started_at_ns`,
`backoff_ns`, `BACKOFF_MAX_NS`, `uptime_ns` — and never once claim a unit the
syscall does not return. That is discipline, and it worked. It worked in the two
programs whose authors happened to keep it up, and failed in the two that did
not. Discipline that must be re-applied at every one of sixteen call sites is not
a defence; it is a probability.

### The fix

`monoclock` (workspace member; `no_std`, no dependencies, no `alloc`, every
method `const`):

- `Instant` — a reading of the monotonic clock. Constructed only by
  `Instant::from_nanos_since_boot`, a name long enough that a reader can check
  the claim against `kernel/src/syscall/number.rs` without leaving the line, and
  which should appear exactly once per program, immediately around the syscall.
  `Instant::BOOT` names the clock's zero.
- `Elapsed` — a span, constructed from a figure in an explicit unit
  (`from_secs`, `from_millis`, `from_micros`, `from_nanos`) and read back the
  same way. Configuration files and RFCs are written in seconds; the conversion
  happens once, in a constructor named for the unit it is given.
- The only arithmetic offered is `saturating_since` (difference of two instants),
  `saturating_add` and `saturating_sub` (offset an instant by a span). There is
  no `Instant + u64` and no `Elapsed` from a bare integer, so
  `now.saturating_sub(60_000)` — the exact shape of the `inetd` bug — does not
  compile.

Saturation, not wrapping and not panicking, in all three: the callers are
timeouts and rate windows that unauthenticated peers can reach, so a backwards
clock step must report "no time passed" rather than a 584-year span, and a window
reaching back past boot must cover everything rather than nothing.

`sshd` and `inetd` are converted; their wrappers now return `Instant` and every
downstream field, parameter and constant holds `Instant` or `Elapsed`.

### Why the crate does not issue the syscall

The obvious design — one `monoclock::now()` that makes the syscall — was
rejected, and the boundary drawn at *what the number means* instead of *where it
came from*, for three reasons:

- **The callers are heterogeneous.** `init` and `ticker` are bare-metal
  `no_main` binaries; `sshd` and `inetd` are `std` programs. A crate that
  contains the inline `syscall` asm cannot be used off-target at all, so the
  `std` daemons' host tests could not link it — and those tests are the only
  tests that exist for this code.
- **Callers legitimately disagree about failure.** `sshd` returns `Option` and
  refuses the connection when it cannot read the clock, because a grace timer it
  cannot measure must not be treated as satisfied. `inetd` substitutes
  `Instant::BOOT`, because its only consumer is a sliding window and a window
  that contains everything over-counts, which fails towards rejecting a flood
  rather than admitting one. Both are right; a shared `now()` would have to pick
  one.
- **`posix` cannot be the home for it.** Depending on the `posix` rlib links a
  second libc in which every syscall is `#[cfg(target_os = "none")]`-gated out
  and answers `-ENOSYS` — see
  `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`.

So each program keeps its four-line wrapper and its own error policy, and hands
the result straight to `Instant::from_nanos_since_boot`. What is centralised is
the part that was actually going wrong: the unit, and the arithmetic done in it.

### The general lesson

**A unit carried only in an identifier is a comment the compiler does not read.**
Renaming `now_ms` to `now_ns` fixes today's bug and leaves tomorrow's available;
the name is checked by nobody, at no call site, ever. Where a value's meaning
constrains what may legally be done to it — a unit, a coordinate space, a
character encoding, a trust level — that meaning belongs in the type. It costs a
newtype and buys the whole class.
