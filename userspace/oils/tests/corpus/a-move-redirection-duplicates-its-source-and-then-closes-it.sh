# `N>&M-` is bash's *move*: duplicate M onto N, then close M — one redirection
# spelled with a trailing `-`. It is the tidy form of the descriptor shuffle
# that would otherwise be written `N>&M M>&-`, and it means exactly that, with
# one exception: a descriptor moved onto *itself* is left open, where the two-
# redirect spelling would close it.
#
# The `-` belongs to the source *text*, not to what the word expands to. bash
# sorts the word at parse time, so:
#
#   - `>&$v-` is a move whatever `$v` holds;
#   - `>&$v` with `v=3-` is *not* one, and on fd 1 names a file called `3-`;
#   - the `-` has to be unquoted, by the same rule that keeps `>&"2"` a word:
#     `>&3"-"` is the filename `3-`, while `>&"3"-` is a move.
#
# Being a redirection like any other, a move on a command is undone when the
# command ends — only `exec` leaves the close behind. And like `r_duplicating_*`
# before it, a move reports a bad descriptor by *number*: the bare-number form
# names its source, and every other form names the redirector, even where the
# plain word form would have named the word.

echo '=== a move duplicates the source and then closes it'
( exec 3>&1; echo moved 1>&3-; echo tail )
( exec 3>&1; echo x 1>&3-; echo "rc=$?" )
( exec 3>&2; echo e 2>&3-; echo "rc=$?" )

echo '=== but a command undoes it, so the source is back afterwards'
( exec 3>&1; echo a 1>&3-; echo b >&3 )
( exec 3>&1; { echo c 1>&3-; }; echo d >&3 )
( exec 3>&1; while :; do echo w 1>&3-; break; done; echo e >&3 )

echo '=== `exec` has nothing to undo, so the close is the one it is left with'
( exec 3>&1; exec 1>&3-; echo persistent; echo "rc=$?"; echo x >&3; echo "rc=$?" ) 2>&1
( exec 3>&1; exec 4>&3-; echo a >&4; echo b >&3 ) 2>&1
( exec 3>&1 4>&1; exec 1>&3- 2>&4-; echo both )

echo '=== a descriptor moved onto itself is left open'
( exec 3>&1; echo same 3>&3-; echo after >&3 )
( exec 3>&1; exec 3>&3-; echo after >&3 )
( exec 3>&1; exec 3>&3 3>&-; echo after >&3 ) 2>&1

echo '=== the dash is the source text, never the expansion'
( v=3; exec 3>&1; echo w 1>&$v-; echo tail )
( v=3; exec 3>&1; echo w 1>&${v}-; echo tail )
( exec 3>&1; echo q 1>&"3"-; echo tail )
mkdir plain
( cd plain; v=3-; exec 3>&1; echo w 1>&$v; echo "tail=$?" )
echo "plain=[$(ls plain)]"
mkdir quoted
( cd quoted; exec 3>&1; echo q 1>&3"-"; echo "tail=$?" )
echo "quoted=[$(ls quoted)]"

echo '=== an input move is the same thing on the read side'
( exec 3<&0; cat <&3- </dev/null; echo tail )
( exec 3<&0; cat 0<&3- </dev/null; echo tail )

echo '=== a varfd move allocates as usual'
( exec 3>&1; exec {v}>&3-; echo hi >&$v )

echo '=== a bad source is reported by number, and the redirector otherwise'
( true 3>&9-; echo "rc=$?" )
( true 5>&9-; echo "rc=$?" )
( /bin/true 1>&9-; echo "rc=$?" )
( cat <&9-; echo "rc=$?" )
( v=9; /bin/true 3>&$v-; echo "rc=$?" )

echo '=== a plain dup is untouched by any of it'
( exec 3>&1; echo p 1>&3; echo q >&3 )
( true 1>&9; echo "rc=$?" )
( v=9; true 1>&$v; echo "rc=$?" )
( true 1>&"9"; echo "rc=$?" )
( true 7<&007; echo "rc=$?" )

echo '=== and it prints back the way it was written, with the fd always shown'
f1(){ true 1>&3-; }; f2(){ true >&3-; }; f3(){ true <&3-; }
f4(){ true 3>&4-; }; f5(){ true 1>&$v-; }; f6(){ true 1>&"3"-; }
f7(){ true >&2; };   f8(){ true >&-; };    f9(){ true 1>&$v; }
declare -f f1 f2 f3 f4 f5 f6 f7 f8 f9 | grep true

echo '=== done'
