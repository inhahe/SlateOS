## TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT (lane B, 2026-09-04)

**In short:** A SlateOS program that lists `posix` as a Rust dependency gets a
*second copy of the C library* compiled into it, and in that copy every single
syscall is replaced by a stub returning `-ENOSYS`. The real libc is still there,
linked in the normal way, still working. Nothing warns; the two copies simply
disagree. This was not a theory — it made `ssh` and `sshd` unable to run at all,
because both drew their secret keys through the broken copy.

**Fixed for the entropy calls (2026-09-04). The underlying trap is still open**
and will catch the next caller; the gate that would close it is proposed at the
bottom of this entry.

### The chain, every link measured rather than argued

| # | Link | How it was checked |
|---|---|---|
| 1 | The SlateOS program target says `target_os = "linux"` | `rustc +nightly --print cfg --target toolchain/x86_64-slateos.json -Zunstable-options` → `target_os="linux"`, `target_vendor="slateos"` |
| 2 | Every syscall in `posix` is gated `#[cfg(target_os = "none")]` | ~1,845 such gates; `target_os = "none"` is the *staticlib* build that produces `libc.a` |
| 3 | So a program's rlib copy of `posix` takes the host arm | `cargo check-slateos -p sshd` prints `Checking posix v0.1.0` — it is compiled fresh, for `linux`, not reused from `libc.a` |
| 4 | On that arm `syscallN` returns `HOST_ENOSYS` | `posix/src/syscall.rs:900` — `pub(crate) const HOST_ENOSYS: i64 = -38;` |
| 5 | `posix::random::fill` reads `-38` as "no kernel here" | `classify_refusal` (`random.rs:251`), `not(target_os = "none")` arm → `KernelFill::Absent` |
| 6 | …and falls through to its userspace pool | `pool_fill` (483) → `seed_material` (389) → `hw_word` (331) |
| 7 | …which needs RDRAND/RDSEED | `hw_rng_kind` (289): CPUID leaf 1 ECX bit 30, leaf 7 EBX bit 18 |
| 8 | The guest CPU has neither | `QEMU_CPU="qemu64,+smep,+smap,+umip"` (`boot-test.sh:6260`); feature set recorded at `known-issues.md:43677` — no RDRAND |

**Result:** `Err(EIO)`. `sshd` could not generate or persist a host key and could
not generate a Diffie-Hellman exponent; `ssh` could not generate its exponent or
its KEXINIT cookie. Neither program could complete a connection.

### Why it was invisible for so long

Three reasons compounding:

1. **The error blamed the wrong component.** The message read "cannot generate a
   host key: the kernel CSPRNG returned errno 5" — naming a kernel that had
   never been asked. A reader chasing that goes to `kernel/src/`, which is both
   another lane's tree and entirely innocent.
2. **The gate's own documentation omits this case.** `posix/src/syscall.rs`'s
   comment describes exactly two builds: `target_os = "none"` (the real
   staticlib) and "any host build … used by `cargo test` against the host
   triple". The third build — *the rlib linked into an actual SlateOS program* —
   is not mentioned, so the stub reads as test-only scaffolding.
3. **The correct route existed and looked identical at the call site.**
   `randrange` had a private `fill_from_kernel` going through the linked libc's
   `getrandom` symbol, and `ssh-keygen` had hand-written the same extern a third
   time. `posix::random::fill(&mut b)` and `randrange::fill_secret(&mut b)` are
   the same shape, the same length, and one of them is silently broken.

### Blast radius, measured

**11 crates** depend on `posix` as an rlib. Only two `posix::` module families
are actually named by any of them:

| Module family | Nature | Effect of the duplicate copy |
|---|---|---|
| `ed25519`, `crypt::*` | pure arithmetic over caller-owned buffers; no syscall, no global state | **harmless.** A second copy computes the same answer. |
| `random::fill` | reaches the kernel, and falls back to per-thread state on failure | **catastrophic**, per the chain above. 5 call sites, all now converted. |

