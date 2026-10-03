### TD-OILS-A-CMDSUB-BODY-IS-RE-READ-AS-WRITTEN-NOT-AS-REPRINTED — 2026-08-05 — ✅ **RESOLVED 2026-08-08**

> **RESOLVED 2026-08-08.** osh now performs bash's second parse. At expansion
> time `Shell::command_sub_body_inner` runs the stored re-print back through
> `parser::parse_cmdsub_body_unmarked` (`Shell::comsub_reparse_error`,
> `interp.rs`), and on failure emits bash's two-line diagnostic and unwinds.
> Covered byte-for-byte by
> `tests/corpus/a-cmdsub-body-is-parsed-twice-and-the-second-parse-reads-the-reprint.sh`.
>
> Three things the spec below got wrong, all corrected against fresh
> measurements (the measurement wins):
>
> - **The line is not `close_line + N`.** It is the *executing command's*
>   line plus the newlines the failed parse consumed, plus one — bash's one
>   shared `line_number`, which `parse_string` does not renumber (it calls
>   `push_stream (0)`). The old formula agreed in the common case only
>   because a simple command's line is stamped in `make_bare_simple_command`
>   (make_cmd.c:505) at the reduction that needs the *next* word as
>   lookahead, so reading the word holding the substitution is what set it.
>   Two shapes separate them and both report `line 2`:
>   `cat > "/dev/null$(⏎!⏎)x" <<< hi` (stamped at the `>`,
>   execute_cmd.c:864) and a `case` pattern (`case_command->line`,
>   execute_cmd.c:3545).
> - **There are two abort classes, not one.** The re-print's *own* grammar
>   error is `parse_string`'s `-DISCARD` (builtins/evalstring.c:686-694)
>   re-raised as `jump_to_top_level (DISCARD)` (parse.y:4325) — one parse
>   unit lost, `$?` 1, caught by `eval`. A `$( … )` **nested** in the
>   re-print that will not parse is raised by `parse_comsub` itself, which
>   does `jump_to_top_level (FORCE_EOF)` (parse.y:4185) for a
>   non-interactive shell — **the shell ends**, `eval` does not contain it,
>   and a subshell exits 2 rather than the 1 an interceptable abort leaves.
>   `parse_cmdsub_body_unmarked` exists to keep the two distinguishable.
> - **The tail is a property of the word, and the post-pass shape guessed
>   below is what landed** — `unparse::attach_comsub_tails`, called from
>   `parser::word_from_segs_in`, filling `CmdSubBody::Parsed::tail`. It
>   works by swapping each substitution out for a byte-string sentinel that
>   the rendering does not already contain and re-rendering the word, which
>   is exact because `part_src` renders a parsed body from `prog` and so
>   there is nothing in `src` for a marker to ride in on.
>
> Two things found while measuring this got entries of their own —
> TD-OILS-A-COMPOUND-ARRAY-ASSIGNMENT-IS-RE-PARSED-UNDER-ITS-OWN-INPUT-NAME
> and TD-OILS-A-COMPOUND-COMMANDS-LINE-IS-STAMPED-AT-ITS-KEYWORD — and both
> are now resolved.

