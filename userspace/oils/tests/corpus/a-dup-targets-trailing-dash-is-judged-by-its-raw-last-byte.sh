# Whether `>&word` is a *move* is settled by the parser, on the word's raw last
# byte, before any quote has been removed. `make_redirection` (`make_cmd.c`)
# does it in four lines whose own comment is `/* Yuck */`:
#
#     w = dest_and_filename.filename;
#     wlen = strlen (w->word) - 1;
#     if (w->word[wlen] == '-')		/* Yuck */
#       { w->word[wlen] = '\0';
#         if (all_digits (w->word) && legal_number (w->word, &lfd) && …)
#           → r_move_output          /* the source is a plain number */
#         else
#           → r_move_output_word;    /* …or a word still to be expanded */ }
#
# Three consequences, none of them obvious:
#
#   * the dash cannot come out of an expansion (`v=3-; >&$v` names a *file*
#     `3-`), and it cannot be hidden by quoting it (`>&3"-"` is that file too,
#     because the raw last byte is the closing quote);
#   * but a *backslash* leaves the dash last after all, so `>&3\-` is a move
#     where `>&3"-"` is not. What is left once the byte is taken off is a
#     dangling `\`, which expands to nothing;
#   * and there is no exception for a dash that is already doubled: only the
#     final byte comes off, so `>&x--` is a move whose source is `x-`.
#
# A lone `>&-` never reaches any of that. The *lexer* returns the `-` as a token
# of its own the moment it follows `<&`/`>&` (`read_token`, `parse.y`), and the
# grammar has literal `'-'` productions to receive it — so the target is exactly
# `-` and whatever follows begins a fresh word.
#
# `declare -f` is the witness for the classification, because the printer spells
# each instruction differently: a close is always `%d>&-` (redirector shown, and
# `>&` even for `<&`), a move is `%d>&%s-` (shown), and a plain dup word is
# `>&%s` with the redirector *omitted* when it is the default.

echo '=== the lexer takes a leading dash before a word is ever collected'
f() { true 1>&--; };    declare -f f
f() { true 1>&-x; };    declare -f f
f() { true 1>&-abc; };  declare -f f
f() { true 1<&--; };    declare -f f
f() { true {v}>&--; };  declare -f f
f() { true 1>& --; };   declare -f f
f() { true 1>&-- -q; }; declare -f f
f() { true 1>&-; };     declare -f f

echo '=== …but only a leading one: a word swallows the rest'
f() { true 1>&2-x; };   declare -f f
f() { true 1>&x--; };   declare -f f
f() { true 1>&$v--; };  declare -f f
f() { true 1>&"x"--; }; declare -f f
f() { true 1>&""--; };  declare -f f

echo '=== a quoted dash is not the raw last byte, so it is no move'
f() { true 1>&3"-"; };  declare -f f
f() { true 1>&3'-'; };  declare -f f
f() { true 1>&'-'; };   declare -f f
f() { true 1>&"-"; };   declare -f f

echo '=== an escaped one is, and the backslash it leaves expands to nothing'
f() { true 1>&3\-; };   declare -f f
f() { true 1>&\-; };    declare -f f
f() { true 1>&x\-; };   declare -f f
f() { true 1>&\--; };   declare -f f
f() { true 1>&\3\-; };  declare -f f
f() { true 1>&"3"\-; }; declare -f f

echo '=== and the same words, performed'
# fd 1 is closed by several of these, so each runs in a subshell that reports
# through a saved copy rather than through the descriptor under test.
run() { ( exec 9>&1; eval "true 1>&$1" 2>&1; echo "  [$1] rc=$?" >&9 ) 2>&1; }
v=3
for w in '-' '--' '-x' '3-' '3"-"' "3'-'" '3\-' '\-' 'x\-' '\--' \
         'x-' 'x--' '$v-' '$v--' '"x"--' '""-' '""--' '"3"-' '"3"\-'; do
  run "$w"
done

echo '=== a close still closes, whatever came after it'
( exec 3>&1; echo z 1>&--; echo "rc=$?" >&3 ) 2>&1
( exec 3>&1; echo z 1>&-x; echo "rc=$?" >&3 ) 2>&1
( echo a 1>&-- 2>/dev/null; echo "rc=$?" )

echo '=== and the argument it left behind is really an argument'
( printf '[%s]\n' 1>&-- 2>/dev/null; echo "rc=$?" )
( exec 3>&1; printf '[%s]\n' 1>&-q 1>&3 )
( exec 3>&1; printf '[%s]\n' 1>&-- -x 1>&3 )

echo '=== done'
