# `2>&1` on an *external* command, when the shell's own stdout and stderr do
# not point at the same place. Measured against bash 5.2.
#
# Every probe below is a no-op whenever fd 1 and fd 2 happen to land on one
# terminal — the child's diagnostics come out looking right either way. They
# only bite when the two streams are separable, which is exactly what this
# harness does (it captures stdout and stderr independently), so the probes
# deliberately print *which stream* each line arrived on rather than merging.
#
#   * `cmd 2>&1` must duplicate fd 1's real sink for the child, not hand it a
#     copy of fd 2;
#   * redirects apply left to right, so `cmd 2>&1 >f` sends fd 2 to the
#     *original* stdout and only fd 1 to the file, while `cmd >f 2>&1` sends
#     both to the file;
#   * the sink `2>&1` follows is whatever fd 1 currently is — a pipeline
#     stage's pipe, a persistent `exec > file`, or the process's fd 1;
#     (the `exec > file` half of that is covered by `exec-fd.sh`, not here,
#     because it needs no separable-stream setup to show up);
#   * an enclosing compound command's `2>&1` reaches the external child too.
#
# `$BASH` is used as the stderr-writing child because its spelling differs per
# host, so it is never printed. The two streams are relabelled by running each
# probe in a subshell whose fd 1 and fd 2 are tagged through `sed`.
child() { "$BASH" -c 'echo E >&2; echo O' </dev/null; }

# Run "$1" with fd 1 tagged `out:` and fd 2 tagged `err:`, both landing on this
# script's stdout so the ordering between the two tags is not compared.
tag() { ( eval "$1" ) 2>e.txt | sed 's/^/  out: /'; sed 's/^/  err: /' e.txt; }

echo "=== plain 2>&1"
tag 'child 2>&1'
echo "=== 2>&1 before a stdout redirect"
tag 'child 2>&1 >f.txt'; echo "  file: $(cat f.txt)"
echo "=== a stdout redirect before 2>&1"
tag 'child >f.txt 2>&1'; sed 's/^/  file: /' f.txt
echo "=== 2>&1 in a pipeline stage"
tag 'child 2>&1 | cat'
tag 'true | child 2>&1'
echo "=== 2>&1 under a compound command"
tag '{ child; } 2>&1'
tag 'f() { child; }; f 2>&1'
echo "=== 1>&2 sends stdout to the stderr sink"
tag 'child 1>&2'
echo "=== no redirect at all"
tag 'child'
