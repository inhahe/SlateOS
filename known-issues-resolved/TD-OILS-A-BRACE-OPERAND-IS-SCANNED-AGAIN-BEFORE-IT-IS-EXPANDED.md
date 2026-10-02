### TD-OILS-A-BRACE-OPERAND-IS-SCANNED-AGAIN-BEFORE-IT-IS-EXPANDED. A `${x:-word}` whose `word` is used reports a failed extent read three times, not two — 2026-08-09 — ✅ FIXED 2026-08-09/10

**Status.** Both defects in the reproduce block below are **fixed**. The `}BB` —
the leftover escaping the sub-word — went with part 1 (see "Part 1, as landed");
the three-vs-two report count went with the report half of part 2 (see "Part 2's
report half, as landed"). `unset z; a='A${z:-p$(fi⏎q)r}B'; "${a@P}"` now gives
three reports and `[Ap⏎q)rB]` in both shells, byte for byte.

The **text** half of part 2 — read 2 does not only report, it *rewrites* the
operand, and read 3 expands the rewrite — is fixed too. The rewrite had two
independent pieces:

- ✅ **The `SX_STRIPDQ` byte rules, fixed 2026-08-10.** This was the piece that
  mattered: an everyday divergence with no `@P` and no failed read anywhere in
  sight —

  ```text
  unset z; echo "${z:-p"\x"r}"     bash: pxr     osh (before): p\xr
  ```

  — because inside the operand's embedded `" … "` a backslash before a character
  that is *not* one of `` $ ` " \ ``⏎ is **removed**, where osh kept it. See
  "Part 2's `SX_STRIPDQ` rules, as landed".
- ✅ **What a *failed* read leaves in `temp`, fixed 2026-08-10.** Prompt-only, and
  it is the table further down. See "Part 2's failed-read copy, as landed".

The rule is the `stripdq` arm of subst.c:906-911:

```c
  if ((stripdq == 0 && c != '"') ||
      (stripdq && ((dquote && (sh_syntaxtab[c] & CBSDQUOTE)) || dquote == 0)))
    temp[j++] = '\\';
```

— with `stripdq` set, the backslash is re-emitted only inside an embedded quote
before a `CBSDQUOTE` character, or anywhere outside one. Measured, `z` unset:

| `z` unset | bash 5.2.37 | osh before the fix |
|---|---|---|
| `echo "${z:-p"\x"r}"` | `pxr` | `p\xr` ❌ |
| `echo ${z:-p"\x"r}` (unquoted — no `Q_DOUBLE_QUOTES`, so `temp = value`) | `p\xr` | same ✅ |
| `echo "${z:-p\xr}"` (no embedded quote — `dquote == 0`) | `p\xr` | same ✅ |
| `echo ${z:-p\xr}` | `pxr` | same ✅ |
| `echo "${z:-p"\$"r}"`, `"\\"`, `"\`"`, `"\`⏎`"` (the `CBSDQUOTE` set) | `p$r`, `p\r`, `` p`r ``, `pr` | same ✅ |

It is the same six operators as the report half — `:-`, `-`, `:=`, `=`, `:+`, `+`
all gave `pxr`/`p\xr`, while `:?`, `#`, `%`, `/`, a `/` replacement and `:off`
all agree — and it shows in every quoted context (`echo "…"`, an assignment RHS,
a here-document body, `printf %s`), not just `@P`.

The failed-read half of the same rewrite is the prompt-only one, and the
divergence was confined to operands containing a `"`:

| operand under `${z:-…}`, `z` unset | bash 5.2.37 | osh before the fix |
|---|---|---|
| `p$(fi⏎q)r` | `[Ap⏎q)rB]` | same ✅ |
| `p"$(fi⏎q)"r` | `[Ap⏎q)rB]` | `[Ap⏎q)"rB]` ❌ |
| `p"a$(fi⏎q)b"r` | `[Apa⏎q)brB]` | `[Apa⏎q)b"rbB]` ❌ |
| `"$(fi⏎q)"` | `[A⏎q)B]` | `[A⏎q)"B]` ❌ |

with two controls that already agreed in both shells and isolate the rewrite to
`parameter_brace_expand_rhs`: a pattern operand `A${z#p"$(fi⏎q)"r}B` → `[AB]`,
and the same text outside a brace `Ap"$(fi⏎q)"rB` → `[Ap"⏎q)"rB]`. (The
pattern control is not just an observation: a modifier's pattern is expanded
under `Q_PATQUOTE` and so never reaches the `SX_STRIPDQ` scan at all — see
TD-OILS-A-MODIFIERS-PATTERN-IS-EXPANDED-AS-A-DOUBLE-QUOTED-STRING.)

**Where:** `userspace/oils/src/interp.rs`, the `${ … }` operand path
(`Shell::brace_extent_scan` scans the operand once, and expanding it reads it a
second time). Two shapes were wrong, and the second is the more visible.

**Reproduce** (the original 2026-08-09 measurement; the `osh` column is now
history — see Status):

```text
unset z
a='A${z:-p$(fi
q)r}B';  printf '[%s]\n' "${a@P}"       # printf on line 4

  bash: three copies of
          command substitution: line 4: syntax error near unexpected token `fi'
          command substitution: line 4: `fi'
        then  line 3: f: command not found
        [Ap
        q)rB]
  osh:  two copies, then the same command-not-found
        [Ap
        q)r}BB]

y=Y; the same word          bash and osh agree: one report, [AYB]
```

**Why (measured against bash 5.2.37, then read back in the source).** Three
reads, not two, and only when the operand is actually used:

1. `extract_dollar_brace_string`'s scan of the whole `${ … }` — the one
   `brace_extent_scan` models. This is the only read when the parameter is set.
2. `parameter_brace_expand_rhs` (subst.c:7726-7731) opens with
   ```c
   if ((quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) && *value)
     { sindex = 0; temp = string_extract_double_quoted (value, &sindex, SX_STRIPDQ); }
   ```
   — another *scan*, over the operand alone, which reads a `$(` it meets
   (subst.c:955-963) and so reports without running anything.
3. `expand_string_for_rhs` (subst.c:7737), the real expansion, which reports a
   third time and runs the child once.

That two of the three are scans is measured, not inferred: with the body
`$(echo RAN >&2; fi⏎q)` the diagnostic appears three times and `RAN` once.

The `Q_DOUBLE_QUOTES` gate on 2 is **unobservable from a script** and osh may
apply the scan unconditionally: a failed extent read survives only under
`no_longjmp_on_fatal_error`, which only `expand_prompt_string` raises, and
`decode_prompt_string` calls it as `expand_prompt_string (result,
Q_DOUBLE_QUOTES, 0)` (parse.y:6094). Measured — the same word gives three
reports whether the `${a@P}` holding it is quoted or not.

**A rule of read 1 the entry did not have, found on the way in and fixed
2026-08-10: the scan carries on past a failed read.** `extract_dollar_brace_string`
does not test how the read came out —

```c
  if (string[i] == '$' && string[i+1] == LPAREN)
    { si = i + 2; t = extract_command_subst (string, &si, flags|SX_NOALLOC);
      i = si + 1; continue; }                  /* subst.c:1896-1903 */
```

— so a read that failed leaves `si` where its reader stopped and the loop looks
for the next `$(` one past it. *Every* failing substitution in the word is
reported, not just the first, and the brace still finds its `}`. osh stopped at
the first: `z=ZZ; A${z:-p$(fi⏎q)r$(for⏎s)t}B` under `${…@P}` reported once where
bash reports twice, and a third failing substitution added nothing.

What is scanned on is **text**, which is what makes the resume point visible and
is why the fix could not be "walk on through the part list": a `$(` the failed
read swallowed is not reached again (`p$(fi $(for⏎x)q)r` reports once, and the
error token `` `fi $(for' `` shows why), while one nested *inside* the failed
body but past the stop point is (`p$(fi⏎$(for⏎x)q)r` reports twice) — and the
part tree never took the failed body apart, so it can answer neither. Fixed by
handing the remainder to a fresh scan: `Shell::extent_read_of_rest` lexes it
with `dquote_word_from_source` the way `expand_extent_rest` already lexes its
own, and `brace_scanned_subs_slice` collects from a bare parts run (a top-level
`$( … )` is not reachable through `unparse::nested_parts`, which was why the
first attempt at this changed nothing). Corpus case
`a-brace-scan-carries-on-past-a-failed-extent-read.sh`.

The `}BB` is a second, independent defect on the same path, and the more
interesting one: **one `tail` cannot serve both reads.** `CmdSubBody`'s `tail` is
"the rest of the enclosing word", filled by `unparse::attach_comsub_tails_in`,
which treats a `[ … ]` subscript as a string scope of its own but not a brace
operand. For read 1 that is exactly right — `extract_dollar_brace_string` is
walking the whole word, and bash's echo proves it: `A${z:-p$(fi)q}B` quotes
`` `fi)q}B' ``, the `}B` included. For reads 2 and 3 it is wrong, because those
are handed the *operand* alone (`value`, a string cut out by the scan), so their
composite is `fi⏎q)r` and their leftover `⏎q)r`. osh reuses the word-scoped tail
and its leftover runs over the `}`, hence `q)r}B` and then the word's own `B`
again.