> **Re-scoped 2026-08-08.** The heading and the "proper fix" below are both out
> of date. `CmdSubBody::Parsed::src` **is** the re-print already —
> `parser.rs` builds it with `let src = crate::unparse::comsub_body(&prog);`
> and osh's re-print is byte-identical to bash's, `declare -f` included:
> both print `x=$(! );` and `y=$(time );`. So the round trip the entry asked
> for has been made, and it did not close the divergence.
>
> What is actually missing is bash's **second parse**. bash parses a `$( … )`
> twice: `parse_comsub` at parse time (over the source, keeping the re-print)
> and `xparse_dolparen` again at *expansion* time (subst.c:1290/1322, over the
> stored re-print, with `)` as `shell_eof_token`). It is that second parse the
> bare prefix fails, and osh has no equivalent — it runs `src` straight off.
> Everything below the horizontal rule still describes the symptom correctly.
>
> **Measured spec for the missing check** (bash 5.2.37, all rows verified):
>
> - **When.** At expansion, not at parse: `f() { echo $(<newline>!<newline>); }`
>   defines cleanly and only fails when `f` is called, and a body behind a
>   short-circuit is never checked at all.
> - **Effect.** `jump_to_top_level(DISCARD)` in the *parent*, below
>   `parse_and_execute`'s guard — so it abandons the rest of the enclosing
>   **parse unit** and leaves `$?` at 1, but an `eval` around it would catch
>   it. `v=p$(<newline>!<newline>)q; echo after` prints no `after`;
>   the same two commands on separate lines do print it. This is
>   `Shell::arm_discard(1)`.
> - **Line.** `close_line + N`, where `N` is the number of lines in the
>   re-print — i.e. one *past* the body's last line, since body line 1 is
>   `close_line`. Verified over `N` = 1, 2 and 4 and four different
>   `close_line`s.
> - **Text.** Two lines, both prefixed `<script>: command substitution: line
>   <close_line + N>:`, reading ``syntax error near unexpected token `)' ``
>   and then `` `<last line of the re-print>)<rest of the word text>' ``.
>   The tail is the rest of the **word**, not of the physical line, and it
>   includes the word's closing quote: `echo "A[$(…)]B" more args` echoes
>   `` `! )]B"' `` — no `more args`; `echo X$(…)Y $(echo z) tail` echoes
>   `` `! )Y' ``; `v=p$(…)q; …` echoes `` `! )q' ``. That is
>   `expand_word_internal` handing `extract_command_subst` the remainder of
>   the stored word string, and the reporter echoing the current line of it.
> - **Not this.** The one-line `$( ! )` is rejected by `parse_comsub` at parse
>   time instead, fatally and with the physical source line echoed — and osh
>   already matches that byte for byte.
>
> **Implementation note.** The tail is the one part osh cannot read off an
> existing field: `seg_to_part` sees one `Seg` and not its siblings. The natural
> shape is a post-pass over the assembled `Word` (in `word_from_segs_in`, which
> does have the whole list) that walks each `CommandSub` and fills a new
> `CmdSubBody::Parsed::tail` with the unparse of everything after it — siblings
> first, then the enclosing container's closer, recursively outwards.

---

**Where:** `userspace/oils/src/ast.rs` — `CmdSubBody::Parsed::src`, which holds
the `$( … )` body *as written*; `userspace/oils/src/parser.rs` —
`parse_cmdsub_body`, which produces it.

**What.** bash does not keep a `$( … )` body's text. `parse_comsub` (parse.y
4193) throws it away and **re-prints the command it just parsed**:

```c
tcmd = print_comsub (parsed_command);   /* returns static memory */
…
ret[retlen++] = ')';
```

so the text re-read at expansion time is the *deparse* plus a `)`. osh keeps the
written text instead. The round trip is invisible almost everywhere — comments
vanish either way, words are re-printed verbatim, an alias survives unexpanded
— but it is not invisible when the deparse does not parse:

```sh
echo $(
!
)
```

```text
bash: line 4: syntax error near unexpected token `)'   line 4: `! )'
osh:  (silence)
```

The deparse of a bare `!` is `!` with no terminator, and bash's grammar only
admits a bare prefix as `BANG list_terminator` — a `;` or a line end (see
`tests/corpus/prefix-bang-time.sh`). Appending `)` therefore yields `! )`, which
is exactly what bash then reports against. `time` behaves the same
(`time )`), and the two are the only productions that drop their own
terminator, so this is the whole of the divergence rather than one case of it.

**When it bites.** Only when the *last* command of the body is a bare prefix.
`$( ! )` written on one line already agrees (the `)` is not a list terminator
either way), and a bare `!` that is not last, or one inside a compound the
deparse closes (`{ !; }`, `if … fi`, a function, `while … done`,
`case … esac`), is fine in both.

The error is raised at **expansion** time, not parse time, and is not fatal to
the script — the substitution's command is abandoned and the shell reads on:

```sh
echo $(
!
)
echo after      # bash prints the error, then `after'
```

**Where the echoed line comes from.** bash splices the re-read text into the
current input with `shell_ungets` (parse.y 2758), so the echoed line is the
deparse followed by whatever was left of the physical line:
`echo "[$(<newline>!<newline>) ]"` echoes `! ) ]"`.

**The proper fix** is to make the re-read text the deparse, as bash's is:
`CmdSubBody::Parsed::src` becomes the printed form of `prog` with a `)` on the
end. osh already has a command printer (`declare -f`), but bash's `print_comsub`
is a *different* printer to the one `declare -f` uses, and the swap puts every
`$( … )` in the shell through a round trip — so it wants its own measurement
pass before it is made, not a bolt-on. A narrower change that merely rejected a
trailing bare prefix would encode the consequence without the cause.

**Impact.** osh accepts a script bash rejects; the substitution then expands to
the body's output rather than to nothing. Confined to a `$( … )` whose last
command is a bare `!` or `time`.

**Found by** the probe matrix for
TD-OILS-A-SUBSTITUTION-INSIDE-AN-ALIAS-IS-BLAMED-ON-ITS-OWN-LINE-1.
