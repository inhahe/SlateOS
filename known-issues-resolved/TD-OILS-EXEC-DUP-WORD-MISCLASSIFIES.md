### TD-OILS-EXEC-DUP-WORD-MISCLASSIFIES. `exec M>&WORD` still splits on "parses as a number" — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `apply_persistent_dup_out` (~14112)
and `apply_persistent_dup_in` (~14163), which still do
`bytes::as_str(target).and_then(|t| t.parse::<i32>().ok())` where the command
path now calls `Shell::classify_dup_word`.

**Reproduce:**

```sh
e=""
(exec 7<&"$e")                    # bash: 7: Bad file descriptor   osh: : ambiguous redirect
(exec 7<&"99999999999999999999")  # bash: 7: Bad file descriptor   osh: 99…: ambiguous redirect
(exec 7<&"+3")                    # bash: +3: ambiguous redirect   osh: 3: Bad file descriptor
(exec 7>&"-1")                    # bash: -1: ambiguous redirect   osh: -1: Bad file descriptor
```

Both directions of the same confusion: `parse::<i32>()` accepts a *sign*, which
bash's `all_digits` rejects, and rejects the empty and out-of-range words, which
`all_digits` accepts. So the signed words wrongly become descriptor numbers (and
then fail as bad descriptors) while the too-long and empty ones wrongly become
ambiguous redirects.

**Impact.** `exec {v}<&"$fd"` with a computed descriptor — the natural way to
park a coproc endpoint on a fixed number — reports the wrong failure whenever
the computation produced nothing.

**Proper fix.** Route both helpers through `Shell::classify_dup_word`, exactly
as `resolve_dup_out`/`resolve_dup_in` do. The bad-fd message needs the word as
written, which the caller at the `RedirectOp::DupOut`/`DupIn` arms has as
`crate::unparse::word_src(&r.target)` but the helpers do not, so they need it
passed in; `persistent_special_dup` synthesises an all-digit word that can never
be `BadFd` and can pass its own. Fold in
`TD-OILS-DUP-BADFD-NAMES-THE-WRONG-THING` at the same time, since the same
`fd`-versus-default test applies here.

**Fixed.** Both helpers now `match Shell::classify_dup_word(target)`, so the
`exec` path and the command path answer the same question, and both name a bad
descriptor through `Shell::dup_error_subject`. What the helpers needed was not
the word but the *redirect*, so they take a `DupOrigin<'_>`:

* `DupOrigin::Word(&Redirect)` — a real `M>&WORD`, which carries the source text
  the message may want and is a `dup2`;
* `DupOrigin::SpecialPath` — a `> /dev/fd/N` filename, which has no word (its
  caller rewrites the message into `PATH: No such file or directory`) and is an
  `open`, not a `dup2`.

That second distinction turned out to matter for more than the message, and
caught a regression from `TD-OILS-DUP-ONTO-A-NON-STD-FD-IS-WRONG`: the self-dup
skip added there (`n != fd`, because bash makes no `dup2` call when a descriptor
is duplicated onto itself) had been applied unconditionally, and
`resolve_special_redirect` funnels `/dev/fd/N` through the same resolvers. So
`echo B 8>/dev/fd/8` with fd 8 closed had started succeeding silently where bash
says `/dev/fd/8: No such file or directory`.

The skip is now split in two, which is what it always should have been. A
descriptor duplicated onto itself changes nothing whichever spelling asked, so
the *effect* is skipped unconditionally; what differs is whether the attempt can
*fail*, and that is `Shell::dup_needs_no_source` — true only for a `Word`, since
a dup bash never makes cannot fail while an `open` of `/dev/fd/8` can. Applying
the effect too had also been wrong in its own right: `> /dev/stdout` inside a
capture is fd 1 onto fd 1, and setting `plan.stdout_to_fd = Some(1)` lost the
capture (caught by the in-process test
`dev_stdout_duplicates_fd1_rather_than_naming_a_file`).

Corpus case: `tests/corpus/exec-dup-word-classification.sh`, matching bash
byte-for-byte on stdout and stderr across all three ways a word can be a bad
descriptor, both self-dup forms, and both spellings of a `/dev/fd/N` that is not
there. `clone_input_fd` now returns `Option` rather than a pre-formatted
message, since the caller owns the subject.
