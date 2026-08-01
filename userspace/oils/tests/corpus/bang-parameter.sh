# When the `!` of `${!…}` is the parameter itself rather than an indirection.
#
# bash reads `${!…}` as an indirection only when what follows the `!` could
# name a parameter — a name, a digit, or one of `# ? @ *`. Otherwise the `!` is
# `$!`, the last background job's pid, and the rest is an operator on it.
sleep 0.3 &
bg=$!
# The pid itself varies, so every check below is against `$!`.
[ "${!}" = "$bg" ] && echo 'bare      ok'
[ "${!-x}" = "$bg" ] && echo 'default   ok'
[ "${!:-x}" = "$bg" ] && echo 'cdefault  ok'
[ "${!+x}" = "x" ] && echo 'alternate ok'
[ "${!:+x}" = "x" ] && echo 'calt      ok'
[ "${!=x}" = "$bg" ] && echo 'assign    ok'
[ "${!:?m}" = "$bg" ] && echo 'cerror    ok'
[ "${!%${bg#?}}" = "${bg%${bg#?}}" ] && echo 'suffix    ok'
[ "${!/${bg}/z}" = "z" ] && echo 'replace   ok'
[ "${!:0:1}" = "${bg%${bg#?}}" ] && echo 'slice     ok'
[ "${#!}" = "${#bg}" ] && echo 'length    ok'
[ "${!^}" = "$bg" ] && echo 'upper     ok'
[ "${!,}" = "$bg" ] && echo 'lower     ok'
wait

# A special parameter as the *referent* — its value is the name indirected
# through — and it may carry a modifier just as a plain name may.
set -- one two
one=ONEVAL
two=TWOVAL
echo "[${!1}][${!1,,}][${!1#O}][${!#}][${!#:-z}][${!#@Q}][${!?+set}]"

# What is left over must still open an operator, or there is no reading at all.
for b in '${!$}' '${!!}' '${!)}' '${! }' '${![0]}' '${!@Q}' '${!*Q}' '${!#1}' \
         '${!?m}' '${!#^}'; do
  ( eval "echo \"[$b]\"" ) 2>&1 | sed 's/^.*: line [0-9]*: //'
done

# The positional list as referent indirects through each *value*; a modifier
# does not change that, so both report the same refusal.
( echo "[${!@}]" ) 2>&1 | sed 's/^.*: line [0-9]*: //'
( echo "[${!@:-z}]" ) 2>&1 | sed 's/^.*: line [0-9]*: //'
echo "=== done"
