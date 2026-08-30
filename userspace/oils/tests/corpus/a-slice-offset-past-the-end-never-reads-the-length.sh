# The length of `${a[@]:off:len}` is read only once `off` landed *inside* the
# parameter. An offset past the end returns before the length is so much as
# evaluated: `${a[@]:99:-1}` is silent where `${a[@]:1:-1}` is a fatal
# "substring expression < 0", and `${a[@]:99:j++}` leaves `j` alone. "Inside"
# differs by shape — an array is inside while some element still has a subscript
# at least `off`, so a three-element array is done at 3 and an empty one at 0,
# while the positionals allow one past the last, ending at count+1 with `$0`
# counted in.

declare -a a=(x y z)
declare -a s=(); s[0]=x; s[5]=y; s[9]=z
declare -a e=()
declare -a one=(solo)

# `j++` runs only if the length is evaluated, so `j` reports whether it was.
arr() { j=0; eval ": \${$1:$2:j++}"; printf '  %-10s %-5s j=%d\n' "$1" "$2" "$j"; }

# A function's positionals are its own, so this installs a list and probes it.
pos() { local o=$1; shift; j=0; eval ": \${@:$o:j++}"; printf '%s:j=%d ' "$o" "$j"; }

echo "### a dense array is done at its element count"
for o in 0 1 2 3 4 9 99; do arr 'a[@]' "$o"; done

echo "### a sparse array is done past its highest subscript (0 5 9)"
for o in 0 1 5 6 9 10 11 99; do arr 's[@]' "$o"; done

echo "### a one-element, an empty and a missing array"
for o in 0 1 2; do arr 'one[@]' "$o"; done
for o in 0 1 2; do arr 'e[@]' "$o"; done
for o in 0 1; do arr 'zz[@]' "$o"; done

echo "### the positionals allow one past the last, counting \$0"
printf '  n=0: '; pos 0; pos 1; pos 2; pos 3; echo
printf '  n=1: '; for o in 0 1 2 3 4; do pos "$o" p; done; echo
printf '  n=2: '; for o in 0 1 2 3 4 5; do pos "$o" p q; done; echo
printf '  n=3: '; for o in 0 1 2 3 4 5 6; do pos "$o" p q r; done; echo

echo "### a negative offset is measured after it becomes a subscript"
printf '  @:   '; for o in "-1" "-2" "-3" "-4" "-9"; do pos " $o" p q; done; echo
for o in "-1" "-3" "-4" "-9"; do arr 'a[@]' " $o"; done
for o in "-1" "-9" "-10" "-11"; do arr 's[@]' " $o"; done

echo "### so a negative length is fatal only from inside"
for o in 0 2 3 4 5; do (eval "echo \${a[@]:$o:-1}"); echo "    a:$o rc=$?"; done
for o in 9 10 11; do (eval "echo \${s[@]:$o:-1}"); echo "    s:$o rc=$?"; done
for o in 0 1; do (eval "echo \${e[@]:$o:-1}"); echo "    e:$o rc=$?"; done
for o in "-1" "-4" "-9"; do (eval "echo \${a[@]: $o:-1}"); echo "    a: $o rc=$?"; done

echo "### the positionals, same question"
neg() { local o=$1; shift; (eval "echo \${@:$o:-1}"); echo "    @:$o rc=$?"; }
for o in 2 3 4 5; do neg "$o" p q; done
for o in "-1" "-3" "-4"; do neg " $o" p q; done

echo "### the star spelling and a quoted slice ask the same question"
j=0; : ${a[*]:99:j++};   echo "  a*:99   j=$j"
j=0; : ${a[*]:1:j++};    echo "  a*:1    j=$j"
j=0; : "${a[@]:99:j++}"; echo "  q a:99  j=$j"
j=0; : "${a[@]:1:j++}";  echo "  q a:1   j=$j"
(echo ${a[*]:99:-1});    echo "  a*:99  rc=$?"
(echo "${a[@]:99:-1}");  echo "  q a:99 rc=$?"

echo "### an in-range offset still evaluates the length exactly once"
j=0; set -- ${a[@]:0:j++}; echo "  a:0 fields=$# j=$j"
j=5; set -- ${a[@]:0:j--}; echo "  a:0 fields=$# j=$j"
