## §370 — Name quoting is a crate of its own, `userspace/quoting`, because 777 utility crates need it and none of them may depend on `coreutils`

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)
**Where it bites:** `userspace/quoting/` (new, was `userspace/coreutils/src/quote.rs`);
`userspace/coreutils/src/lib.rs` (`pub use quoting as quote;`);
`userspace/coreutils/Cargo.toml`; every one of the 777 utility crates under
`userspace/` that prints a diagnostic naming a file; `scripts/quote-names.py`
and pre-push gate 8, which is the thing that measured the problem.

**In short:** A file's name can contain a newline. So an error message that
prints the name raw — `cut: {path}: {e}` — lets whoever *named* the file write
extra lines into `cut`'s error output, and the fake lines look exactly like
real ones. The function that renders a name safely lived inside the `coreutils`
crate, and the other 777 utility crates in the tree do not depend on
`coreutils` — so a scan found 1798 diagnostics in them doing it the unsafe way,
each one a single call away from being fixed by a function they could not
reach. The only question was where to put the function. It moved into a small
crate of its own; `coreutils` re-exports it under its old name, so nothing that
already called it changed.

### The options

| | *What changes* |
|---|---|
| **A. Its own crate, `quoting`** (chosen) | Any utility crate can `quoting = { path = "../quoting" }` and fix its diagnostics; the workspace gains one small library crate. |
| **B. Leave it in `coreutils`; dependents import the bundle** | `btrfs`, `cups`, `flatpak` &c. each take a dependency on an 86-binary bundle to reach one function. |
| **C. Leave it in `coreutils`; the other 777 stay unfixable** | The 1798 sites can never be repaired; the pre-push gate can only stop new ones. |

### Why A

This is the same shape as §364 (`modechange`) and §322 (`ere`), and it is
decided by the same fact: **the callers are not all in `coreutils`, and cannot
be.** `userspace/btrfs`, `userspace/cups`, `userspace/flatpak` and 774 others
are separate binaries with their own crates. What is different here is only the
scale — `modechange` had four callers, this has 778 — and scale argues the same
way, harder.

B is the option worth arguing with, because two crates (`stat`, `oils`) already
do exactly that, so the precedent exists. It is wrong at 777. `coreutils`
depends on `ere`, `bignum`, `charwidth`, `modechange`, `sha2`, `pwdb` and
`localtime`; making `btrfs` depend on `coreutils` to reach `quotef_os` drags
all seven in behind it, and does it 777 times. It also points the dependency
graph the wrong way for `design.txt` §8, which wants one tool per crate: a
per-tool crate importing the bundle §8 retires is the exact edge
`design-decisions.md` §359 was written to avoid.

C is what "don't refactor, just gate it" amounts to, and it is the option the
pre-push gate quietly makes *tempting* — with the ratchet in place the number
stops growing, and it is easy to call that done. It is not: 1798 diagnostics
would remain permanently forgeable, in exactly the tools (`btrfs`, `parted`,
`losetup`, `mkfs`, `cups`) whose entire job is to take a path from the user and
report what went wrong with it.

### What it cost

Almost nothing, which is itself the argument. `quote.rs` referred to the rest
of `coreutils` in exactly two places, both **doc comments** — a link to
`getopt::Program::argmatch` and one to `xnum::strtol_fatal` — so the move was a
`git mv`, a crate-level lint header, and demoting those two links to plain
code spans. No function signature changed, no caller changed, and
`coreutils::quote::quotef_os` still resolves because `lib.rs` says
`pub use quoting as quote;`.

That the extraction was this cheap is worth recording for the next one: a
1743-line module with two doc-comment couplings was *already* a crate, and had
been for some time. The thing that kept it inside `coreutils` was that nobody
had asked who else needed it — which is precisely what the tree-wide scan
finally asked.

### Interaction with B-Q7

This move is **neutral on B-Q7 and cheaper under either answer**, which is part
of why it did not wait for one. If B-Q7 resolves as **A** (standalone crates
canonical, `coreutils` retired), `quoting` has to exist as a standalone library
anyway, and this is that work done early. If it resolves as **B** (`coreutils`
canonical), `coreutils::quote` keeps working untouched and the only residue is
one extra path entry in a `Cargo.toml`. There is no answer to B-Q7 under which
this has to be undone.

It does weaken one bullet of the B-Q7 write-up, and `open-questions.md` has
been amended to say so: "moving a tool out of `coreutils/src/bin` costs it the
only mechanical name-quoting check it has" was true when written and is now
only half true, since gate 8 holds every crate at *no worse than today* even
though only `coreutils/src/bin` is held at *zero*.
