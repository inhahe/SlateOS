## 603. The shell's operand parsers are one generic `required_num<T>`, not one function per integer width

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel shell has ~700 places that read a number the user
typed. §600 says each must *refuse* a word it cannot read rather than
substitute a default, and the refusals are being converted in batches. The
conversion runs through a small helper that does the reading and the
refusing. The question was whether that helper should be written once per
integer size -- `required_u32`, `required_u64`, `required_i32`, and so on --
or once, generically, over "any type that can be parsed from a string." It is
now one generic pair, `required_num<T>` / `optional_num<T>`. Nothing a user
sees changes; the diagnostics are identical.

**What was there before.** Six near-identical functions had accumulated over
three burn-down batches: `required_u32`, `required_u64`, `required_i32`,
`optional_u32`, `optional_u64`, and the id-specific `required_id`. Each was
about fifteen lines and differed from its siblings only in the type after
`parse::<>`. `required_i32` was added on its own in the second batch, for
`monitors pos`, because a monitor coordinate to the left of the primary
display is negative and `required_u32` would have refused exactly the inputs
that are correct.

**The argument for keeping them separate** is written in the file's own
history: `required_u64`'s docstring said, in as many words, that a generic
version was rejected because *"a `T: FromStr` bound would pull
`core::str::FromStr` into a signature that every call site would then have to
name."* That is a real cost if true -- 700 call sites each carrying a
turbofish would be worse than six duplicated helpers.

**It was not true, and that is what settled it.** Of the 55 call sites at the
time of conversion, **51 compile with no annotation at all**: the value flows
into a typed destination -- a struct field, a function parameter, a comparison
against a typed constant -- and inference reads the type from there. Only four
need `required_num::<u64>`, and they need it for a reason worth knowing
rather than a reason worth hiding: `reslimit setmem` and `reslimit setio` are
the only operands in the file whose value never reaches a typed destination.
Both are multiplied out to bytes or bytes-per-second on the spot, and the
*product* is what the subsystem sees, so nothing downstream pins the width.
Those four sites carry a comment saying so, and saying why `u64` is the right
answer and not a defensive widening: a limit in MiB times 1024*1024 overflows
`u32` at 4 GiB, which is an ordinary memory limit to ask for.

**The other half of the argument is the size of what remains.** A survey of
the 706-site backlog's parse targets found **nine distinct types**: `u32`
(279), inferred-from-context (221), `u64` (159), `usize` (60), `u8` (21),
`u16` (16), `i32` (14), `i64` (9), `i8` (1). Continuing the per-width family
therefore meant writing and maintaining ten more functions that differ in one
token -- and, worse, meant that the next person converting a batch would have
to *notice* that `u16` has no helper yet and write a seventh, or quietly
reach for `required_u32` and truncate. A helper family with gaps in it is a
helper family that invites the wrong helper.

**What was given up.** Two things, both small and both real:

* **The error message can no longer name the type.** A per-width helper could
  in principle say "is not a 32-bit number". It never did -- every one of them
  took a `noun` parameter and said `` `abc' is not a size in MiB ``, which is
  the better message anyway, because the user typed a size in MiB and does
  not know or care how many bits it lands in.
* **Four call sites now carry a turbofish** that they did not before. Judged
  worth it: the turbofish is *at* the site where the width genuinely is a
  decision, which is where a reader should be made to think about it.

**Where the deleted rationale went.** Each of the six docstrings carried a
paragraph earned by a real defect, and none of it was thrown away.
`optional_u64`'s note about `reslimit`'s inverted default — `0` means
*unlimited*, so `.unwrap_or(0)` answered a mistyped cap by removing the cap
-- is now the worked example in `optional_num`. `required_i32`'s note about
negative monitor coordinates is now the "Choose `T` from what the operand
means" section of `required_num`. Deleting a helper must not delete the bug
report that justified it.

**`required_id` was kept.** It is not a width variant: it fixes the operand
index at 1, returns `u64` unconditionally, and emits an id-specific
diagnostic. 42 of the 52 sites in the first burn-down batch were the
character-for-character identical line it replaced. Folding it into
`required_num` would have cost every one of those sites two extra arguments
to say something the helper's name already says.

**Where it came from.** `known-issues.md` →
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`,
third burn-down batch (`cmd_colorpicker`, 23 sites). Commit `f6c59b370`.
