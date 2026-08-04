# `OPTIND` is the shell's half of `getopts`' cursor: the caller sets it to start
# a scan over, and the builtin writes back where it got to. But the builtin does
# not scan from the number in the variable — it scans from what that number
# becomes on the way in, and two things happen to it there.
#
# It is read like C's `atoi`: leading whitespace, an optional sign, a run of
# digits, and *anything after the digits ignored*. So `OPTIND=3junk` is 3 and
# starts the scan at the third argument. This only ever shows after `unset
# OPTIND`, because while the variable keeps the integer attribute the shell
# gives it, it can only ever hold a canonical decimal.
#
# And the result lands in a plain C `int`. A value too big for one is not
# treated as "past the end" — it is whatever its low 32 bits say, which is why
# `OPTIND=4294967297` starts at the first argument all over again, and why
# `OPTIND=8589934594` starts at the second. Anything at or below zero once
# narrowed is no position at all, and the scan starts from the first argument.
#
# What is left over — a position genuinely past the last argument — is pulled
# back to one past it. The end of the options is *reported at the end*, rather
# than by parroting the number back: over two arguments, `OPTIND=99` answers 3.
# It is a clamp and not a rescan, so asking again gives the same answer; and it
# measures the arguments this call was given, so an explicit operand list is
# counted instead of the positionals.

echo "=== past the end is pulled back to one past the last argument"
OPTIND=99; set -- -a;       getopts "a" o;   echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=99; set -- -a -b -c; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=5;  set -- -a -b;    getopts "ab" o;  echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=4;  set -- -a -b;    getopts "ab" o;  echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=99; set --;          getopts "a" o;   echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== one past the last argument is already the end, and is left alone"
OPTIND=3; set -- -a -b; getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== it is a clamp, not a rescan"
OPTIND=99; set -- -a -b
getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"
getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== the arguments counted are this call's own"
set -- -a -b -c -d -e
OPTIND=99; getopts "ab" o x; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=7;  getopts "ab" o -a -b; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== the number is narrowed to a C int"
set -- -a -b -c
OPTIND=4294967297;  getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=8589934594;  getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=12884901891; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=2147483647;  getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=2147483648;  getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=4294967295;  getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== nothing positive is nowhere, and starts over"
set -- -a -b
OPTIND=0;  getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=-1; getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"
OPTIND=-9; getopts "ab" o; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== the variable holds an integer, so the arithmetic is the shell's"
OPTIND=999999999999999999999999; echo "raw=[$OPTIND]"
set -- -a; getopts "a" o; echo "rc=$? o=[$o] ind=$OPTIND"

echo "=== read like atoi once the attribute is gone"
set -- -a -b -c
unset OPTIND; OPTIND=3junk; echo "raw=[$OPTIND]"; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
unset OPTIND; OPTIND=abc;   echo "raw=[$OPTIND]"; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
unset OPTIND; OPTIND=" 2 "; echo "raw=[$OPTIND]"; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
unset OPTIND; OPTIND=+3;    echo "raw=[$OPTIND]"; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
unset OPTIND;               echo "raw=[${OPTIND-unset}]"; getopts "abc" o; echo "rc=$? o=[$o] ind=$OPTIND"