**Proper fix.** Two parts, in this order:

1. ✅ **Done** (with the ordering inverted — see "Part 1, as landed"). Make the
   sub-word a string scope, so a part inside it carries the *sub-word's*
   remainder, and let the scan keep the word-level one, which is the only reader
   that wants it. This is worth doing on its own: it is the only reason the
   leftover escapes the sub-word.
2. Add the double-quoted pre-scan of 2 above, gated on the expansion being
   quoted, as a scan that reports without running. ✅ **Report half done
   2026-08-10** — see "Part 2's report half, as landed". **It is not only a
   scan: it rewrites the operand**, and read 3 expands the rewrite, not the
   original. That half has two independent pieces. The *first* is the
   `SX_STRIPDQ` rewrite of the ordinary bytes — strip the unescaped `"`s and
   drop a backslash that is inside one of them and not before a `CBSDQUOTE`
   character (see Status). ✅ **Done 2026-08-10**; it needed no failed read at
   all and was what showed up in ordinary scripts. The *second*, still open, is
   what a failed read leaves in `temp`:
   `string_extract_double_quoted` copies a `$( … )` through by *its extent*
   (subst.c:955-993), and on a failed read takes the fallback

   ```c
   ret = extract_command_subst (string, &si, (flags & SX_COMPLETE));
   temp[j++] = '$'; temp[j++] = string[i + 1];
   /* Just paranoia; ret will not be 0 unless no_longjmp_on_fatal_error is set. */
   if (ret == 0 && no_longjmp_on_fatal_error)
     { free_ret = 0; ret = string + i + 2; }        /* the WHOLE remainder */
   for (t = 0; ret[t]; t++, j++) temp[j] = ret[t];
   temp[j] = string[si];
   if (si < i + 2) i += 2;
   else if (string[si]) { j++; i = si + 1; }
   else i = si;
   ```

   — so the remainder is copied once as the body *and* again by the loop
   resuming at `si + 1`, with `string[si]` between them. The leftover is
   **duplicated**.

