# `jobs` takes two independent settings, not a set of flags. `-l`, `-p` and `-n`
# choose the one *form* the listing takes, so the last of them given decides;
# `-r` and `-s` choose which jobs are considered at all. A pid is never printed
# here — the two shells' pids differ — so the form is read off the shape of the
# line instead: how many fields it has, and whether the second one is a number.

shape() {
  read -r one two rest < "$1"
  if [ -z "$one" ]; then
    echo "nothing"
  elif [ -z "$two" ]; then
    echo "pid alone"
  else
    case "$two" in
      [0-9]*) echo "long" ;;
      *) echo "standard" ;;
    esac
  fi
}

echo "=== each form on its own"
sleep 0.3 & jobs > o; shape o; wait
sleep 0.3 & jobs -l > o; shape o; wait
sleep 0.3 & jobs -p > o; shape o; wait
sleep 0.3 & jobs -n > o; shape o; wait

echo "=== …and the last of them wins when more than one is given"
sleep 0.3 & jobs -lp > o; shape o; wait
sleep 0.3 & jobs -pl > o; shape o; wait
sleep 0.3 & jobs -nl > o; shape o; wait
sleep 0.3 & jobs -ln > o; shape o; wait

echo "=== -n prints only what has not been reported yet"
sleep 0.05 & sleep 0.3; jobs -n; echo "--"; jobs -n; echo "nothing the second time"; disown -a
# A bare listing reports the job, which leaves `-n` nothing to say.
sleep 0.05 & sleep 0.3; jobs; jobs -n; echo "nothing left to report"; disown -a
# It is not a filter on *finished* jobs: a job nobody has looked at yet is
# unreported whether it is still running or not. The second job has to still be
# *running* when the two listings look, and it is the wall clock that decides —
# so it sleeps far longer than the wait rather than a hair longer. At
# `sleep 0.5` the margin was 0.2 s, and a loaded machine spent it: bash reached
# the listing after the job had finished and printed `Done` where osh printed
# `Running`.
#
# The *first* job's length is a deadline of its own, and in the other
# direction: it has to still be running when the second job is forked, because
# that is what puts the `-` on it (a shell that finds it already dead points
# `-` at the new job instead, and prints no marker on this one). At
# `sleep 0.05` the margin was the 50 ms between the two forks, and a loaded
# machine spent that too — bash printed `[1] ` where osh printed `[1]-`. So it
# sleeps long enough to outlive the fork by a wide margin and still be `Done`
# well before the listing.
sleep 0.4 & sleep 3 & sleep 1.2; jobs -n; echo "--"; jobs; kill %2; wait 2>/dev/null
# Reporting is what `-n` looks at, so naming the job in `kill` — which puts its
# fate back on the books — brings it back.
sleep 0.05 & sleep 0.3; jobs -n; kill -0 %1; jobs -n; echo "back on the books"; disown -a
# `-n` reports the job it prints, so the row lingers exactly as a bare listing's
# does: still nameable until the next sweep.
sleep 0.05 & sleep 0.3; jobs -n; kill %1; echo "rc=$?"; disown -a

echo "=== -p says where a job is, which does not amount to reporting it"
sleep 0.05 & sleep 0.3; jobs -p > o; jobs; echo "still to report"; disown -a
sleep 0.05 & sleep 0.3; jobs -p %1 > o; jobs; echo "still to report"; disown -a

echo "=== -r and -s narrow a listing that had to choose, not one that was told"
# The listing picks for itself, so a finished job is filtered out…
sleep 0.05 & sleep 0.3; jobs -r; echo "nothing running"; disown -a
sleep 0.05 & sleep 0.3; jobs -s; echo "nothing stopped"; disown -a
# …but an operand names its job outright, and neither has any say over it.
sleep 0.05 & sleep 0.3; jobs -r %1; disown -a
sleep 0.05 & sleep 0.3; jobs -s %1; disown -a
# They are independent of the form, so `-n` still applies alongside them.
sleep 0.05 & sleep 0.3; jobs -rn; echo "--"; jobs; disown -a
sleep 0.05 & sleep 0.3; jobs -nr; echo "--"; jobs; disown -a
