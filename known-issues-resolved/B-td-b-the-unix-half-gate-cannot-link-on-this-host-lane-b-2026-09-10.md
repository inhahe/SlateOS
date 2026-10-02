## ~~TD-B-THE-UNIX-HALF-GATE-CANNOT-LINK-ON-THIS-HOST~~ (lane B, 2026-09-10) — FIXED the same day

**Fixed by `--no-test`.** The gate asks whether the *other* arm COMPILES.
Running the linux tests additionally requires a `cc` for
`x86_64-unknown-linux-gnu` to link with — which this machine lacks — and then
requires **executing a linux binary on Windows**, which is not possible at all.
So compile-only is not a weakening; it is the only thing this gate could ever
have meant here. `scripts/coreutils-check.sh --only linux --no-test` reports
`clean (linux half checked)` for `userspace/udevd`, and the udevd symlink arm
that this entry was blocking is restored.

**And a correction to this entry, which was on `main`.** It said the fix "is a
change to raise rather than make" because `scripts/coreutils-check.sh` is
"shared with the boot test". It is not: `scripts/boot-test.sh` references it
**zero** times. Its only readers are the push hook, two gate-wiring checkers,
and its own test suite — all lane B's. I asserted a lane boundary I had not
checked and used it to defer a fix I could have made immediately. Thirteenth
adjacent claim of the day and the first where the consequence was inaction
rather than a wrong number.

**Also fixed:** the refusal text's "Reproduce and iterate with" command omitted
`--no-test`, so following the hook's own instructions no longer reproduced what
the hook ran. That is the same shape as lane A's design-decisions-bands advice
pointing at a heading instead of a section end — a gate whose instructions do
not lead to the state it checked.

### Original entry



**In short:** pre-push gate 12 compiles the unix half of a changed crate by
building it for `x86_64-unknown-linux-gnu`. That target has no linker on this
machine — `error: linker \`cc\` not found` — so the gate fails whenever it has
something to check, and passes only when it has nothing.

**Found by adding a unix-gated arm to `userspace/udevd`**, which had none. The
crate became the gate's subject for the first time and the push was refused
with "udevd does not compile, or does not pass its tests". It compiles; it
cannot be linked here.

**Why this is worse than a broken gate.** The gate derives its scope from the
files a push changes, so it only builds a crate when that crate's unix arm was
touched. A gate that fails exactly when it has work and passes the rest of the
time is indistinguishable, from the tally, from a gate that is doing its job —
until somebody edits one of the arms it exists to protect. The corollary is
that **no unix-gated arm under `userspace/` has been compiled on this host**,
which is the thing gate 12 was written to guarantee.

**What it blocks right now.** `userspace/udevd` should create a real symlink for
`/dev/disk/by-uuid/<uuid>` rather than nothing; `posix::file::symlink` exists
and `userspace/backup` already has the pattern. The arm was written and
reverted rather than shipped behind a gate that would refuse it for an
unrelated reason. The decoy it replaced — a text file containing `-> sda1` — is
gone regardless, because a missing link is a visible failure and a text file
pretending to be one is not.

**The fix is a cross-linker**, not a code change: either a `cc` for
`x86_64-unknown-linux-gnu` on PATH, or teaching the gate to use a check-only
build where linking is not what it is verifying. The second is cheaper and
`cargo check` would answer the question the gate actually asks — does the other
arm compile — but `scripts/coreutils-check.sh` is shared with the boot test, so
that is a change to raise rather than make.
