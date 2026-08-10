# A `[` at the head of a word, where an assignment could stand, is one of the
# *reader's* matched pairs — not text that happens to hold a bracket. bash reads
# it in `read_token_word` (parse.y:5145-5152):
#
#       else if MBTEST(character == '[' &&		/* ] */
#                      ((token_index > 0 && assignment_acceptable (last_read_token) && token_is_ident (token, token_index)) ||
#                       (token_index == 0 && (parser_state&PST_COMPASSIGN))))
#         {
#           ttok = parse_matched_pair (cd, '[', ']', &ttoklen, P_ARRAYSUB);
#           if (ttok == &matched_pair_error)
#             return -1;		/* Bail immediately. */
#
# so a subscript that never closes is a syntax error found while *reading*:
# everything from the `[` on is swallowed by the search for the `]`, and what
# comes out is `unexpected EOF while looking for matching `]'` with status 2.
# Whatever stood before the `[` has already been read and run — `echo x; f[1`
# prints `x` first — and a blank does not end the run, so `f[1 x` is still one
# unclosed subscript.
#
# `parse_matched_pair` reports at the line the pair *opened* on
# (`parser_error (start_lineno, …)`, parse.y:3711, with `start_lineno =
# line_number` taken on entry at :3701), so a subscript that runs off the end
# over several lines still names the line its `[` stood on.
#
# The two halves of the guard are what decides whether a `[` is read this way at
# all. `assignment_acceptable(t)` is `command_token_position (t) &&
# (parser_state & PST_CASEPAT) == 0` (parse.y:2989) — so the `[` must open a
# word where a command, or a further assignment, could begin, and a `case`
# pattern is excluded however much it looks like one. `token_is_ident` is
# `legal_identifier` on the token read so far (parse.y:4846), so the text before
# the `[` must be a name. Past the first word of a command there is nothing to
# test for and the `[` is ordinary text; the `PST_COMPASSIGN` arm is the other
# way in, an element of a compound literal, where the name may be empty.
#
# An inner construct opened inside the subscript is the one that is named: the
# search steps over quotes and substitutions whole, so `f["]` runs off the end
# looking for a `"` and `f[$(echo 1` for a `)`. A `]` inside quotes does not
# close the subscript.
#
# Verified against bash 5.2.37.
#
# Each row is `eval`-wrapped so the search for the `]` swallows only the string.

i=1

echo "=== an unclosed subscript is a reader error ==="
eval 'f2[b[$i]=R';   echo "1 rc=$?"
eval 'f3[1=R';       echo "2 rc=$?"
eval 'f6[1 x';       echo "3 rc=$?"
eval 'echo x; f7[1'; echo "4 rc=$?"
eval 'f8[1;';        echo "5 rc=$?"
eval 'f9x[1#';       echo "6 rc=$?"

echo "=== and it is reported at the line the [ opened on ==="
eval $'echo a\nf10[1\n\n\n'; echo "7 rc=$?"
eval $'echo b\nf11[1+\n2+\n3'; echo "8 rc=$?"

echo "=== only where an assignment could stand ==="
eval 'echo a[1';     echo "9 rc=$?"
eval 'declare f12[1';  echo "10 rc=$?"
eval 'f13[1]x[2';    echo "11 rc=$?"
eval 'x=1 f14[1';    echo "12 rc=$?"
eval '! f15[1';      echo "13 rc=$?"
eval 'a && f16[1';   echo "14 rc=$?"
eval 'f17[1 | g';    echo "15 rc=$?"
eval 'if f18[1; then :; fi'; echo "16 rc=$?"
eval '{ f19[1; }';   echo "17 rc=$?"
eval 'f20() { f21[1'; echo "18 rc=$?"
eval 'for f22[1';    echo "19 rc=$?"
eval '[[ f23[1 ]]';  echo "20 rc=$?"
eval '(( f24[1 ))';  echo "21 rc=$?"
eval 'case x in f25[1) :;; esac'; echo "22 rc=$?"

echo "=== and only after a name ==="
eval '1[1=R';        echo "23 rc=$?"
eval 'f-x[1';        echo "24 rc=$?"
eval '_f26[1';       echo "25 rc=$?"
eval 'f27[1]';       echo "26 rc=$?"

echo "=== the inner construct is the one named ==="
eval "f28[']'";      echo "27 rc=$?"
eval 'f29["]';       echo "28 rc=$?"
eval 'f30[$(echo 1'; echo "29 rc=$?"

echo "=== a compound literal's element is the other way in ==="
eval 'a1=([1';       echo "30 rc=$?"
eval 'a2=(x [1';     echo "31 rc=$?"
eval 'a3=(x [1]=y';  echo "32 rc=$?"
eval 'declare -A m1=([a'; echo "33 rc=$?"

echo TAIL
