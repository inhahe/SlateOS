# A nameref chain that closes on itself names nothing — unless the variable it
# closed on is a *function-local* one, in which case bash resolves the name once
# more at global scope, with namerefs not followed (variables.c):
#
#     v = find_variable_internal (newname, flags);
#     if (v == orig || v == oldv)
#       {
#         internal_warning (_("%s: circular name reference"), orig->name);
#         /* XXX - provisional change - circular refs go to
#            global scope for resolution, without namerefs. */
#         if (variable_context && v->context)
#           return (find_global_variable_noref (v->name));
#         else
#           return ((SHELL_VAR *)0);
#       }
#
# So a local reference that names itself is an alias for the global of the same
# name, and every use of it warns once per walk.
b() { printf '%-24s ' "$1"; c=$2; ( eval "$c"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
g=GVAL
n=(x y z)
echo "--- a self-naming local, with a global behind it"
f() { local -n g=g
  b 'value' 'printf "<%s>" "$g"'
  b 'length' 'printf "<%s>" "${#g}"'
  b 'default' 'printf "<%s>" "${g:-D}"'
  b 'test -v' '[ -v g ] && printf "<set>" || printf "<unset>"'
  b 'transform @Q' 'printf "<%s>" "${g@Q}"'
  b 'transform @a' 'printf "<%s>" "${g@a}"'
  b 'declare -p' 'declare -p g'
  b 'element 0' 'printf "<%s>" "${g[0]}"'
  b 'arith' 'printf "<%s>" "$(( g + 0 ))"'
  b 'indirect' 'printf "<%s>" "${!g}"'
}
f
echo "--- and an array behind it"
fa() { local -n n=n
  b 'all elements' 'printf "<%s>" "${n[@]}"'
  b 'element 1' 'printf "<%s>" "${n[1]}"'
  b 'keys' 'printf "<%s>" "${!n[@]}"'
  b 'count' 'printf "<%s>" "${#n[@]}"'
}
fa
echo "--- a write through one lands on the global"
fw() { local -n g=g; g=W; declare -p g; printf 'read=<%s>\n' "$g"; }
fw
printf 'after: <%s>\n' "$g"
declare -p g
echo "--- and creates the global when there is none"
fq() { local -n q=q; q=W2; }
fq
declare -p q
echo "--- the global it lands on is not followed, reference or not"
tgt=TGT
declare -n ptr=tgt
fp() { local -n ptr=ptr; b 'read ptr' 'printf "<%s>" "$ptr"'; }
fp
echo "--- with no global of that name there is nothing to find"
fz() { local -n zz=zz
  b 'read' 'printf "<%s>" "$zz"'
  b 'test -v' '[ -v zz ] && printf "<set>" || printf "<unset>"'
}
fz
declare -p zz
echo "--- a mutual cycle between two locals escapes the same way"
a1=GA; a2=GB
fm() { local -n a1=a2; local -n a2=a1
  b 'read a1' 'printf "<%s>" "$a1"'
  b 'read a2' 'printf "<%s>" "$a2"'
}
fm
echo "--- one local link closing onto a global reference"
gx=GX
declare -n gr=gx
fg() { local -n gx=gr; b 'read gx' 'printf "<%s>" "$gx"'; }
fg
echo "--- a cycle that closes on a GLOBAL reaches nothing, function or not"
declare -n c1=c2; declare -n c2=c1
b 'read c1 at top' 'printf "<%s>" "$c1"'
fc() { b 'read c1 in fn' 'printf "<%s>" "$c1"'; }
fc
echo "--- and the same shape declared at top level is refused outright"
# Not through `b`: the escape and the refusal both turn on being in a function,
# and `b` is one.
declare -n s=s
printf 'st=%s ' "$?"
printf '<%s>\n' "$s"
declare -p s
echo TAIL
