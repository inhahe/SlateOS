## §360 — `uname -s` says `Linux` and `uname -o` says `SlateOS`, which is exactly GNU's `Linux` / `GNU/Linux` split

**Date:** 2026-08-22
**Decided by:** Claude (autonomous) — but only for the `-o` half; the `-s` half
was already decided by the **Operator** on 2026-06-10 (`roadmap-detailed.md`
§72, "Version-surface policy"), and this entry records that I found that
decision rather than made it.

**In short:** `uname` has two options that both look like they ask "what system
is this": `-s` (kernel name) and `-o` (operating system). On a normal Linux box
they answer differently on purpose — `Linux` and `GNU/Linux` — because the
kernel and the system built around it are two different things. We now do the
same: `-s` answers `Linux`, `-o` answers `SlateOS`. A program probing for a
Linux kernel ABI gets `Linux` and is satisfied; a human or an installer asking
what distribution this is gets `SlateOS` and is told the truth.

### The part that was not mine

`-s` = `Linux` is settled policy. `sys_uname` in `kernel/src/syscall/linux.rs`
states the reasoning: sysname and release are Linux-ABI-only surfaces, native
code uses native APIs, so the only callers of `uname(2)` are Linux binaries
that expect Linux values — and the release string must satisfy glibc's start-up
version gate. I initially took "what does `uname -s` return" for an operator
question and was about to file it as one; reading `roadmap-detailed.md` §72 and
the kernel's own source first is what stopped that.

### The part that was mine: what `-o` should say

`-o` is a GNU extension with no POSIX definition, and the kernel has no
opinion about it — there is no `/proc/sys/kernel/` file behind it, because
Linux itself does not publish one. So the value had to be chosen here.

| Option | *What changes:* | Against |
|---|---|---|
| **`SlateOS`** (chosen) | `uname -o` prints `SlateOS` while `uname -s` prints `Linux` | Two options print two different names, which surprises anyone who assumed they were synonyms |
| `GNU/Linux` | `uname -o` prints exactly what a Linux box prints | Says we are a GNU system, which we are not — our userland is our own. It is a lie told for compatibility with nothing in particular: nothing gates on `-o`'s value the way glibc gates on `release` |
| `Linux` | `-s` and `-o` agree | Throws away the one field where we *can* say what we actually are without breaking anything, and makes `-o` pointless |

`SlateOS` wins because the `-s`/`-o` distinction exists precisely so that a
system can name its kernel personality and itself separately, and we are the
textbook case of a system where those differ. The compatibility argument that
forces `-s` to say `Linux` — that real programs parse it and act on it — has no
counterpart for `-o`: it is a human-facing field.

**No space in it.** `SlateOS`, not `Slate OS`, which is what the previous
implementation used. `uname -a` output is routinely split on whitespace, so a
field that is two words shifts every field after it. That is not a preference;
it is why the old value was a bug.

### Revisit if

…anything is ever ported that keys off `-o`. Nothing does today, and if
something does, the cheap answer is that it almost certainly wants `GNU/Linux`,
in which case this is a one-constant change. Recorded as a Claude decision
precisely so it stays cheap to reverse.
