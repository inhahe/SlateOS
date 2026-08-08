# bash labels a syntax error with *two* names: the one the diagnostic is blamed
# on (`$0`, or `BASH_SOURCE[0]` inside a sourced file) and the one it is being
# *read* from. The second is a stack — one entry per open input — and only the
# innermost is shown, so each construct here takes the name over from the one
# enclosing it and hands it back on the way out. When the two names coincide the
# second is dropped, which is why an ordinary script file reports its path once.
#
# Every probe runs in a subshell or under `eval`, because a syntax error
# abandons the rest of the input it was read from — at the top level that would
# be the case itself.
printf 'echo insub\nfi\necho tail\n' > bad.sh
printf 'eval "fi"\n' > evl.sh

echo "=== a sourced file names itself, so the name appears once"
. ./bad.sh; echo "rc=$?"
# Reached through an `eval`, the file still takes the name over from it.
eval '. ./bad.sh'; echo "rc=$?"
# …and hands it back: an `eval` *inside* the file differs from the file again.
. ./evl.sh; echo "rc=$?"

echo "=== eval and a substitution body carry their own names"
eval 'fi'; echo "rc=$?"
x=`fi`; echo "rc=$?"
# A substitution body is pushed on top of the `eval` that wrote it…
eval 'y=`fi`'; echo "rc=$?"
# …and an `eval` the body itself runs is pushed on top again.
z=`eval 'fi'`; echo "rc=$?"

echo "=== a trap handler is read under the trap's own name"
( trap "'" EXIT ); echo "rc=$?"
( trap "'" DEBUG; : ); echo "rc=$?"
( trap "'" ERR; false ); echo "rc=$?"
( f() { trap "'" RETURN; :; }; f ); echo "rc=$?"
# A handler that parses is read under the same name, so the failure inside it is
# blamed there too.
( trap 'q=$(fi)' EXIT ); echo "rc=$?"

echo "=== and the enclosing name comes back afterwards"
. ./bad.sh; eval 'fi'; echo "done"
