# Deferred Questions — not answerable yet

Decisions that will eventually need the operator, but **cannot be answered
usefully today** because the evidence or the prerequisite does not exist yet.

They are here rather than in `open-questions.md` because that file is the
operator's *decision queue* — a list of things they can act on now. A question
that says "do not decide this yet" does not belong in a queue; it pads it, and a
padded queue trains the reader to skim.

**Every entry must carry a `Trigger:` line** — the concrete event that makes it
answerable. When that event happens, whoever notices moves the entry back into
`open-questions.md` (refreshed with whatever the evidence turned out to be) and
deletes it here. An entry without a trigger is either a real open question or
dead; it is never deferred.

This file is distinct from:

- **`open-questions.md`** — decisions the operator can make **now**.
- **`design-decisions.md`** — decisions already made.
- **`known-issues.md`** — bugs and technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Same per-lane rules as the other shared documents: append your own entries,
don't rewrite another lane's, and merge `origin/main` before trusting what you
read here.

---

## D-Q1 — Once a fastpy utility is proven as good as the Rust one, which does a stock install run by default?

*(Was `open-questions.md` Q39, raised 2026-08-14 out of §108. Moved here
2026-08-15 at the operator's direction — the entry itself said "ask again
later", which is the definition of not-a-queue-item. See `design-decisions.md`
§313.)*

**Trigger:** the first fastpy utility clears both bars — a parity test suite it
passes, and a measured benchmark showing it is faster, equal, or not
significantly slower than the Rust implementation. Promote this entry then,
with those numbers attached. **Nothing clears both bars today.**

**In short:** some OS utilities are being rewritten in Python (compiled to
native code by fastpy, so there is no speed penalty). §108 already decided that
a Python version may replace the Rust one, per command, once it is proven equal
on behaviour and speed. The only thing left open is which one a normal user
gets **without changing any setting** — the proven Python one, or the original
Rust one with Python as a switch you flip.

**Why it is not askable yet.** Answering before a single utility has cleared the
bars means answering without evidence. The honest input is *how close to parity
the first one actually gets, and what it measures* — and that does not exist.
Any answer now would be a guess dressed as policy.

**Nothing is blocked meanwhile.** §108 part 1 — fastpy utilities are added to
the test rootfs alongside the Rust ones, never replacing them — is the current
behaviour and needs no answer here. This only becomes live at the first real
swap.

**The options, for when it is live.**

| Option | *What changes* for a user who never touches settings |
|---|---|
| **A — Rust by default, Python opt-in** | Nothing; they run exactly what ships today. Switching is a deliberate act. |
| **B — Python by default once it clears both bars, Rust opt-out** | Their `ls` (or whatever cleared the bars) is silently the Python one. Behaviour should be identical — that is what the parity suite asserts — but "should" is doing work. |
| **C — decide per command at promotion time** | Depends on the command; a `cat` and a package manager are not the same risk. |

- **A's cost:** the Python implementations stay lightly exercised precisely
  because they are off, which is the "perpetual demo" trap §108 was trying to
  escape — just one bar higher.
- **B's cost:** the bars are *measured*, not *proven*. A parity suite is not
  years of field use, and the failure mode is user-visible behaviour changing
  under people who never asked for it.
- **C's cost:** no coherent story a user can hold ("which of my tools are
  which?"), and it defers the question forever by construction.

**Where it bites:** `scripts/create-ext4-rootfs.sh` (the `PROMOTED` map, and
whatever assembles the production rootfs `/bin`), `kernel/src/proc/spawn.rs`
(`resolve_command` / `COMMAND_PATH`), and wherever the opt-in switch ends up
living — most likely the settings surface rather than a build flag, since §108
makes it a user choice.

---

## D-Q2 — Install `clang` + `lld` and turn on LLVM CFI for C code?

*(Was `open-questions.md` A-Q1's **option B**, raised 2026-08-14 by Lane A.
Answered by the operator 2026-08-15 with **"not yet"** — a deferral, not a
refusal — and moved here the same day per `design-decisions.md` §313.)*

**Trigger:** the first substantial C port enters the build — ext4, Mesa, or
anything else that makes CFI govern a meaningful amount of compilation. Promote
this entry back into `open-questions.md` **at the start of that port, not
after**: retrofitting CFI onto a landed port means re-linking and re-testing it,
whereas building it that way costs one decision. Today the only C we compile is
`scripts/create-ext4-rootfs.sh`, with gcc.

**In short:** `clang` and `lld` are an alternative C compiler and linker. We do
not have them installed. Having them would let us switch on **CFI**
(Control-Flow Integrity — a compiler feature that stops an attacker redirecting
a function call to code of their choosing). The install is small and standard;
the question is whether it is worth changing how we build C at all right now.

**Why the answer was "not yet".** The payoff is currently near zero. Our rule is
that C is used *only* for porting existing C code, and nothing substantial is
ported yet — so "CFI as the default for C/C++" would today govern one shell
script's worth of compilation. Two costs against that:

- The one piece of C we build (`scripts/create-ext4-rootfs.sh`) uses **gcc**,
  and that script is **Lane B's tree**. Making CFI a default would reach across
  a lane boundary to change another lane's build for no present benefit.
- CFI wants **LTO** (whole-program optimization at link time), which changes
  build times and link behaviour for everything it touches.

**Nothing is blocked.** No roadmap item other than "Enable LLVM CFI as default
for C/C++ compilation" depends on it, and that item is the question itself. The
current behaviour — gcc, no CFI — is safe and stays.

**What will need deciding when it comes back** (not just install/don't):

- Whether CFI is the *default* for all C, or opt-in per ported component.
- Whether the LTO requirement is acceptable for the ported tree's build times.
- Whether `scripts/create-ext4-rootfs.sh` moves to clang too, or stays on gcc
  and is exempted — a Lane B decision that needs a `requests/` entry either way.

**Where it bites:** `.cargo/config.toml` (C flags), `scripts/create-ext4-rootfs.sh`
(Lane B), and `roadmap.md`'s Lane A backlog item "Enable LLVM CFI as default for
C/C++ compilation". Related: `design-decisions.md` §313 (this file's rules) and
`open-questions.md` → A-Q1, where option **A** (GNAT/SPARK, answered *install
it, with the prover*) still awaits recording by Lane A.
