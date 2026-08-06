# `[N]>&WORD` and `[N]<&WORD` are the only redirects whose *instruction* is
# decided after the expansion. bash rewrites them in `do_redirection_internal`,
# and the ladder it uses is narrower than the obvious "a number dups, anything
# else is a filename":
#
#   1. the word expanded to no word at all (unset-and-unquoted, or split into
#      several) — a **move** becomes a close of its own redirector, and every
#      other form is `ambiguous redirect`. bash's comment credits ksh93;
#   2. the word is exactly `-` — a close, however it was spelled;
#   3. the word is all digits — the numbered dup or move (an empty word is
#      vacuously all-digits, and so is a bad descriptor rather than a name);
#   4. `>&WORD` on fd 1 *only*, and only as a plain dup, not a varfd and not a
#      move — both streams to a file of that name;
#   5. anything else — `ambiguous redirect`.
#
# Rule 4 is the narrow one: `1>&nope` creates the file, but `1>&nope-` is
# ambiguous, because a move is a different instruction and never reaches it.
#
# A `{v}` redirect adds a second twist. Once rules 1 or 2 have turned it into a
# close it is no longer an allocation: bash takes the descriptor out of `$v` —
# the `{v}>&-` form — so it stores nothing, and complains about `v` when the
# variable holds no descriptor. And a `{v}` redirect that fails with one of
# bash's *own* error codes names the variable rather than the word, which is
# `redirection_error`'s first rule: `{v}>&nope` says `v`, not `nope`. A real
# `errno` is not one of those codes, so `{v}> /nodir/x` still names the path.

echo '=== 1. no word at all: a move closes its redirector, everything else does not'
( v=; true 1>&$v-; echo "rc=$?" )
( v=; true 3>&$v-; echo "rc=$?" )
( v=; true 1<&$v-; echo "rc=$?" )
( v="a b"; true 1>&$v-; echo "rc=$?" )
( v=; true 1>&$v; echo "rc=$?" )
( v=; true 3>&$v; echo "rc=$?" )
( v="a b"; true 3>&$v; echo "rc=$?" )
( exec 3>&1; echo lost 1>&$v-; echo "rc=$?" ) 2>&1
( exec 3>&1; echo kept 1>&$v-; echo back >&3 ) 2>&1

echo '=== a quoted empty word is one word, so it is a bad descriptor instead'
( true 1>&""-; echo "rc=$?" )
( true 3>&""-; echo "rc=$?" )
( true 3>&""; echo "rc=$?" )

echo '=== 4. only a plain `>&WORD` on fd 1 is a file'
mkdir dup && ( cd dup; true 1>&nope; echo "rc=$?" ); echo "dup=[$(ls dup)]"
mkdir mv && ( cd mv; true 1>&nope-; echo "rc=$?" ); echo "mv=[$(ls mv)]"
mkdir fd3 && ( cd fd3; true 3>&nope; echo "rc=$?" ); echo "fd3=[$(ls fd3)]"
mkdir inp && ( cd inp; true 1<&nope; echo "rc=$?" ); echo "inp=[$(ls inp)]"
mkdir vf && ( cd vf; true {v}>&nope; echo "rc=$?" ); echo "vf=[$(ls vf)]"
( true 3>&"nope"-; echo "rc=$?" )

echo '=== a varfd that becomes a close stops being an allocation'
( e=-; true {v}>&$e; echo "rc=$? v=$v" )
( e=-; v=3; exec 3>&1; true {v}>&$e; echo "rc=$? v=$v"; echo back >&3 )
( v=3; exec 3>&1; true {v}>&$nope-; echo "rc=$? v=$v"; echo back >&3 )
( v=3; exec 3>&1; exec {v}>&$nope-; echo "rc=$? v=$v"; echo gone >&3 ) 2>&1
( v=1; e=-; echo hi {v}>&$e; echo "rc=$? v=$v" )
( v=3; exec 3>&1; echo a {v}>&-; echo "rc=$? v=$v"; echo back >&3 )

echo '=== a varfd names its variable for the errors bash raises itself'
( true {v}>&nope; echo "rc=$?" )
( true {v}<&nope; echo "rc=$?" )
( w="a b"; true {v}>&$w; echo "rc=$?" )
( w="a b"; true {v}> $w; echo "rc=$?" )
( w=; true {v}> $w; echo "rc=$?" )
( true {v}>&nope-; echo "rc=$?" )
: > exist
( set -C; true {v}> exist; echo "rc=$?" )
( set -C; exec {v}> exist; echo "rc=$?" ) 2>&1
echo '--- but not for an errno, and not without the varfd'
( true {v}> /nodir/x; echo "rc=$?" )
( true > /nodir/x; echo "rc=$?" )
( set -C; true > exist; echo "rc=$?" )
( w=; true > $w; echo "rc=$?" )

echo '=== `exec` reports the same ambiguity a command does'
( v=; exec 3>&$v; echo "rc=$?" ) 2>&1
( v="a b"; exec 3>&$v; echo "rc=$?" ) 2>&1
( v=; exec > $v; echo "rc=$?" ) 2>&1
( v="a b"; exec > $v; echo "rc=$?" ) 2>&1
( v=; exec 3>&"$v"; echo "rc=$?" ) 2>&1

echo '=== done'
