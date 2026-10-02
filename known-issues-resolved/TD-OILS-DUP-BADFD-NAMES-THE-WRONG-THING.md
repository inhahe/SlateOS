### TD-OILS-DUP-BADFD-NAMES-THE-WRONG-THING. a bad-fd dup on a non-default redirector names the word, not the redirector — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs`, `resolve_dup_out` / `resolve_dup_in`
— the `DupWord::BadFd` arm, which always formats
`crate::unparse::word_src(&r.target)`.

**Reproduce:**

```sh
e=""
read -r l <&"$e"     # bash: "$e": Bad file descriptor
read -r l 0<&"$e"    # bash: "$e": Bad file descriptor
read -r l 7<&"$e"    # bash: 7: Bad file descriptor      osh: "$e": …
echo hi >&"$e"       # bash: "$e": Bad file descriptor
echo hi 2>&"$e"      # bash: 2: Bad file descriptor      osh: "$e": …
```

**The measured rule.** bash names the *word as written* only when the redirector
is the operator's own default — 0 for `<&`, 1 for `>&`. Any other redirector
number, including one that merely *is* the default written out (`0<&` still
counts as default, `1<&` does not), makes the message name the **redirector
number** instead. Fourteen samples across command/`exec`, quoted/bare and
literal/expanded words agree, and writing the default explicitly is
indistinguishable from omitting it — so this is a test on the redirector's
*value*, not on whether the parser saw a number.

This is recorded as measured behaviour: no reading of bash's `redirection_error`
accounts for it (the obvious paths would print either the expansion or the
redirector in *all* cases), so do not "simplify" it away without re-measuring.

**Impact.** Only the diagnostic text, and only for a redirector other than the
default — which is the rarer form. Nothing in the corpus exercises it yet.

**Fixed** (2026-08-01) by `Shell::dup_error_subject`, which all four
bad-descriptor reports in `resolve_dup_out`/`resolve_dup_in` now call instead of
formatting `word_src` unconditionally.

Re-measuring for the fix turned up a **third** answer the entry above had
missed, and it takes precedence over both of the others: a redirectee that is a
bare run of digits fitting an `i32` — no quotes, no backslashes, no expansions —
is not a word to bash at all but a `NUMBER` token, and the redirect it builds
carries the *number*. So the message names that number reprinted, which need not
be what was typed and does not depend on the redirector: `<&007` and `2>&007`
both say `7`. One quote or backslash anywhere in the run (`<&9""`, `<&\9`) makes
it a word again, and so does a run too long for an `i32` — which is why
`0<&99999999999999999999` names the whole run while `7<&99999999999999999999`
names `7`.

The corpus case gained three sections for this: the same bad word under nine
different redirectors, all three ways of being bad under both operators, and the
number/word split. Every row is byte-identical to bash on both streams.

**Not fixed here**, and split out below as
`TD-OILS-DUP-ONTO-A-NON-STD-FD-IS-WRONG`: an input dup with a non-zero
redirector never checks its source at all, and a dup of a descriptor onto itself
is checked when bash does not check it.
