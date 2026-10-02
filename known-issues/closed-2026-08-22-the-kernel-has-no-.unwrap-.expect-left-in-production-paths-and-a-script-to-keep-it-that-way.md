## CLOSED 2026-08-22 — the kernel has no `.unwrap()`/`.expect()` left in production paths, and a script to keep it that way

`CLAUDE.md` has always said "every `unwrap`/`expect` in non-test code is a
potential DoS if an attacker can shape the input." That was policy with no
instrument. `scripts/scan-unwrap.py` is the instrument: it classifies every
`.unwrap()`/`.expect()` site in `kernel/` as production or test and prints the
production ones as `file:line: [fn name] source`.

**It now reports 0, down from 71.** The removals are commits `4eba85262`,
`a7e393361`, `10f4f6b01` and `69021e7f6`; the reasoning is `design-decisions.md`
§282 (the fixes) and §283 (the classifier).

### If you are adding kernel code

Run `python scripts/scan-unwrap.py` before you commit. Zero is the expected
output; anything else is a new way to kill the machine. The exit status is 1
when it finds something, so it can gate a script.

Test code is exempt and correctly detected — a panic on bad data in a test is
the point. Detection walks a *stack* of enclosing functions, so a helper defined
inside a `self_test` is still test code (§283 explains why two earlier versions
got this wrong in opposite directions).

### What this does NOT cover, and is still open

Zero unwrap sites is not zero panics. Still present in kernel production paths,
and **not** measured by this script:

- `panic!` and `assert!`/`debug_assert!` called directly.
- Indexing and slicing (`a[i]`, `&s[a..b]`), which panic out of range.
- Unchecked arithmetic, which panics on overflow in debug builds.

The defensive clippy lints that would surface those report roughly **18 000**
warnings across `kernel/`, untriaged. That backlog is separate from the
userspace-crate tally earlier in this file and has not been worked.

Two smaller notes for whoever picks that up:

- The three fix shapes that make a panic *unrepresentable* (see §282) apply just
  as well to indexing: `.get()` returning `Option` is the "return the error"
  shape, but a fixed-size array plus a `const` assertion is the "delete the
  possibility" shape and leaves less behind.
- Do not bulk-`allow` the lints in kernel modules to clear the count. The
  userspace crates that did this are recorded above as a warning, not a model.

### 2026-08-22 — that 18 000 triaged, and two of the claims above corrected

The paragraph above is right that 18 000 warnings exist and wrong about what
they mean. `scripts/clippy-sites.py --by-context` now splits them by enclosing
scope (see §286), giving **9 751 production / 8 386 self-test / 24 test**.
Within production:

| lint | production | self-test |
|---|---:|---:|
| `arithmetic_side_effects` | 4 893 | 2 545 |
| `indexing_slicing` | 4 455 | 1 428 |
| `expect_used` | **0** | 3 509 |
| `unwrap_used` | **0** | 877 |
| `panic` | **3** | 21 |

So the second bullet of "what this does NOT cover" — direct `panic!` — was
never a large backlog. It is three sites, and all three were adjudicated:

- `main.rs:5882` — aborts the boot when syscall dispatch is broken under live
  filtering. Correct: a kernel that cannot dispatch syscalls should stop
  loudly, and this one shipped broken once already (stale `MAX_SYSCALL_NR`).
- `sync.rs:962` — a proven self-deadlock. Correct, and already carries a
  "## Why it panics" section arguing it: the alternative is spinning to the
  boot test's timeout with no indication of which lock was involved.
- `mm/kvspace.rs:238` — **fixed**, see below.

The remaining ~9 300 production sites are all `arithmetic_side_effects` and
`indexing_slicing`. That is still a real backlog and still unworked, but it is
one kind of work, not five.

**Do not prioritise it by count.** The per-file ranking puts
`kernel/src/proc/elf.rs` first by a wide margin, which reads as "the ELF parser
handles attacker-supplied executables and is the worst file in the tree". It is
not: every one of those sites is in a `build_*_elf` function that *constructs*
a synthetic ELF for a self-test, writing constants into a `Vec` of known size.
They are test fixtures that happen to live in a production module, and they are
now classified as such. Ranking by count points at the safest code in the file.

The criterion that matters is the one CLAUDE.md states — "a potential DoS if an
attacker can shape the input" — which picks out a different list: the
decompressors (`fs/zstd.rs`, `fs/xz.rs`, `fs/bzip2.rs`, `fs/sevenz.rs`,
`fs/compress.rs`), the wire parsers (`net/tcp.rs`, `net/tls.rs`, `net/ssh.rs`,
`net/firewall.rs`), `drm/edid.rs` (bytes supplied by whatever monitor is
plugged in), and the untrusted-image filesystems (`fs/fat.rs`,
`fs/ext4/driver.rs`). Start there.

**Correction 1: `kvspace::validate` was not the boot check it said it was.**
Its doc read "Call once at boot to catch configuration errors", and no boot
path called it — its only caller was this module's own `self_test`, so the
check ran when the self-test ran and never otherwise. The module's blanket
`#![allow(dead_code)]` meant nothing pointed out the gap. Every region it
compares is a compile-time constant, so the question was always a compile-time
one: it is now `const _: () = assert!(first_overlap().is_none(), …)`, which
fails the **build**. Verified by moving `FAULT_TEST` onto `PT_SELFTEST` and
confirming `error[E0080]: evaluation panicked`.

