# Arithmetic is where bash's circular-nameref warning stops being one line and
# becomes a count. The rule is not "once per statement" but **once per walk of
# the chain**, and each arithmetic shape walks it a fixed number of times:
#
#   a scalar read   walks once     `(( c1 ))`
#   an element read walks twice    once to find the array, once to fetch
#   a scalar write  walks twice    once to find the variable, once to bind
#   an element write walks three   the two above, plus the bind
#
# So the counts add: `(( c1[0] += 5 ))` is an element read plus an element
# write, and warns five times.
#
# The two writes differ in more than a line. A **scalar** write stores nothing
# and leaves the name a reference (`declare -n c1="c2"` survives it). An
# **element** write lands on the name the walk started from, which stops being
# a reference because bash has no array namerefs — so the cycle is broken and
# the other end can read what was written.
#
# Every case is a subshell so each starts from the same untouched cycle.

cyc='declare -n c1=c2; declare -n c2=c1;'

echo '=== a scalar read walks once'
( eval "$cyc"; echo "[$(( c1 ))] rc=$?" )
( eval "$cyc"; echo "[$(( -c1 ))] rc=$?" )
( eval "$cyc"; (( c1 )); echo "rc=$?" )
( eval "$cyc"; let "c1"; echo "rc=$?" )

echo '=== an element read walks twice, and the counts add'
( eval "$cyc"; echo "[$(( c1[0] ))] rc=$?" )
( eval "$cyc"; echo "[$(( c1[0] + c1[1] ))] rc=$?" )

echo '=== a scalar write walks twice, stores nothing, stays a reference'
( eval "$cyc"; (( c1 = 5 )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; let "c1 = 5"; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( c1 += 5 )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( c1++ )); echo "rc=$?"; declare -p c1 )

echo '=== an element write walks three times, and lands'
( eval "$cyc"; (( c1[0] = 5 )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; let "c1[0] = 5"; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( c1[0] += 5 )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( c1[0]++ )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( ++c1[0] )); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; (( c1[2] = 7 )); echo "rc=$?"; declare -p c1 )

echo '=== the element write breaks the cycle, so the other end reads it'
( eval "$cyc"; (( c1[0] = 5 )); echo "[${c2[0]}] rc=$?" )
( eval "$cyc"; (( c1[0] = 5 )); (( c1[1] = 6 )); declare -p c1 )

echo '=== a longer cycle is blamed on the name that was written'
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1
  echo "[$(( a1[0] ))] rc=$?" )
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1
  (( a1[0] = 9 )); echo "rc=$?"; declare -p a1 a2 a3 )

echo '=== a chain that resolves is untouched by any of this'
( t=3; declare -n r=t; echo "[$(( r ))] rc=$?" )
( t=3; declare -n r=t; (( r += 4 )); echo "rc=$?"; declare -p r t )
( declare -a u=(1 2); declare -n r=u; (( r[1] += 4 )); echo "rc=$?"; declare -p r u )

echo still here