**Worked example**, derived from the source and agreeing byte-for-byte with the
measured `[Ap⏎q)rB]` above. Operand `p$(fi⏎q)r` (indices `p`0 `$`1 `(`2 `f`3
`i`4 `⏎`5 `q`6 `)`7 `r`8):

| read | over | reports | leaves |
|---|---|---|---|
| 1 | the word after `$(`: `fi⏎q)r}B` | `` `fi' `` | scan resumes at `i`, counts on, finds `}`; operand = `p$(fi⏎q)r` |
| 2 | the operand: `fi⏎q)r`, `si` = 4 | `` `fi' `` | `temp` = `p` + `$(` + `fi⏎q)r` + `i` + `⏎q)r` = `p$(fi⏎q)ri⏎q)r` |
| 3 | `temp` after `$(`: `fi⏎q)ri⏎q)r` | `` `fi' `` | body `f` (run: `f: command not found`), resume at `temp[5]` |

Expansion = `temp[0]` + `temp[5..]` = `p⏎q)r`, i.e. `[Ap⏎q)rB]`. Each read stops
one line in (`ep` past `fi⏎`, newline-stripped to past `fi`), which is
`Shell::failed_extent_split` — the three differ only in *what string* they are
handed, which is the whole of part 1.

**Part 1, as landed** (commit "oils: a brace sub-word is expanded on its own
string"). The prescription above was **inverted in the implementation**, and
deliberately:

> …have `Shell::brace_extent_scan` compose the word-level remainder itself…

it cannot. `brace_extent_scan(&mut self, part: &WordPart)` is handed one part
and has no word context at all — the parts after it are not reachable from it —
so the word-level remainder is *only* computable in the unparse pass, which is
where `attach_comsub_tails_in` already computes it. So the tail attached in
unparse stays the **word-scoped** one (the scan's, which is right for the scan),
and the *sub-word*'s remainder is re-derived on demand at the expansion sites by
a new `unparse::rescoped_part` / `rescoped_parts` (`userspace/oils/src/unparse.rs`),
which rebuilds the part with its nested strings re-tailed as their own scopes.
`Nested` grew `Index` and `Quoted` to name the two shapes that already were
their own strings. Call sites (`userspace/oils/src/interp.rs`), each immediately
after its `brace_extent_scan`: `quoted_per_element_parts`, `expand_dynamic_with`,
`split_items`.

That is only half of it, and the second half was not in the prescription at all:
**`Shell::extent_consumed` must be scoped to the sub-word too.** A failed read
sets the flag to mean "this walk is over"; if the sub-word's walk shares the
word's flag, the word's own tail is dropped as well. bash prints `AzzB` for
`z=zz; b='A${z#p$(fi⏎q)r}B'; "${b@P}"`, not `Azz`, because
`parameter_brace_remove_pattern` is handed `patstr` and reads it with a fresh
`expand_word_internal` owning its own `sindex`. So `expand_word_pattern_inner`
and `expand_replacement_inner` now save/restore `extent_consumed` exactly as
`expand_word_annotated` does. Every sub-word reader is its own
`expand_word_internal` for this purpose — `parameter_brace_remove_pattern`,
`parameter_brace_patsub`'s `expand_string_if_necessary (rep, …)`
(subst.c:9180-9187), `array_expand_index`'s `expand_arith_string`, and
`cond_expand_word` for a `case` arm.

Measured byte-identical with bash 5.2.37 for `#`, `%`, `##`, `/pat/`,
`//x/repl`, `^^`, `${z:off}`, `${z:off:len}` and `${a[…]}`; corpus case
`a-brace-sub-word-is-expanded-on-its-own-string.sh` (16 probes plus a `PS4`
block). The `:-`/`:=`/`:+` operand is *not* covered: it is the one shape whose
leftover is duplicated rather than merely misscoped, and that is part 2.

