## `TD-B-CORPUS-PROSE-THAT-RECORDS-OSHS-LIMIT-AS-BASHS-BEHAVIOUR` (lane B, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** the shell test corpus is written as prose — each case file explains
*why* bash answers the way it does, and the explanation is as much the artifact
as the commands are. Two of those explanations turned out to describe **osh**
rather than bash: they stated a limitation of our implementation as though it
were a fact about the reference shell. Because the harness compares live output
rather than a stored transcript, a wrong explanation is invisible until the two
shells disagree — and when they do, the explanation argues for the wrong side.
Both were found by running the corpus against glibc bash and reading the prose
next to the diff instead of only the diff.

The disease is not "a comment went stale." It is that the comment was written
*from the implementation*, which makes it evidence for whatever the
implementation already does, and therefore worthless exactly when it is needed.

### The two instances

| Case | What the prose claimed | What bash actually does |
|---|---|---|
| `a-std-fd-bound-to-a-read-only-source.sh` | "Nothing here can be handed over as a *description* — a directory yields no usable handle — so what the child gets is no fd at all" | A directory opens read-only like anything else and the child is handed the real description |
| `a-subscript-in-an-arithmetic-word-is-expanded-in-place-first.sh` | "=== tilde expansion happens, at quoting 0 ===" | It does not happen at all: the word carries `W_NOTILDE` |

Neither claim was ever measured. The first is osh's own behaviour on Windows
(where `File::open` of a directory does fail) generalised into a claim about
POSIX; the second is a guess at what "quoting 0" implies, `Q_ARITH` and
`Q_DOUBLE_QUOTES` being the two bits the source visibly masks off, with the
third suppression — a *word* flag rather than a quoting bit — never looked for.

### Instance 1 — a directory reaches a child as a real descriptor

`sh -c 'echo W >&2' 2<dd` is the one line in the case that can tell an absent
descriptor from an unwritable one, and it was the one line that differed:

```
  ext-fd2 rc=1     bash
  ext-fd2 rc=2     osh
```

The status is **dash's**, not the outer shell's. With fd 2 present, `>&2` dups
it successfully and the *write* fails — dash exits 1. With no fd 2 at all the
`dup` is what fails, and dash exits 2. osh handed the child nothing, so it took
the second path. Every other line in the section is a 1 either way, which is why
the wrong explanation survived: eight of the nine cases cannot tell the two
stories apart.

**Fix.** `InputSrc::Directory` became `Directory(Option<File>)`.
`open_input_source` asks the host for the handle and keeps it when the host
gives one (Win32 refuses, and leaves `None` — the approximation there is
unchanged). The shell's *own* readers still synthesise `EISDIR` from the variant
rather than from a read, because that is what they saw before and it is
independent of whether a handle exists; but `child_input` and
`child_read_only_out` now `try_clone()` the handle and hand it over, so a child
answers for the descriptor itself. A `#[cfg(unix)]` test
`a_directory_reaches_a_child_as_a_real_descriptor` pins all three statuses.

### Instance 2 — `W_NOTILDE` is a flag on one word

`expand_subscript_string` builds the word it expands, and the flags it puts on
it are as much a part of the semantics as the quoting it is passed:

```c
  td.flags = W_NOPROCSUB|W_NOTILDE|W_NOSPLIT2;
  td.word = savestring (string);
  tlist = call_expand_word_internal (&td, quoted, 0, …);
                                            /* subst.c:10800-10812 */
```

So the subscript's own leading `~` stands, and `abstab` then backslash-quotes
it, which is why bash's arithmetic reader complains about `\~`. osh expanded it
and complained about `/zz`.

The half that matters for the *implementation*, and the reason this could not be
fixed with a mode the expansion is left in: the flag is on the word and does not
reach the words nested inside it. Measured, bash 5.2.21, `HOME=/zz`, `z` unset:

```text
  (( a[~] ))            \~: syntax error … (error token is "\~")
  (( a[${z:-~}] ))      /zz: syntax error … (error token is "/zz")
  [[ -v a[~] ]]         \~: syntax error … (error token is "\~")
  [[ -v a[${z:-~}] ]]   /zz: syntax error … (error token is "/zz")
```

**Fix.** A `Shell::no_tilde` field armed by `expand_arith_subscript` and
**taken** — `std::mem::take` — at the top of each of the three word-level walks
(`expand_word_annotated`, `expand_word_joined_annotated`, and the pattern walk).
Taking is what confines it to one word without a stack: the outermost word
consumes the arming, and every word nested in it sees the flag clear. A first
attempt armed only `expand_word_annotated`, which is the *splitting* walk; a
subscript goes through the joining one, so the flag was still set when the
operand's literal was pushed and the two cases came out exactly inverted — the
bare tilde expanded and the operand's did not. That inversion is a useful
signature: it means the flag was read by the wrong walk, not that the rule was
wrong.

The corpus case gained the `${z:-~}` and `[[ -v ]]` contrasts, so the scoping is
now part of what the harness checks rather than something the fix knew and the
test did not.

**The lesson worth keeping:** a corpus explanation must be written from a
*measurement of bash*, never from what osh does or from what the C source
appears to imply. Where a claim cannot be measured, say so in the file. The two
above cost nothing to check — one `bash -c` each — and both were wrong.
