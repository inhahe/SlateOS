# A `-u` operand is validated twice, and the two rejections are worded
# differently on purpose: a number that parses is a legal file-descriptor
# *specification*, so a descriptor the shell does not hold is refused one step
# later — by the attempt to use it — and carries the `errno` text instead.
# `read` and `mapfile` agree on both spellings.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== a descriptor that is not open"
( read -u 9 x; echo "rc=$?" ) 2>&1 | e
( read -u 88 x; echo "rc=$?" ) 2>&1 | e

echo "=== …versus something that is not a descriptor at all"
( read -u abc x; echo "rc=$?" ) 2>&1 | e
( read -u -1 x; echo "rc=$?" ) 2>&1 | e
( read -u '' x; echo "rc=$?" ) 2>&1 | e

echo "=== an open descriptor at end of file is silent"
( exec 3< /dev/null; read -u 3 x; echo "rc=$? x=[${x-unset}]" ) 2>&1 | e

echo "=== mapfile and readarray use the same words"
( mapfile -u 9 a; echo "rc=$?" ) 2>&1 | e
( readarray -u 9 a; echo "rc=$?" ) 2>&1 | e
( mapfile -u abc a; echo "rc=$?" ) 2>&1 | e
( readarray -u -1 a; echo "rc=$?" ) 2>&1 | e

echo "=== the complaint is the builtin's own output, so it is redirectable"
( read -u 88 v 2>&1 | cat ) 2>&1 | e
( mapfile -u 88 v 2>&1 | cat ) 2>&1 | e

echo "=== and nothing was assigned"
( read -u 88 v; echo "v=[${v-unset}]" ) 2>&1 | e
( mapfile -u 88 v; echo "v=[${v-unset}]" ) 2>&1 | e

echo "=== done"
