# `${name:=word}` is an assignment wearing an expansion's clothes, and the two
# halves fail differently.
#
# When the *expansion* cannot name a destination at all — a positional, `$@`, a
# pointer that reaches nothing or reaches a name no variable could have — the
# command is abandoned with status **1**, like any other bad expansion.
#
# When a destination has been named and the *store* refuses it — a readonly
# variable, a subscript that names nowhere, a chain that leads nowhere — the
# status is **2**. That is what bash rates an assignment that fails while an
# expansion is in progress; a plain `q=v` to the same readonly is only 1, so the
# extra is the cost of the assignment happening inside an expansion.
#
# Where the readonly check sits is visible from two sides. It is *after* the
# subscript is judged — a readonly array given an out-of-range index complains
# about the subscript and never mentions the readonly — and *after* the default
# word is expanded, so that word's side effects have already happened by the
# time the refusal is printed.
#
# `:=` only assigns when it has to, so a readonly that is already non-null is
# never a refusal at all: the value is simply read.

echo '=== the expansion cannot name a destination: status 1'
( echo "[${1:=v}]"; echo not-reached ); echo "rc=$?"
( echo "[${@:=v}]"; echo not-reached ); echo "rc=$?"
( echo "[${*:=v}]"; echo not-reached ); echo "rc=$?"
( unset z; echo "[${!z:=v}]"; echo not-reached ); echo "rc=$?"
( z="b c"; echo "[${!z:=v}]"; echo not-reached ); echo "rc=$?"

echo '=== the store refuses: status 2'
( readonly q; echo "[${q:=v}]"; echo not-reached ); echo "rc=$?"
( readonly q; echo "[${q=v}]"; echo not-reached ); echo "rc=$?"
( readonly q=""; echo "[${q:=v}]"; echo not-reached ); echo "rc=$?"
( declare -ra w=(1); echo "[${w[3]:=v}]"; echo not-reached ); echo "rc=$?"
( declare -rA m=([k]=1); echo "[${m[j]:=v}]"; echo not-reached ); echo "rc=$?"
( readonly t; declare -n r=t; echo "[${r:=v}]"; echo not-reached ); echo "rc=$?"
( a=(1 2); echo "[${a[-9]:=v}]"; echo not-reached ); echo "rc=$?"
( q=(1 2); declare -n e=q[0]; echo "[${e[0]:=v}]"; echo not-reached ); echo "rc=$?"

echo '=== a function does not contain it either'
( f() { echo "[${q:=v}]"; echo not-reached-inner; }; readonly q; f; echo not-reached ); echo "rc=$?"

echo '=== the subscript is judged first, and the readonly never comes up'
( declare -ra w=(1 2); echo "[${w[-9]:=v}]" ); echo "rc=$?"
( declare -rA m=([k]=1); blank=; echo "[${m[$blank]:=v}]" ); echo "rc=$?"

echo '=== and the default word has already had its say'
( readonly q; echo "[${q:=$(echo SIDE >&2)}]" ); echo "rc=$?"

echo '=== a readonly that is already set is only ever read'
( readonly q=1; echo "[${q:=v}]"; echo reached ); echo "rc=$?"
( declare -ra w=(1 2); echo "[${w[1]:=v}]"; echo reached ); echo "rc=$?"
( declare -rA m=([k]=1); echo "[${m[k]:=v}]"; echo reached ); echo "rc=$?"

echo '=== a plain assignment to the same readonly is only worth 1'
( readonly q=1; q=2; echo not-reached ); echo "rc=$?"
( declare -ra w=(1); w[0]=2; echo not-reached ); echo "rc=$?"

echo '=== and the write builtins report it without abandoning anything'
( readonly q; printf -v q x; echo "rc2=$?"; echo reached ); echo "rc=$?"
( readonly q; echo z | { read q; echo "rc2=$?"; }; echo reached ); echo "rc=$?"

echo still here