**Part 2's report half, as landed** (commit "oils: read a used brace operand once
more before it expands"). `Shell::operand_rhs_read` runs the second read, and
`Shell::rhs_scanned_subs` collects what that read would have reported. Three
things had to be got right, and each is measured:

- **Which operators reach it.** `parameter_brace_expand_rhs` is called by exactly
  `-`, `=` and `+` (subst.c:7724-7732 is its opening), and the call sites are the
  four in `expand_operand_fields` plus the two `ParamOp::AssignDefault` arms
  (scalar and array). `:?` expands its operand through
  `parameter_brace_expand_error` and every pattern operator through a reader of
  its own, so those stay at **two** reads, not three. `${z:+…}` with `z` unset,
  and `${z:-…}` with `z` set, leave the operand unused and stay at **one**.
- **The refusal comes first.** With no positionals, `A${1=p$(fi⏎q)r}B` reports
  **once**, not twice: bash says `cannot assign in this way` before
  `parameter_brace_expand_rhs` is called at all. So the call goes *after* the
  refusal in both `AssignDefault` arms, not at the top.
- **It is the same kind of scan, so it carries on.** Two failing substitutions in
  a used operand are two extra reports; a nested `${ … }` does its own second
  read of *its* operand, for two extra rather than one; a backquote is a byte
  hunt and cannot fail; a `$((` is the same `$(` row. All of which falls out of
  reusing `extent_read_of_subs`, i.e. read 1's machinery, over the operand alone.

