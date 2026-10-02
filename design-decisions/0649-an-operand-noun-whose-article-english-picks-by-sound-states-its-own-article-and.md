## 649. An operand noun whose article English picks by *sound* states its own article, and a static gate enforces it for the open class only

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** The shell refuses a bad operand with a sentence like "`1OOO' is
not a user id". The "a" versus "an" is chosen by a helper that looks at the
first *letter*, so a noun spelled with a vowel but pronounced with a consonant
— "user", which sounds like *yoo*-zer — came out as "an user id". Seven call
sites in `kshell.rs` were saying that; six had been saying it for some time and
nobody had noticed, because the only thing that reads a refusal's exact wording
is a self-test assertion, and six of the seven had none. All seven now write
their own article into the noun (`"a user id"`), which `article_for` already
supports and prints verbatim, and a new static gate,
`scripts/check-shell-noun-article.py`, refuses any future noun in the same
class that does not.

### What went wrong

`kshell.rs`'s operand helpers (`required_num`, `optional_num`, `readable_num`,
`readable_hex`, `required_hex`, `optional_hex`, `required_key`) build their
refusal as `is not {article}{noun}`, and `article_for` picks the article by
spelling: a leading vowel *letter* gets `"an "`, anything else gets `"a "`.
That rule exists for a good reason — it is right for the large majority of the
284 distinct nouns in the file, and before it the shell said "is not a element
id" and "is not a inode ratio".

`article_for`'s own doc comment already named the flaw and the escape hatch: a
noun that begins with `"a "` or `"an "` is printed as-is, "which keeps the
exception at the one call site that needs it instead of in a table here that
the next noun would fall off the end of." Four sites were using it (`"a UID"`).
Eleven `u`-initial nouns were not thinking about it at all, and seven of those
were wrong.

The failure surfaced in the worst possible place. Batch 44 of the §600
burn-down added a self-test rung asserting the *correct* English for a new
`autostart add` refusal, and the rung failed — in QEMU, at the end of an
eleven-minute boot, with a kernel panic:

```
!! `an unreadable uid is named, not read as root`: output lacked text it must contain
   expected: `1OOO' is not a user id
   actual:   autostart: add: `1OOO' is not an user id
```

Worth recording plainly, because it is the part that generalises: I had chosen
the noun `"user id"` over `"uid"` *specifically* to avoid `article_for` printing
"an uid" — and walked straight into "an user id", because `u` is a vowel letter
either way. Reasoning about the rule without running it produced a wrong answer
that looked careful.

### Why this needs a gate and not just a fix

A rule about the text of a string literal should not cost a boot cycle to
check, and this one could only be checked that way. The two gates that might
have caught it structurally cannot:

- `check-selftest-wording.py` resolves the fragments a rung asserts against
  text the command can print. The article is not in the format string — it is
  the return value of a function call, so there is nothing in the source for it
  to match against.
- The rung itself only catches a site that *has* a rung. Six of the seven wrong
  sites did not, which is exactly why they had survived.

So the gate is static, runs in under a second, and was proved non-vacuous
before being trusted: reverting the one repaired site reproduced the QEMU
failure as `kshell.rs:49653  'user id' would print "is not an user id"`, exit
1. It also carries a 14-case `--self-test` fixture, following
`check-shell-message-names.py`'s convention, so that a gate whose parser has
quietly collapsed cannot report a clean tree in the same words as a working
one — the file itself is expected to be clean, so agreement with the tree
proves nothing on its own.

### What is enforced, and why not more

The enforced class is `u`, `eu` and `one` — the "yoo"/"wun" onset. That line
was drawn from a survey of all 284 distinct nouns in the file, not from
intuition:

| Class | Sites | Finding |
|---|---|---|
| `u` | 7 | **all seven wrong.** Three *other* `u` nouns ("an uncompressed size", "an upload limit", "an utterance id") were right, so the letter genuinely splits both ways |
| `eu`, `one` | 0 | none yet, but the same open, productive class |
| `h` | 11 | **all eleven right** — handler, hard, height, high, horizontal, hotspot, hue. Every one a hard `h` |

`h` is deliberately excluded. The distinction that decides it is
**open versus closed**: English keeps making new "yoo" words (user, unit, uid,
URL, unicast, usable) and they are indistinguishable by spelling from the "uh"
ones (unreadable, update, upper), so a caller writing a new `u` noun genuinely
has to stop and decide. Silent `h` is a closed set inherited from French —
hour, honest, honour, heir and their derivatives — four words, none of which
appears here. Demanding an explicit article on eleven correct call sites to
guard four words nobody has written is noise, and noise in a gate is how a gate
stops being read.

If an `h` noun from that closed set ever does appear, the right fix is a
four-entry table inside `article_for`, not an extension of this gate. A closed
set is precisely the thing a table handles without "the next noun falling off
the end of it", which is `article_for`'s own stated objection to tabulating the
open case.

### Alternatives considered

| Option | Why not |
|---|---|
| **Fix only the site that panicked** | The panic named one line; the survey found seven, six of them pre-existing and none of them covered by a rung. Fixing one and leaving six is how the class stays alive. |
| **Teach `article_for` a pronunciation table** | For the *open* class this is the table its own doc rejects: it is never finished, and its failure mode is silent — a new noun that is not in it gets the wrong article with no diagnostic anywhere. |
| **Rely on self-test rungs to assert the wording** | That is the status quo that let six sites be wrong indefinitely, and it charges an eleven-minute boot for a fact about a string literal. |
| **Enforce `h` as well, for symmetry** | Eleven correct sites would each have to restate an article to guard a four-word closed set with no members in the tree. |
| **Make it a `clippy` lint or a `const` assertion** | The nouns are runtime `&str` arguments to generic helpers; nothing in the type system distinguishes the noun from the command name. A textual gate over the call's last string literal is the shape the data actually has. |

### How to reverse it

Delete `scripts/check-shell-noun-article.py` and its two-step call in
`scripts/boot-test.sh`. The nouns themselves need no reversal — an explicit
`"a user id"` is correct English with or without the gate. What reversing
costs is the guarantee that the next `u`-initial noun is noticed at all: it
would be caught by a self-test rung if one exists, in QEMU, or by nobody.
