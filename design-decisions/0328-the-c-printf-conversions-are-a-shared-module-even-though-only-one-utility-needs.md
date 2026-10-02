## §328 — The C `printf` conversions are a shared module, even though only one utility needs them today

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)

**In short:** The rules for turning a number into text — how `%5.2f`, `%-08x`
and `%.3s` decide what to print — are needed by the `printf` command, and will
later be needed by `awk`, by the shell's own built-in `printf`, and by anything
else that formats. The choice was whether to write those rules inside the
`printf` command (where they are used now) or in the shared library (where
three future callers can reach them). We put them in the shared library, as
`coreutils::cfmt`, and accepted a module with one caller for a while.

**Terms.** A *conversion specification* is one `%…` item in a format string:
a `%`, then flags (`-`, `+`, space, `#`, `0`), then a width, then a precision,
then a conversion letter (`d`, `s`, `x`, `f`, …). *Precision* means two
opposite things depending on the letter — a *minimum* number of digits for an
integer, a *maximum* number of characters for a string.

**The default this project follows** is stated in the head of
`userspace/coreutils/src/lib.rs`: almost everything is a standalone
`src/bin/*.rs`, because a utility that reaches into the others' machinery is
harder to read and no faster to build. The library is for the exceptions. So
this decision needed a reason, not just a convenience.

**The reason: the rules are numerous, interacting, and nobody recalls them
correctly.** A representative sample, all measured against glibc:

| input | prints | because |
|---|---|---|
| `%.0d` of `0` | *nothing* | precision 0 erases the value zero, and only zero |
| `%#.0o` of `0` | `0` | `#` raises the precision instead of prepending |
| `%#x` of `0` | `0` | but `#` on hex prepends nothing to zero |
| `%05.2d` of `7` | `   07` | the `0` flag loses to a precision |
| `%-05d` of `7` | `7    ` | and loses to `-` |
| `%+u` of `5` | `5` | sign flags are ignored by the unsigned conversions |
| `%.1s` of `abc` | `a` | precision truncates a string |
| `%.1d` of `5` | `5` | precision pads a number |

Eight rules, no two alike, and each one is a plausible thing to get wrong
independently. Written once per caller, three callers means three chances at
each; the failure mode is not "`awk` is broken" but "`awk`'s `%#.0o` disagrees
with `printf`'s", which is the kind of difference nobody finds by using the
system and nobody thinks to test.

**The alternative — write it in `printf.rs`, extract it when the second caller
appears — was rejected on when the extraction actually happens.** It happens
under deadline, from a file that has meanwhile grown tests and diagnostics
that assume its own internals, by an agent whose task is "write `awk`" and for
whom refactoring `printf` is a detour. The realistic outcome is not extraction
but a second copy. Doing it now costs a `pub` and a file move, and it is
already paid for: `printf` had to be rewritten anyway, so the rules were being
written from scratch either way.

**What it cost.** One module with one caller, and one indirection between
`printf` and its own formatting. `cfmt` deliberately *re-exports*
`extfloat::Spec` rather than defining a parallel type, so there is no
conversion layer between the two and `printf`'s directive scanner — which must
build a `Spec` by hand anyway, because `*` takes the width from an argument —
constructs the same struct both modules already use. `%a %e %f %g` are handed
straight to `extfloat::render`, so one call site covers the whole directive
vocabulary and the split between the two modules is invisible to the caller.

**What would reverse it:** nothing that is likely. If `awk` and the shell both
turn out to need a *different* formatting model — a locale-aware one, say, or
one that formats into a caller-supplied buffer — then `cfmt` would become the
coreutils-specific one of two, and the honest shape would be to name it so.
The pending change in `TD-PRINTF-BUILDS-THE-WHOLE-FIELD-IN-MEMORY` (a
`render_to(&mut impl Write, …)` alongside `render`) is the first step toward
that buffer-less form and is compatible with either outcome.
