# `$BASH_XTRACEFD` names a *descriptor*, not a sink. bash opens it once —
# `fp = fdopen (fd, "w"); xtrace_set (fd, fp)` in `sv_xtracefd` — and every trace
# line is then an `fprintf (xtrace_fp, …)` in `print_cmd.c`. So when the number
# is `1`, the trace goes to descriptor 1 *as it stands when the line is written*,
# and it follows fd 1 through everything that rebinds it: a command
# substitution's collecting pipe, a pipeline stage's pipe, a group's `> file`, a
# runtime `exec > file`, a close.
#
# That is easy for bash, which forks — the substitution's child simply *has* the
# pipe as its fd 1. osh runs those bodies in-process and carries fd 1 as a value
# threaded through execution, so it has to be told: a trace line is raised from
# deep inside expansion and from the `[[ … ]]` evaluator, neither of which has
# that value in hand. See known-issues
# TD-OILS-XTRACEFD-STDOUT-IS-THE-AMBIENT-ONE.
#
# Two orderings are part of the answer and are pinned here:
#
#   * a *simple command's own* redirect comes after its trace, so
#     `echo x > f` traces to the ambient fd 1 and only then rebinds it — which
#     is why `echo p | cat > f` puts `++ echo p` into `f` (it went down the pipe
#     and `cat` copied it) while `++ cat` does not (it was written before
#     `> f` took effect);
#   * a *later word on fd 1 wins*: an `exec > file` or a group's `> file` inside
#     a capture takes the descriptor away from that capture for good, and the
#     trace goes with it.
#
# The nesting probes are the point of the whole mechanism: an inner
# substitution's trace lands in the inner capture, so it becomes part of the
# *value* the outer assignment is traced with — bash prints
# `+ x='++ echo v'` and then a newline and `v`.
#
# Everything runs inside `( … ) 2>&1 | sed`, so one pipe carries every byte in
# write order and the enclosing fd 1 is itself a pipe rather than the terminal —
# which is what makes probes 4 and 5 mean anything.

# TIMEOUT: 60

p() {
  printf '%-2s %s\n' "$N" "$1"
  rm -f p.txt
  ( eval "$1" ) 2>&1 | sed 's/^/     /'
  if [ -f p.txt ]; then printf '     p.txt=[%s]\n' "$(cat p.txt)"; fi
  rm -f p.txt
  N=$((N + 1))
}
N=1

echo '=== the capture is fd 1, so the capture is where the trace goes'
p 'x=$(BASH_XTRACEFD=1; set -x; echo hi); echo "x=[$x]"'
p 'y=$(BASH_XTRACEFD=1; set -x; [[ a == a ]]); echo "y=[$y]"'
p 'x=$(BASH_XTRACEFD=1; set -x; case a in a) echo K;; esac); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; if [[ -n a ]]; then echo T; fi); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; while [[ -z "$k" ]]; do k=1; done; echo D); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; f(){ echo IN; }; f); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; ( echo S )); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; eval "echo EV"); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; command echo CC); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; PS4="@ "; echo P4); echo "x=[$x]"'

echo '=== …and so an inner trace becomes part of the value an outer one shows'
p 'BASH_XTRACEFD=1; set -x; x=$(echo v); echo "x=$x"'
p 'z=$(BASH_XTRACEFD=1; set -x; w=$(echo inner); echo "w=$w"); echo "z=[$z]"'
p 'BASH_XTRACEFD=1; set -x; x=$( (echo deep) | cat ); echo "x=[$x]"'

echo '=== a pipeline stage traces down its own pipe'
p 'BASH_XTRACEFD=1; set -x; { echo A; } | cat'
p 'BASH_XTRACEFD=1; set -x; echo pipe1 | cat > p.txt'
p 'BASH_XTRACEFD=1; set -x; { { echo N; } | cat; } > p.txt'
p 'x=$(BASH_XTRACEFD=1; set -x; echo A | cat); echo "x=[$x]"'

echo '=== a simple command is traced before its own redirect is applied'
p 'BASH_XTRACEFD=1; set -x; echo redirected > p.txt'
p 'BASH_XTRACEFD=1; set -x; { echo G; } 1>&2'
p 'BASH_XTRACEFD=1; set -x; { echo H; } 3>&1 > p.txt'

echo '=== the later word on fd 1 wins, and takes the trace with it'
p '{ BASH_XTRACEFD=1; set -x; echo body; } > p.txt'
p 'echo one | { BASH_XTRACEFD=1; set -x; read v; } > p.txt'
p 'f() { BASH_XTRACEFD=1; set -x; echo F; }; f > p.txt'
p 'BASH_XTRACEFD=1; set -x; for i in 1; do echo L; done > p.txt'
p '( BASH_XTRACEFD=1; exec > p.txt; set -x; echo E )'
p 'x=$(BASH_XTRACEFD=1; set -x; exec > p.txt; echo E); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; { echo G; } > p.txt); echo "x=[$x]"'
p 'x=$(BASH_XTRACEFD=1; set -x; exec 1>&-; echo Z); echo "x=[$x]"'

echo '=== a compound declaration'"'"'s line is a trace too, so it moves as well'
p 'x=$(BASH_XTRACEFD=1; set -x; declare -a arr=(1 2)); echo "x=[$x]"'
p 'exec 3>p.txt; BASH_XTRACEFD=3; set -x; declare -a arr=(1 2); declare s=1; set +x; exec 3>&-'
p 'exec 3>p.txt; BASH_XTRACEFD=3; set -x; declare -A m=([k]=v); set +x; exec 3>&-'

echo '=== fd 2 and a descriptor of its own are unchanged'
p 'x=$(BASH_XTRACEFD=2; set -x; echo hi) 2>&1; echo "x=[$x]"'
p 'x=$(set -x; echo hi) 2>&1; echo "x=[$x]"'
p 'exec 3>p.txt; BASH_XTRACEFD=3; set -x; x=$(echo v); echo "x=$x"; set +x; exec 3>&-'

echo '=== done'
