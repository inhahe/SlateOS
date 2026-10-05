## BUG-TR-CURLED-TWO-QUOTES-GNU-LEAVES-STRAIGHT — §351 applied one level too broadly (lane B)

**Found 2026-08-21** by `scripts/tr-diff.sh`, the only red harness in a
26-harness sweep. **Fixed 2026-08-21** in `userspace/coreutils/src/bin/tr.rs`.

**What it was.** Two diagnostics differed from GNU in nothing but their quote
marks:

| command | ours | GNU 9.4 |
|---|---|---|
| `tr -d '[::]'` | `missing character class name ‘[::]’` | `missing character class name '[::]'` |
| `tr -d '[==]'` | `missing equivalence class character ‘[==]’` | `missing equivalence class character '[==]'` |

Both sides exit 1, so this is cosmetic — but the harnesses compare stderr
byte-for-byte on purpose, and a divergence nobody can explain is how a real one
gets waved through later.

**Cause, and why the obvious suspect was wrong.** The first hypothesis was the
character-wise `quote.rs` rewrite (§357), since that was the change in flight.
It is falsifiable and false: `tr-diff.sh` contains no non-ASCII case at all, so
nothing in it can reach a code path the rewrite touched. The actual cause is
**§351** — the operator's decision that our `quote()` emits U+2018/U+2019 in
*every* locale, where GNU emits them only under a UTF-8 one. That decision is
right, and is not what was wrong. What was wrong is that we routed these two
strings through `quote()` at all: they are compile-time constants that GNU
bakes into the translatable literal with ASCII apostrophes already inside it —
`tr.c` reads `_("missing character class name '[::]'")`. GNU's text is
therefore not locale-sensitive, and §351 has nothing to apply to. Making
`quote()` unconditionally curly turned a latent mismatch into a visible one.

**The trap this leaves behind, stated so nobody falls into it.** It is tempting
to conclude "constants are not quoted, runtime values are" and to go correct
every `quote()` of a literal in the tree. That rule is false, and it was
measured false before this entry was written: GNU's `test` puts its constant
`]` through `quote()` and *does* come out curly —

```
$ /usr/bin/[ a          # LC_ALL=C.UTF-8
/usr/bin/[: missing ‘]’
```

so `test.rs`'s three `quote(b"]")` / `quote(b")")` call sites are **correct and
must not be changed**. Nor is `tr` the odd one out for being a message *about*
syntax: `seq` quotes a constant too, and it is a whole English word rather than
a punctuation mark —

```
$ /usr/bin/seq nan          # LC_ALL=C.UTF-8
/usr/bin/seq: invalid ‘not-a-number’ argument: ‘nan’
```

— which is `seq.c`'s `quote_n (0, "not-a-number")`, and is why `seq.rs` line
469 is right to do the same. Whether a message quotes is a per-message choice
made upstream, message by message, with no principle behind it. Measurement
against the real GNU binary is the only oracle; both `tr` call sites now carry
a comment saying so, and each names the other.

The complete inventory of `quote()`-on-a-literal in the tree, all measured:
`test.rs` ×3 (`]`, `)` ×2 — curly, correct), `seq.rs` ×1 (`not-a-number` —
curly, correct), `tr.rs` ×2 (`[::]`, `[==]` — straight, fixed here). There is
no fourth case waiting to be found.

In the same file, `invalid character class ‘foo’` stays curly, because `foo`
came off the command line and GNU does route it through `quote()`. Two
conventions in three adjacent messages of one tool is exactly the shape of
thing that gets "tidied up" by a future reader, which is why the unit test
`an_unknown_class_and_an_empty_one_are_told_apart` asserts all three together
with the reason written above them.

**Coverage.** `tr-diff.sh` (the two cases that caught it, now green) plus that
unit test.
