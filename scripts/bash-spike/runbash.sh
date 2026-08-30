#!/bin/bash
# Does the cross-compiled musl bash actually execute? Run it on Linux first —
# a static musl ELF runs unchanged there, so this isolates "did the port work"
# from "does SlateOS load it".
B=/tmp/bash-cross/bash
ls -l "$B"
file "$B"
echo "=== --version ==="
"$B" --version | head -2
echo "=== basic -c ==="
"$B" -c 'echo hello from cross bash; for i in 1 2 3; do echo "i=$i"; done; echo "major=${BASH_VERSINFO[0]}"'
echo "exit=$?"
echo "=== the corpus features osh was built for ==="
"$B" -c 'f(){ local a=("$@"); echo "${#a[@]} ${a[1]}"; }; f x y z'
"$B" -c 'declare -A m=([k]=v [j]=w); for k in "${!m[@]}"; do echo "$k=${m[$k]}"; done | sort'
"$B" -c 'set -- -a -b val; while getopts "ab:" o; do echo "opt=$o arg=$OPTARG"; done'
"$B" -c 'printf "%s|%05.2f|%q\n" abc 3.14159 "a b"'
"$B" -c 'echo "${x:-default}"; y=abcdef; echo "${y:2:3} ${y^^} ${y//c/Z}"'
"$B" -c 'echo $(( 2**10 + 7 % 3 )); echo {a..e} {1..3}'
echo "=== script from a file + stdin ==="
printf 'read -r line\necho "got:$line"\n' > /tmp/t.sh
echo piped | "$B" /tmp/t.sh
echo "=== job control (expected to be the sore spot) ==="
"$B" -c 'sleep 0.1 & wait; echo "bg+wait ok"'
"$B" -i -c 'echo interactive-c' 2>&1 | head -3
