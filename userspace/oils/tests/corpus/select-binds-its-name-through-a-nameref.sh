# `select NAME in …` binds the choice with an *ordinary scalar assignment* to
# NAME, which means it goes **through** a nameref rather than over it — exactly
# as a plain `NAME=value` would. `declare -n r=t; select r in …` leaves `r` a
# nameref and gives `t` the choice; a nameref to an array element writes the
# element; a chain is followed to its end; a nameref to a name nothing has bound
# creates it.
#
# This is where `select` parts company with `for`, which deliberately does *not*
# follow a nameref and overwrites the nameref cell itself. The two loops look
# alike but do not agree here.
#
# Every way that assignment can be refused ends the loop with status 1 and
# leaves the target as it was — and each is reported the way the same write is
# reported anywhere else:
#
#   - a circular chain warns `warning: NAME: circular name reference`, blaming
#     the name written rather than any name in the cycle;
#   - a readonly target blames the name *resolved* to, not the one written, so
#     `declare -n n=ro; select n …` says `ro: readonly variable`;
#   - a name the shell maintains itself (`FUNCNAME`) refuses silently — status 1
#     with nothing said at all.
#
# All of them happen only once the choice has been read, so the menu and the
# prompt have already been written either way. The value attributes of whatever
# the write lands on still apply (`-i` evaluates the item as arithmetic, `-u`
# upper-cases it), and `set -a` exports it, because this is the same store as
# any other scalar assignment.

echo "=== a plain nameref is followed, not overwritten"
t1=orig
declare -n r1=t1
select r1 in aa bb; do echo "  body r1=[$r1]"; break; done < <(echo 1)
echo "  rc=$?"
declare -p r1 t1

echo "=== …where a for loop over the same nameref overwrites the cell"
t2=orig
declare -n r2=t2
for r2 in aa bb; do :; done
declare -p r2 t2

echo "=== a nameref to an array element writes the element"
declare -a arr=(p q r)
declare -n r3="arr[1]"
select r3 in aa bb; do echo "  body r3=[$r3]"; break; done < <(echo 2)
echo "  rc=$?"
declare -p r3 arr

echo "=== a nameref to a name nothing has bound creates it"
declare -n r4=never4
select r4 in aa bb; do echo "  body"; break; done < <(echo 1)
echo "  rc=$?"
declare -p r4 never4

echo "=== a two-link chain is followed to its end"
u5=orig
declare -n m5a=u5
declare -n m5b=m5a
select m5b in aa bb; do echo "  body"; break; done < <(echo 2)
echo "  rc=$?"
declare -p m5b m5a u5

echo "=== a circular chain warns, blames the name written, and gives up"
declare -n c6a=c6b
declare -n c6b=c6a
select c6a in aa bb; do echo "  body RAN"; break; done < <(echo 1)
echo "  rc=$?"

echo "=== a readonly target blames the name resolved to"
readonly ro7=frozen
declare -n n7=ro7
select n7 in aa bb; do echo "  body RAN"; break; done < <(echo 1)
echo "  rc=$?"
declare -p n7 ro7

echo "=== …and a readonly written directly blames itself"
readonly ro8=frozen
select ro8 in aa bb; do echo "  body RAN"; break; done < <(echo 1)
echo "  rc=$?"
echo "  ro8=[$ro8]"

echo "=== a name the shell maintains refuses silently"
select FUNCNAME in aa bb; do echo "  body RAN"; break; done < <(echo 1)
echo "  rc=$?"

echo "=== the target's own attributes still apply through the nameref"
declare -i t9
declare -n r9=t9
select r9 in 3*4 zz; do echo "  body"; break; done < <(echo 1)
echo "  rc=$?"
declare -p r9 t9

echo "=== …including one that turns a non-numeric item into zero"
declare -i i10
select i10 in aa bb; do echo "  body i10=[$i10]"; break; done < <(echo 1)
declare -p i10

echo "=== …and the case attributes"
declare -u u11
select u11 in abc def; do echo "  body u11=[$u11]"; break; done < <(echo 2)
declare -p u11

echo "=== set -a exports whatever the write lands on"
set -a
t12=orig
declare -n r12=t12
select r12 in aa bb; do break; done < <(echo 1)
set +a
declare -p t12

echo "=== an out-of-range reply binds the empty string through the nameref"
t13=orig
declare -n r13=t13
select r13 in aa bb; do echo "  body"; break; done < <(echo 9)
echo "  rc=$?"
declare -p r13 t13

echo "=== a blank line reprints the menu without binding anything"
t14=orig
declare -n r14=t14
select r14 in aa bb; do echo "  body r14=[$r14]"; break; done < <(printf '\n2\n')
echo "  rc=$?"
declare -p r14 t14

echo "=== end of input leaves the target untouched"
t15=orig
declare -n r15=t15
select r15 in aa bb; do echo "  body RAN"; break; done < /dev/null
echo "  rc=$?"
declare -p r15 t15

echo "=== a local nameref inside a function is followed too"
f16() {
  local t=inner
  local -n r=t
  select r in aa bb; do break; done < <(echo 2)
  declare -p r t
}
f16

echo "=== a nameref the declaration itself refused is just a plain name"
declare -n r17='not a name'
select r17 in aa bb; do echo "  body"; break; done < <(echo 1)
echo "  rc=$?"
declare -p r17