**Correction 2: "the exit status is 1 when it finds something" was false.**
`scan-unwrap.py`'s `main()` returned 0 unconditionally, so any caller written
as `if scan-unwrap.py; then` would have succeeded on a tree full of findings.
Nothing called it, so nothing noticed. Now it returns 1 on findings *and* is
wired into `scripts/boot-test.sh` as `check_production_unwrap`, next to the
user-access gate. Verified by injecting `Some(1u64).unwrap()` into
`kvspace::identify` and confirming exit 1, then removing it and confirming 0.

Those two are the same defect wearing different clothes: **a guard whose
documentation asserts an enforcement that nothing performs.** It is the third
instance in this file — `write_user_image`'s safety contract asserted what its
caller happened to arrange (§285) — and it is worth checking for directly,
because in all three cases the code read correctly and only the *wiring* was
missing. Two questions catch it: does anything call this, and does anyone
check what it returns?

### 2026-08-22 — sweep 1 of that backlog: `drm/edid.rs` 100 → 0, and six blanket allows with it

First file off the attacker-facing list above. `kernel/src/drm/edid.rs` parses
the 128-byte block a monitor volunteers over the DDC wire — a surface with real
CVE history in Linux — and it is now at **zero** `indexing_slicing` and
**zero** `arithmetic_side_effects` findings, with **no `#[allow]` left anywhere
in the file**. Whole-kernel total 18 173 → 18 122.

That second clause is the part worth reading twice. The file's clippy count
*already* read zero for `arithmetic_side_effects` before this work, because six
blanket `#[allow(clippy::arithmetic_side_effects)]` attributes sat on the
functions that do the arithmetic. **A zero produced by a suppression looks
exactly like a zero produced by a fix**, and only one of them survives the next
edit — a blanket allow on a function blesses whatever arithmetic a later change
adds *inside* it. Neutralising the six (temporarily, via
`#[cfg_attr(never_set, allow(…))]`) exposed 17 hidden sites. None was a live
bug — every one was provably in range given u8/u16/u32 widths — but "provably
in range" was an argument in a comment, not a property of the code.

**The fix shape, in two kinds.** They are genuinely different and were treated
differently rather than with one blanket rewrite:

| kind | example | fix |
|---|---|---|
| bound is *known* — a fixed offset into a fixed-size block | 19 `data[N]` reads, the four 18-byte descriptor slots, the fixture builder | put the bound in the **type**: `&[u8; 128]`, `&[u8; 18]`, `[u8; 128]`. Clippy does not fire on a constant index *or a constant range* into an array — verified empirically, it was the one thing I was unsure of when planning this |
| bound is *attacker-derived* — read out of the monitor's own bytes | the CEA data-block walk (`pos`, `length`, `dtd_offset`) | `get` / `checked_*` / `saturating_*`, with the guard folded **into** the operation |

Concretely, on the second kind: `parse_detailed_timing` had
`if htotal > 0 && vtotal > 0 { … if denom > 0 { numer / denom } else { 60 } }`
— a correct guard standing *next* to the division. It is now
`numer.checked_div(denom)…unwrap_or(60)`, where the check and the division are
one operation and cannot drift apart. Same move in `decode_manufacturer_id`:
`if c1 > 0 && c1 <= 26 { b'A' + c1 - 1 }` became `field.checked_sub(1)`, since
0 is both "not a letter" and the value that would underflow — one operation
decides both, and it replaced three copies of the guarded expression.

Two incidental finds, neither a lint issue:

- `EdidInfo::name_str` sliced `&name[..self.monitor_name_len]` on a **`pub`**
  field. The parser never sets it past 13; a caller building an `EdidInfo` by
  hand can. Now `get(..len).unwrap_or(name)` — a truncated name in the display
  path beats a panic.
- The self-test asked `modes.is_empty()` and then indexed `modes[0]`. Splitting
  one question into two checks is what forced the index; `modes.first()` in a
  `let … else` answers both.

**Fixture code counts too, and the array move is why.** `build_test_edid` built
into a `vec![0u8; 128]`, so ~48 constant-offset writes were flagged. Changing
one line — the `Vec` to `[u8; EDID_BLOCK_SIZE]` — cleared all of them, along
with the sixth blanket allow, which turned out to be dead afterwards. This is
the cheapest ratio in the sweep and the same lesson as `proc/elf.rs` above,
read the other way: those fixtures are not dangerous, but they are *free* to
fix when the container can carry its own length.

**Method note for the remaining files.** Grep the file for `#[allow(` before
believing its count, and re-measure after the change rather than assuming —
the count here moved 100 → 51 → 0 across three passes, and the middle number
was only visible because the paths in `--message-format=short` use backslashes
on Windows and my first `grep 'drm/edid.rs'` silently matched nothing. **A
grep that returns zero and a file that is clean are indistinguishable until
you check the total.**

Next on the list, unchanged: the decompressors (`fs/zstd.rs` 299, `fs/xz.rs`
144, `fs/compress.rs` 110, `fs/sevenz.rs` 92, `fs/bzip2.rs` 88), the wire
parsers (`net/tcp.rs` 182, `net/ssh.rs` 104, `net/tls.rs` 99,
`net/firewall.rs` 87), then `fs/fat.rs` 140 and `fs/ext4/driver.rs` 104.
