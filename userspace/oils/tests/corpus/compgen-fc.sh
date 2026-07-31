# compgen's two live-completion sources: -F (a shell function) and -C (a
# command). Both run outside a completion here, which is what the warning on
# stderr is about, and both hand back their matches unnarrowed by the word.

f() {
  echo "args=[$1][$2][$3]"
  echo "env=[$COMP_LINE][$COMP_POINT][$COMP_TYPE][$COMP_KEY][$COMP_CWORD][${#COMP_WORDS[@]}]"
  COMPREPLY=(alpha beta)
}
echo "== plain"
compgen -F f wo
echo "rc=$?"

echo "== the completion environment does not outlive the call"
declare -p COMP_LINE COMP_CWORD COMP_WORDS 2>&1 | sed 's/^.*declare:/declare:/'

echo "== COMPREPLY is read as it stands, then removed"
g() { :; }
COMPREPLY=(kept)
compgen -F g q
declare -p COMPREPLY 2>&1 | sed 's/^.*declare:/declare:/'

echo "== a scalar COMPREPLY is one match; an associative one is none"
s() { COMPREPLY=one; }
a() { declare -A COMPREPLY=([k]=v); }
compgen -F s q
compgen -F a q
echo "rc=$?"

echo "== 124 asks for a rebuild there is none of"
r() { COMPREPLY=(zz); return 124; }
compgen -F r q
echo "rc=$?"

echo "== the function runs in this shell, so its changes stand"
h() { seen=$2; COMPREPLY=(k); }
compgen -F h word > /dev/null
echo "seen=$seen"

echo "== an undefined function"
compgen -F nosuchfunc q
echo "rc=$?"

echo "== the completion words are appended to the -C text, quoted"
compgen -C 'printf "[%s]\n"' 'a b'
compgen -C 'echo hi' x

echo "== blank output lines are dropped, the rest kept verbatim"
compgen -C 'printf "zz\n\n  sp  \nqq"' a

echo "== -C is a subshell and its status is ignored"
y=outer
compgen -C 'y=inner; echo z' q
echo "y=$y"
compgen -C 'exit 3' q
echo "rc=$?"

echo "== source order: actions, then -W, then -F, then -C; -X and -P apply"
compgen -k -W 'iw' -F s -C 'echo ic' -X 'in' -P '<' i
echo "rc=$?"

echo "== one slot each: the last -F and the last -C win"
t() { COMPREPLY=(tt); }
compgen -F s -F t -C 'echo c1' -C 'echo c2' q
