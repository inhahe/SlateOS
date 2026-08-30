# bash reads a `name[` in command position with
#
#     ttok = parse_matched_pair (cd, '[', ']', &ttoklen, P_ARRAYSUB);
#     if (ttok == &matched_pair_error)
#       return -1;                /* Bail immediately. */    /* parse.y:5148-5150 */
#
# and that scan knows nothing about the `$( … )` it may be standing in. So the
# subscript runs to *its* `]` however many `)`, newlines, `#`s or `<<`s stand in
# the way: `echo A$(h[1)2]=v)B` is a substitution whose one word is `h[1)2]=v`,
# closed by the *second* `)`. When it never closes, the pair bash names is the
# `]` — at the line the `[` stood on, `parse_matched_pair` taking
# `start_lineno = line_number` on entry (parse.y:3701).
#
# It is a subscript only where an assignment could have stood (parse.y:5145-5146):
#
#     else if MBTEST(character == '[' &&      /* ] */
#                    ((token_index > 0 && assignment_acceptable (last_read_token) &&
#                      token_is_ident (token, token_index)) || …
#
# `assignment_acceptable` is `command_token_position` less a `case` pattern, and
# `token_is_ident` runs `legal_identifier` over the token buffer *as written* —
# so `"f"[1` is not one, `f+[1` is not one, and `echo f[1` is not one. The same
# buffer decides what makes the *next* word a command word: `assignment` reads it
# for a `=`, a `+=` or a `[ … ]=`, and since the buffer holds the quotes, a
# quoted character in the name spoils an assignment (`"v"=1`) while one in the
# value cannot (`v="a b"`).
#
# Verified against bash 5.2.37.
#
# The rows are `eval`-wrapped so an unclosed `]` swallows only the string. Not
# included: `case x in f[1`, which is a fatal syntax error in bash — the pattern
# is a position where the `[` is text, so the reader meets the newline instead —
# and takes the rest of the script down with it.

echo "=== a subscript swallows the substitutions own close ==="
eval 'echo A$(h1[1)2]=v)B';        echo "1 rc=$?"
eval 'x=$(h2[1)2]=v); echo [$x]';  echo "2 rc=$?"
eval 'echo "$(h3[1)2]=v)"';        echo "3 rc=$?"
eval 'echo $(h4[0]=v; echo $((h4[0]+1)))'; echo "4 rc=$?"
eval 'echo $(h5[1] X)';            echo "5 rc=$?"

echo "=== and when it never closes, the ] is what is named ==="
eval 'echo $(f6[1';                echo "6 rc=$?"
eval 'echo $(time f7[1';           echo "7 rc=$?"
eval 'echo $(: ; f8[1';            echo "8 rc=$?"
eval 'echo $(echo $(f9[1';         echo "9 rc=$?"
eval 'echo "$(fa[1)""';            echo "10 rc=$?"
eval 'echo "$(fb[1)"a"b"';         echo "11 rc=$?"
eval 'echo $(fc[a"]"b';            echo "12 rc=$?"
eval 'echo $(fd[[1]';              echo "13 rc=$?"

echo "=== it is a subscript only where a name is ==="
eval 'echo $(echo fe[1';           echo "14 rc=$?"
eval 'echo $("ff"[1';              echo "15 rc=$?"
eval 'echo $(fg+[1';               echo "16 rc=$?"
eval 'echo $(1fh[1';               echo "17 rc=$?"
eval 'echo $([1';                  echo "18 rc=$?"
eval 'echo $(fi1[1] fj[2';         echo "19 rc=$?"

echo "=== and only where a command may begin ==="
eval 'echo $(v=1 fk[2';            echo "20 rc=$?"
eval 'echo $(v="a b" fl[2';        echo "21 rc=$?"
eval 'echo $(v[0]=1 fm[2';         echo "22 rc=$?"
eval 'echo $(v+=1 fn[2';           echo "23 rc=$?"
eval 'echo $("v"=1 fo[2';          echo "24 rc=$?"
eval 'echo $(2>/dev/null fp[1';    echo "25 rc=$?"
eval 'echo $(true && fq[1';        echo "26 rc=$?"
eval 'echo $(true | fr[1';         echo "27 rc=$?"
eval 'echo $(coproc fs[1';         echo "28 rc=$?"
eval 'echo $(case x in x) ft[1';   echo "29 rc=$?"

echo "=== nothing inside it introduces anything ==="
eval 'echo $(fu[<<E';              echo "30 rc=$?"
eval 'echo $(fv[1 #';              echo "31 rc=$?"
eval 'echo $(fw[1
2]=v; echo Z)';                    echo "32 rc=$?"

echo TAIL