The rules of *which* parts the second read looks at are the double-quoted
string's rather than the brace's (subst.c:850-1000): no single-quote row at all,
`` ` `` a byte hunt, `$(` read, `${` handed to the brace scan. Hence
`rhs_scanned_subs` rather than `brace_scanned_subs`.

Corpus case `a-used-brace-operand-is-scanned-once-more-before-it-expands.sh`
(5 sections). Two shapes were deliberately left out of it: a `"` in the operand,
which is the text half above; and a `\$`, which is *also* a prompt escape and
renders through `$EUID` — osh reports root by design, see open-questions Q28.

**Part 2's `SX_STRIPDQ` rules, as landed** (commit "oils: strip the quotes off a
used brace operand before expanding it"). `Shell::operand_rhs_read` now returns
the operand read 3 should expand, and its three call sites use it in place of the
one the parser gave. The rewrite is `Shell::stripdq_text` /
`stripdq_quoted_text`.

It needed no byte walk, because **the lexer has already sorted the two cases.**
Reading a double-quoted run (`userspace/oils/src/lexer.rs`), a backslash before
one of `"` `\` `$` `` ` `` becomes a one-character `WordPart::SingleQuoted` with
`escaped` set, a backslash before a newline is dropped, and every *other*
backslash is pushed into the literal run as a plain byte. So a bare `\` byte in a
`Literal` under a `WordPart::DoubleQuoted` is exactly the backslash `SX_STRIPDQ`
drops, and nothing else is — the fix is to walk the operand's top-level
`DoubleQuoted` parts and take those bytes out. Descending no further is what
gives the extent rule for free: a `$( … )` and a nested `${ … }` are other kinds
of part, so what is inside them is untouched, and a nested brace gets the rule
from its own `parameter_brace_expand_rhs`.

Two rules that fall out of this and are pinned rather than assumed: the operand's
*own* top level is `dquote == 0` however the enclosing word was quoted (a
top-level `\x` is a `Literal` too, but not under a `DoubleQuoted`, so the walk
does not reach it); and a `'` is not special to
`string_extract_double_quoted` at all, so `"${z:-p'\x'r}"` is `p'\x'r` — quotes,
backslash and all.

Corpus case `a-used-brace-operands-embedded-quotes-lose-their-backslashes.sh`
(6 sections, 30 probes: the rule, the `CBSDQUOTE` set, the outside-the-quotes
controls, quoted vs unquoted, all eleven operators, four quoted contexts, and
the extent-copied shapes).

**Part 2's failed-read copy, as landed** (commit "oils: build a used brace
operand's rewrite as text, not as parts"). The rewrite above was first written as
an edit of the operand's *parts*, which gives the same answer for every operand
that expands cleanly. It does not for one whose `$( … )` fails to read: that path
takes its remainder from the **source text after the substitution**, and the
parts still spell the embedded `"` that `SX_STRIPDQ` had removed — hence the
stray `"` and the doubled tail in the table above.

bash hands on a *string*, so osh now does too: `stripdq_text` builds `temp` and
`operand_rhs_read` re-lexes it with `dquote_word_from_source`, exactly as
`expand_string_for_rhs (temp, quoted, …)` re-reads it from the start
(subst.c:7737). Where `temp` comes out equal to the operand's own source there is
no rewrite and the parser's word is kept, so the common case costs one comparison.

What the copy itself does needed no code: `$` + `string[i+1]` + `ret` +
`string[si]` is the original text from the `$(` through `si` byte for byte
(parse.y:4348-4376 hands back the read text less its last byte and leaves `si` on
that byte), so a failed read copies its region exactly as a successful one does —
only over a shorter region — and then **carries on with its ordinary rules** from
`si + 1`. That last part is what the old code got wrong by treating the remainder
as opaque: measured, `A${z:-p"$(fi⏎q)"\y"\z"r}B` under `@P` is `[Ap⏎q)\yzrB]`, the
`"` after the failure still closing the run that decides the two backslashes.

Corpus case `a-failed-read-in-a-used-brace-operand-is-copied-and-the-scan-goes-on.sh`
(4 sections, 13 probes). One of them, `now set`, falls out of the `=` row before
it and is worth keeping: an operand that is *not* used is still read once by the
brace scan (one report) and never by the rhs (no second or third).