This is why the fix is not a crate-wide `compile_error!` on `posix` for
non-`none` targets: `apps/lockscreen` (lane C) uses `crypt` and is unaffected in
behaviour, and red-lighting another lane's build over a harmless use is how a
gate gets deleted rather than obeyed.

### The fix that was applied

All five entropy draws now go through one public
`randrange::fill_secret(&mut [u8]) -> Result<(), EntropyError>`, which reaches
the kernel CSPRNG through the *linked libc's* `getrandom` C symbol:

- `userspace/sshd` — host key seed, OpenSSH `checkint`, DH exponent
- `userspace/ssh` — DH exponent, KEXINIT cookie
- `userspace/ssh-keygen` — its private `fill_random`/`getrandom` block deleted in
  favour of the shared one

Each crate's `Cargo.toml` now says in a comment that `posix` is depended on for
`ed25519` **only**, and why that limit exists.

### The fix that was considered and rejected

Widening posix's ~1,845 gates from `#[cfg(target_os = "none")]` to
`#[cfg(any(target_os = "none", target_vendor = "slateos"))]`, so the rlib copy
issues real syscalls.

**This would make the problem worse, not better.** The rlib copy has its own
`errno` cell, its own fd table, and its own per-thread block
(`perthread::current()`'s `not(target_os = "none")` arm is a `thread_local!`,
which is a *different* TLS slot from the linked libc's). Letting it issue real
syscalls converts a loud, uniform `-ENOSYS` into silent divergence between two
libcs that each believe they own the process — a file closed through one and
read through the other, an `errno` set in one and read from the other. A stub
that fails every call is a bad state; two live libcs disagreeing is a worse one.

**The rule this establishes: one libc per process.** Anything in `posix` that
touches the kernel, holds global state, or holds per-thread state is reachable
from a program only through the C ABI (the symbols in `libc.a`), never as a Rust
dependency. Pure computation over caller-owned buffers is exempt because it has
nothing to disagree about. Recorded as `design-decisions.md` §768.

### What is still open

**Nothing enforces the rule.** The next crate to write
`posix::something_stateful::call()` gets the same silent stub, and its author
gets the same misdirected error message. The proposed gate has two halves:

1. Fail when a crate outside `posix/` names a `posix::` module that is not on an
   allowlist of provably pure modules (currently `ed25519`, `crypt`).
2. Fail when an allowlisted module gains a reference to `syscall` or `perthread`
   outside `#[cfg(test)]` — because that is the moment an entry on the allowlist
   stops deserving to be there, and half a gate that trusts a stale allowlist is
   worse than none.

The second half is the part that matters: without it the allowlist is a claim
made once and never rechecked, which is the shape of every rule in this tree
that has quietly stopped being true.

**A third possibility worth recording:** `posix/src/syscall.rs`'s stub arm could
`compile_error!` when `target_vendor = "slateos"` — turning the silent stub into
a build failure at exactly the point of misuse, with no allowlist to maintain.
It was not done immediately because it fails the *whole* crate rather than the
offending call, so a crate using only `ed25519` would stop building. Worth
revisiting if the allowlist proves hard to keep honest.

**If it is never fixed:** the entropy bug is gone, so nothing is broken today.
The cost is paid by whoever next reaches for a stateful `posix::` API from a
program — as a failure that names the kernel, sits in another lane's tree, and
is not there.

### Lesson 300: an error that names a component nobody asked is worse than no error (lane B, 2026-09-04)

*First lesson taken from lane B's proposed band 300–399
(`requests/b-ac-lesson-numbers-have-the-disease-the-section-numbers-were-cured-of.md`),
rather than from 115. The band scheme is not agreed yet, so this is a
unilateral choice of number — but the alternative was the collision the
proposal predicts: B and C have now both written a 110, a 111, a 112 **and** a
113, and C has taken 114. Picking 115 would have made it five. If the proposal
is rejected the cost of this number is that it looks odd; if it is accepted the
cost of 115 would have been a sixth collision.*

`sshd` could not generate a host key. It said:

    cannot generate a host key: the kernel CSPRNG returned errno 5

Every word of that is a lie of attribution. The kernel CSPRNG was never asked.
The call went to an rlib copy of `posix` whose syscalls are stubbed to
`-ENOSYS`, which read that as "there is no kernel here", fell through to a
userspace pool, tried to seed it from RDRAND, and found the guest CPU has no
RDRAND. `EIO` is the truthful *code*; "the kernel CSPRNG returned" it is not.

**A reader who trusts that message goes to `kernel/src/`** — which is another
lane's tree, and entirely innocent. They find a correct `SYS_GETRANDOM` handler
with a passing self-test, conclude the message must mean something subtler than
it says, and are now debugging with a false premise they have just confirmed.
Silence would have sent them to the caller, which is where the defect was.

The message was not careless. It was written by someone who knew the call was
*supposed* to reach the kernel, and described the design rather than the run.
That is the general shape: **an error message states what the author believed
the code does, and is trusted as evidence of what it did.** It is the one class
of comment that a reader cannot discount as possibly stale, because it arrives
attached to a live failure.

The rule that follows: **an error may name only what the code on that path
demonstrably did.** `posix::random::fill` cannot know whether it reached a
kernel — that is precisely the fact in question — so the honest text is what it
now says, "the system random number generator is unavailable", which sends the
reader to *how this program reaches the generator*. That is the question whose
answer was wrong.

Cheap test, worth applying to any error text you write: **would this sentence
still be true if the call had been intercepted?** If not, it is naming a
component on faith. See `design-decisions.md` §768 and
`TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`.

### Lesson 115: a blanket `allow(dead_code)` turns "this subsystem is unused" into a fact nobody is told (lane C, 2026-09-04)

`apps/undelete` offers two scan modes, and the module doc describes the second
as "a deep scan mode for sector-by-sector file signature detection". The crate
has a `SignatureDetector` holding a table of twenty-odd file signatures -- JPEG's
`FF D8 FF`, PNG's eight-byte header, `%PDF`, `PK\x03\x04` with a `word/`
secondary for DOCX -- a `detect`, a `detect_best`, and a `scan_sectors` that
walks a buffer sector by sector reporting what it finds. Thirteen tests cover
`detect`. Every one passes.

Nothing used it. `RecoveryEngine` constructed a `SignatureDetector`, stored it in
a field, and never read the field. `scan_deep` returned a hard-coded list of ten
`(offset, kind, size)` triples. The whole table could have been wrong -- every
magic byte, every offset, the DOCX secondary -- and no screen would have
differed, because the deep scan's answers never passed through it.

**The compiler knew.** `field signature_detector is never read` is a warning
rustc emits by default, and it was switched off at the top of the file:

    #![allow(dead_code)]

One line, no reason recorded, and it is the only thing standing between this
defect and a build that names it. Removing it surfaced the field immediately,
along with `scan_sectors`, `detect`, and seventeen other functions with no
caller -- including the entire filter builder (`with_file_type`, `with_min_size`,
`with_min_confidence`, `with_search`...), which is four more of the module doc's
bullet points.

**The tests could not have caught it, and it is worth being precise about why.**
`test_jpeg_signature_detection` calls `detector.detect(...)` and asserts the
answer. That is a correct test of a correct function. It says nothing whatever
about whether the *program* calls it -- and no test of a component ever can,
because the question is about the component's callers, not the component. The
signal for "is this used" is the one the `allow` deleted.

**The repair is not to delete the detector.** It is to put it on the path it was
written for. There is no device to read here, so the simulation now *plants* the
sectors -- writing each file type's real magic bytes, taken from the detector's
own table, into a sector each -- and the deep scan reads them back through
`scan_sectors`. The finds are the detector's answers rather than the plan's, so
a wrong entry in the table now costs a file type in the results, which is the
same failure a real disk would produce. That is what makes the thirteen tests
worth having: they now fail *and* the program breaks.

**The rule:** a blanket `#![allow(dead_code)]` in a crate is a statement that
nobody wants to know which parts of it are connected. Prefer scoped
`#[allow(dead_code, reason = "...")]` per item -- as `apps/kanban` now has, 22 of
them -- so the next unused thing is still reported.


### Lesson 116: a fast path in front of a table becomes a second table, and the fast one wins (lane C, 2026-09-04)

`apps/netscan` resolves a port number to a service name. It had two ways to do
it:

* `service_database()` -- 125 entries, each with a port, a protocol, a service
  name and a description. The module doc advertises it: "service detection with
  100+ port-to-service mappings".
* `lookup_service(port)` -- a 57-arm `match`, introduced by this comment:

      // Use a static-like approach; match on common ports for O(1) lookup of
      // the most frequently queried ports, falling back to the database for
      // the rest.

It did not fall back. The `match` ended `_ => None`, and `service_database` had
no caller anywhere in the crate. So 68 of the 125 entries were unreachable: a
scan that found port 43 open showed `-` where the table said `whois`.

**The part worth remembering is not that it was incomplete.** It is that the
shadowing copy was *less accurate* than the table it shadowed. Where both knew a
port, they disagreed four times, and every disagreement lost information:

| port | the match said | the table said |
|---|---|---|
| 137 | `netbios` | `netbios-ns` |
| 139 | `netbios` | `netbios-ssn` |
| 67 | `dhcp` | `dhcp-server` |
| 68 | `dhcp` | `dhcp-client` |

Four services under two names. That is what a hand-written fast path drifts
towards: it is written from memory, in a hurry, for the "common" cases, and the
carefully-sourced table beside it is the one nobody reads.

The repair was to delete the `match` outright rather than make it agree.
125 entries is a few hundred bytes to scan, once per open port on screen; the
performance argument for the fast path was never measured and would not have
survived being measured. One table cannot disagree with itself.

**How to find the next one:** grep for a lookup function that returns a
`'static` answer and check whether the data structure it is named after has a
caller. A `match` with more than a dozen arms that duplicates a nearby table is
the shape. The test that now guards it walks the whole table and asserts every
entry is reachable through the lookup -- which is cheap, and which no test of
either half alone would have caught.

### Lesson 117: a surviving mutation sometimes means the code is redundant, not that the test is weak (lane C, 2026-09-04)

Twice this week a mutation survived and the correct response was to delete code
rather than to write a test.

* `apps/systemrestore`'s `advance_operation` ended a running operation two ways:
  a `progress.complete` check, and running out of frames. Deleting the first
  changed nothing observable, because the filmstrip's finished frame *is* its
  last one -- so the second condition always fired on the same tick.
* `apps/netscan`'s `start_scan` already reset both scroll offsets at the end,
  with a comment explaining why and a test covering it. I added a second reset
  at the top without reading that far. The mutation that deleted mine survived,
  because the real one was still there.

The reflex on a survivor is "my test is too loose". Check the other possibility
first, because it is quicker to check and the fix is better: **if removing a
line changes nothing, ask whether some other line is already doing the job.**
The tell is a survivor on code you *just wrote* to fix something -- if the
behaviour was already correct, the thing you added is a duplicate, and the
duplicate is the defect.


### Lesson 118: a witness the code already satisfies is not a witness (lane C, 2026-09-04)

`apps/tmux` gained copy and paste. The test read:

```rust
prefixed(&mut mux, 'y');              // yank
let copied = mux.clipboard.clone();
prefixed(&mut mux, ']');              // paste
assert!(pane_text(&mux).contains(copied.trim()));
```

It passes with the paste deleted. The text was *yanked from that pane*, so it
is on that screen either way; the assertion asks whether some words are visible
and they were visible before the operation under test ran.

The repair is to paste somewhere the text cannot already be — a freshly split
pane, which starts empty — and to assert that it starts empty first, so the
premise is checked rather than assumed:

```rust
prefixed(&mut mux, '%');
assert!(pane_text(&mux).trim().is_empty(), "or this proves nothing either");
prefixed(&mut mux, ']');
assert!(pane_text(&mux).contains(copied.trim()));
```

This is the same family as lesson 54 (a fixture where two quantities happen to
be equal) and lesson 58 (a witness more than one rule explains), and it has a
sharper tell than either: **the test's subject and its witness were the same
object.** Copy took text *out of* the pane and paste put text *into* it, and
both ends were checked against the same screen. Whenever a round trip is tested
by looking at where it started, the return leg is untested.

The general check, cheap enough to apply to every test that asserts something
is present: *ask what the assertion would say if the operation were deleted.* If
the answer is "the same thing", the assertion is about the fixture.

### Lesson 119: an unreachable-pattern warning is a keybinding collision (lane C, 2026-09-04)

Adding a copy-mode mark to `apps/tmux`, I bound it to Ctrl+B Space. The build
said:

    warning: unreachable pattern
      --> apps/tmux/src/main.rs:2404:13
       |
    2365 |             ' ' => {
       |             --- matches all the relevant values

Ctrl+B Space was already `next-layout`, forty lines further down the same
`match`. My arm came first, so the existing feature became unreachable —
silently, from a user's point of view: the layout key simply stopped working.

Worth writing down because of what the warning is *usually* taken to mean. An
unreachable pattern reads like a tidiness complaint about a redundant arm. In a
keymap it is a **regression report**: some binding that used to work no longer
does, and the compiler knows exactly which one and where. In a `match` on a
`char` with fifty arms spread over a hundred lines, it is the only thing that
knows.

Two consequences for this tree:

* Never `#[allow(unreachable_patterns)]` on a keymap. The one place the warning
  is most often suppressed as noise is the one place it carries the most.
* When adding a binding, read the whole `match` first — or add it and let the
  compiler check, which is what happened here and is why the collision cost a
  minute instead of a bug report. `v` was the right key anyway: it is what
  tmux's own vi copy mode uses for begin-selection.


### Lesson 120: a failed command followed by "already up to date" is the shape of a silent no-op (lane C, 2026-09-04)

Merging lane C to `main` printed this:

    fatal: this operation must be run in a work tree
    fatal: this operation must be run in a work tree
    fatal: this operation must be run in a work tree
    5d2f40fb9 Merge remote-tracking branch 'origin/main' into lane-a
    Everything up-to-date

Every line after the `fatal`s is *true and reassuring*. `git log` printed a real
commit. `git push` reported that the remote already had everything — which it
did, because the merge that would have given it something new had not run. A
chain of `cmd; cmd; cmd` reports success at the end whether or not the middle
happened, and the last line is the one that gets read.

The cause was `core.bare=true` on the shared repository, left over from an
incident earlier the same day: `scripts/quote-names.py`'s self-test built a
fixture with `git init` under `-C <tmp>`, and **git exports `GIT_DIR` into every
hook**, which outranks `-C` outright -- so inside `pre-push` the fixture *was*
the repository. Repaired there with `env=gitenv.clean_env()`; the identical bug
in `check-requests-not-deleted.py` was repaired on 2026-08-29. Two scripts, the
same trap, five days apart. `scripts/gitenv.py` exists to be the one way to
avoid it and its docstring is the post-mortem.

**The lesson here is not about `GIT_DIR`, which is well covered.** It is that
the damage was invisible at the point it mattered. What should have caught it is
a check on the *postcondition* rather than the command:

    before=$(git rev-parse main)
    git merge --no-edit lane-c
    test "$(git rev-parse main)" != "$before" || { echo "merge did nothing"; exit 1; }

"Did `main` move?" is a question with an answer. "Did `git merge` succeed?" is a
question whose answer gets buried under three later lines of output that are
about something else.

The general form, for anything driven by a sequence of shell commands: **assert
the state you wanted, not the exit code of the step you hoped would produce
it.** This is the same discipline as lesson 118 -- a witness the code already
satisfies is not a witness -- arriving in a shell instead of a test.
