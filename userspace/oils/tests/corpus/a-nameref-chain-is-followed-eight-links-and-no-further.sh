# bash's nameref walk is bounded, and its cycle test is narrower than "a name
# came round again" (`find_variable_nameref`, variables.c:2074):
#
#	  level = 0;
#	  orig = v;
#	  while (v && nameref_p (v))
#	    {
#	      level++;
#	      if (level > NAMEREF_MAX)
#		return ((SHELL_VAR *)0);	/* error message here? */
#	      …
#	      oldv = v;
#	      v = find_variable_internal (newname, flags);
#	      if (v == orig || v == oldv)
#		{
#		  internal_warning (_("%s: circular name reference"), orig->name);
#		  …
#		}
#	    }
#
# `NAMEREF_MAX` is 8 (variables.h:172). So a chain of more than eight links
# resolves to nothing with no diagnostic at all — it is not a cycle, merely too
# long — and a cycle that closes anywhere other than on the variable the walk
# started from (or the one it has just left) is never recognised as one: bash
# spins to the cap and gives up quietly.
exec 2>&1
echo "--- how far the walk follows"
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do eval "declare -n n$i=n$((i+1))"; done
n13=DEEP
for s in 1 3 4 5 6 12; do eval "printf '%-4s [%s]\n' n$s \"\${n$s}\""; done
echo "--- the cap counts links followed, not names seen"
declare -n m1=m2 m2=m3 m3=m4 m4=m5 m5=m6 m6=m7 m7=m8 m8=m9
m9=NINE; printf 'm1 [%s]\n' "${m1}"
declare -n k1=k2 k2=k3 k3=k4 k4=k5 k5=k6 k6=k7 k7=k8 k8=k9 k9=k10
k10=TEN; printf 'k1 [%s]\n' "${k1}"
# `find_variable_last_nameref` (variables.c:2116) — the walk `declare +n` uses to
# find which link in the chain the attribute comes off — counts its links the
# same way and gives up just as quietly.
echo "--- the walk to the last reference gives up at the same depth"
declare +n k1; echo "st=$?"; declare -p k8 k9
declare +n m1; echo "st=$?"; declare -p m7 m8
echo "--- a store through a chain it gave up on takes the command list with it"
n4=X; echo "st=$? (never runs)"
declare -p n13
n5=Y; echo "st=$?"
declare -p n13
echo "--- and so does one through a cycle, which says so first"
declare -n b1=b2; declare -n b2=b1
printf 'read [%s]\n' "${b1}"
b1=X; echo "st=$? (never runs)"
declare -p b1 b2
echo "--- a cycle closing further along the chain is not one bash notices"
declare -n a1=a2; declare -n a2=a3; declare -n a3=a2
printf 'read [%s]\n' "${a1}"
a1=X; echo "st=$? (never runs)"
declare -p a1 a2 a3
unset a1; echo "st=$?"; declare -p a1 a2 a3
echo "--- a reference naming itself is refused at declaration time instead"
declare -n c1=c1; echo "st=$?"; declare -p c1
echo "--- and the escape to global scope is keyed to the same test"
declare -a g=(G)
f1() { local -n g=g; printf 'self  [%s]\n' "${g}"; }
f1
declare -a h3=(H)
f2() { local -n h1=h2; local -n h2=h3; local -n h3=h2; printf 'far   [%s]\n' "${h1}"; }
f2
echo TAIL
