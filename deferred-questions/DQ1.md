## DQ1 (was D-Q1) — Once a fastpy utility is proven as good as the Rust one, which does a stock install run by default?

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

**The operator's remark, 2026-09-27** (made while answering lane B's B-Q21,
§1053, and added here by lane B because it bears on this entry; it does not
answer it -- the trigger above still stands):

> As for the second question, I guess there's no point in having both a fastpy
> and a Rust implementation of anything. Wait, yes there is. We may determine
> that the Rust implementation is better and make that the stock install, but
> the user may find Python much easier to edit. And vice versa, they may prefer
> Rust for some reason even if we think the Python version is better. Though
> another option is to keep the alternative versions in the repo but not
> included in the OS distribution.

So both implementations may be kept, whichever becomes the default -- and a
fourth option joins the three above: **D, ship one, keep the other in the
repository only.**
