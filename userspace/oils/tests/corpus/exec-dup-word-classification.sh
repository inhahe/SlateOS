# `exec` applies its redirects to the shell's own descriptor table rather than
# to one command's, and it used to reach them by a different road: where an
# ordinary redirect asks whether every byte of the dup word is a *digit*, `exec`
# asked whether the word parses as a number. So it gave a different answer in
# each of the three places those two questions disagree — and, being persistent,
# went on giving it to every later command.
#
# This is therefore the `<&WORD`/`>&WORD` classification of
# dup-word-that-is-not-a-descriptor, asked again of `exec`:
#
#   * the empty word and a run of digits too long for a descriptor are *bad*
#     descriptors rather than ambiguous ones, and `exec 1>&""` is not an open of
#     a file whose name is empty;
#   * a sign makes a word non-numeric, so `exec 7<&"+3"` is ambiguous however
#     well `+3` parses, and must not quietly dup fd 3;
#   * and which of the word and the redirector a bad one names is settled the
#     same way too — the word as written where the operator's own descriptor is,
#     the redirector anywhere else, and a bare run of digits always the number
#     it spells.
#
# What the dup then *is* matters as well. It is a `dup2`, so `exec 7>&7` needs
# no source and succeeds with fd 7 closed, and `exec 3<&3` leaves fd 3's cursor
# where it was. The one spelling that is not a dup is the special filename:
# `> /dev/fd/7` is an `open`, and an open of a descriptor that is not there
# fails even when it is the very descriptor being redirected.
#
# Every probe runs in a subshell so a persistent redirect that does land cannot
# reach the next one.
#
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err
e=""
printf 'one\ntwo\nthree\n' > in
exec 3<in
exec 5>f5

echo "=== a word that parses as a number but is not all digits"
( exec 7<&"+3"; read -r l <&7; echo "  +3 read=[$l]" ); echo "  +3 rc=$?"
( exec 7>&"+5"; echo x >&7 );                            echo "  +5 rc=$?"
( exec 7<&" 3"; read -r l <&7; echo "  ' 3' read=[$l]" ); echo "  ' 3' rc=$?"

echo "=== all digits but no descriptor"
( exec 7<&"$e" );                   echo "  7<&'' rc=$?"
( exec 7>&"$e" );                   echo "  7>&'' rc=$?"
( exec 7<&"99999999999999999999" ); echo "  7<&big rc=$?"
( exec 7>&"99999999999999999999" ); echo "  7>&big rc=$?"
( exec 0<&"$e" );                   echo "  0<&'' rc=$?"
( exec 1>&"$e" );                   echo "  1>&'' rc=$?"

echo "=== a descriptor that is merely closed, named the same three ways"
( exec 7<&"9" ); echo "  7<&'9' rc=$?"
( exec 0<&"9" ); echo "  0<&'9' rc=$?"
( exec 7>&"9" ); echo "  7>&'9' rc=$?"
( exec 1>&"9" ); echo "  1>&'9' rc=$?"
( exec 7<&9 );   echo "  7<&9 bare rc=$?"

echo "=== nothing numeric about it at all"
( exec 7<&"abc" );          echo "  7<&abc rc=$?"
( exec 7>&"abc" );          echo "  7>&abc rc=$?"
( exec 1>&"abc"; echo hi ); echo "  1>&abc rc=$?"
echo "  abc=[$(cat abc 2>/dev/null)]"

echo "=== a descriptor is its own source, and duplicating it changes nothing"
( exec 7<&7; echo "  self-in ok" );  echo "  7<&7 rc=$?"
( exec 7>&7; echo "  self-out ok" ); echo "  7>&7 rc=$?"
( exec 3<&3; read -r l <&3; echo "  self-open read=[$l]" ); echo "  3<&3 rc=$?"

echo "=== but a special filename is an open, and an open needs the descriptor"
( exec 6>/dev/fd/5; echo A >&6 ); echo "  6>/dev/fd/5 rc=$?"
( exec 6>/dev/fd/8 );             echo "  6>/dev/fd/8 rc=$?"
( exec 6</dev/fd/8 );             echo "  6</dev/fd/8 rc=$?"
( exec 8>/dev/fd/8 );             echo "  8>/dev/fd/8 rc=$?"
( exec 8</dev/fd/8 );             echo "  8</dev/fd/8 rc=$?"

echo "=== which holds for an ordinary command's own redirects too"
echo B 8>/dev/fd/8;    echo "  8>/dev/fd/8 rc=$?"
read -r l 8</dev/fd/8; echo "  8</dev/fd/8 rc=$?"
echo C 8>&8;           echo "  8>&8 rc=$?"

exec 5>&- 3<&-
echo "  f5=[$(cat f5)]"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
