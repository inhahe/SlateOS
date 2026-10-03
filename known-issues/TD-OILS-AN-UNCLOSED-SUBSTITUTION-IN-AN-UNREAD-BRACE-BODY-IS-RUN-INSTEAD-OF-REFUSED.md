### [B] TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED. `x='A${z:-$(fi}B'; echo "${x@P}"` runs `fi}` where bash reports `bad substitution` — 2026-08-14 — ⚠️ OPEN

A `$( … ` with no `)` inside a `${ … }` written in text no parser read — a
`${x@P}` re-read, a `PS4`, a here-document body. bash reads the extent with
`xparse_dolparen`, which fails at end of input; `si` is left past the end of the
string, so the brace never closes, so `parameter_brace_expand` reports
`bad substitution` naming the whole text and prints the text unchanged. Nothing
is run. osh gets the *first* diagnostic right and then runs the body anyway:

```sh
x='A${z:-$(fi}B'; echo "${x@P}"
# bash: command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: A${z:-$(fi}B: bad substitution
#       A${z:-$(fi}B
# osh:  command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: fi}: command not found
#       A

x='A${z:-$(echo hi}B'; echo "${x@P}"
# bash: … unexpected EOF …; … bad substitution; A${z:-$(echo hi}B
# osh:  … unexpected EOF …; Ahi}
```

Both spellings are affected identically — `<(fi}` behaves exactly as `$(fi}`,
which is the point: the delimiter is not what is wrong here.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::extent_read_of_subs`]
(~29622) and [`Shell::run_abandoned_extent`]. The scan classifies the failed
read as `ExtentRead::Abandoned { body, rest }` and hands the body on to be run.
That classification is *right* for an abandoned extent bash really does run on
— it is `extract_command_subst`'s no-`)` path with the `jump_to_top_level`
suppressed — but wrong when the caller is the brace scan, because there the
unclosed read is also what stops the `}` from ever being found, and the
`bad substitution` that follows pre-empts the run.

**Proper fix:** distinguish the two callers. `extent_read_of_subs` should
report the abandonment to the brace scan (so `brace_extent_scan` fails the
whole `${ … }` and takes the `bad substitution` path with the source text)
rather than letting the body reach `run_abandoned_extent`. The `closed: false`
flag on `CmdSubBody::Unread` already names exactly this shape, so the test is
to hand.

**How it was found:** measuring the `<(` row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN
against its `$(` twin, which turned out to be wrong the same way.
