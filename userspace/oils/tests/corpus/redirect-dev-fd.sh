# bash's *special redirection filenames*: `/dev/stdin`, `/dev/stdout`,
# `/dev/stderr` and `/dev/fd/N` name a descriptor to duplicate, not a file to
# open (bash manual, "Redirections"). `> /dev/stderr` must reach fd 2, not
# create a file called `/dev/stderr`.
#
# Every probe here keeps the shell's fd 1 on something the shell itself made —
# a command substitution, a pipeline, a file — never the harness's own pipe.
# That is deliberate: this reference bash is an MSYS build whose `/dev/stdout`
# is a real magic path, and opening it fails with ENOENT when fd 1 is an
# inherited non-MSYS pipe (as it is when the harness runs a case). The
# behaviour under test is unaffected — it is fd 1's *sink* that the redirect
# duplicates — but the probe has to be phrased so the reference can answer.
# `/dev/stdin` cannot be phrased that way (fd 0 is always the harness's), so it
# is covered by the in-process tests in `interp.rs` instead.

echo "=== /dev/stdout duplicates fd 1"
x=$(echo hi > /dev/stdout); echo "  x=[$x]"
{ echo hi > /dev/stdout; } | sed 's/^/  piped: /'
echo "  [$(echo one > /dev/stdout; echo two)]"
echo "--- including where fd 1 currently points, not where it started"
( exec > g.txt; echo hi > /dev/stdout ); echo "  g=[$(cat g.txt)]"
echo "--- and it is a dup, so noclobber has no file to protect"
x=$(set -C; echo hi > /dev/stdout; echo "rc=$?"); echo "  x=[$x]"
echo "--- >> is the same dup ( appending to a descriptor is meaningless )"
x=$(echo hi >> /dev/stdout); echo "  x=[$x]"

echo "=== /dev/stdout as a stderr target merges the two streams"
x=$( { echo E >&2; } 2>/dev/stdout ); echo "  x=[$x]"
echo "--- interleaved in write order, not stdout-then-stderr"
x=$( { echo E >&2; echo O; } 2>/dev/stdout ); echo "  x=[$x]"
echo "--- left to right: 2>/dev/stdout then >file leaves fd 2 behind"
x=$( { echo E >&2; echo O; } 2>/dev/stdout >f.txt ); echo "  x=[$x] f=[$(cat f.txt)]"

echo "=== /dev/fd/N duplicates descriptor N"
exec 3>f.txt; echo hi >/dev/fd/3; exec 3>&-; echo "  f=[$(cat f.txt)]"
printf 'from-fd3\n' > f2.txt
exec 3<f2.txt; read l </dev/fd/3; exec 3<&-; echo "  l=[$l]"
echo "--- an unbound descriptor is reported against the path"
x=$( { echo hi >/dev/fd/9; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"

echo "=== a non-numeric or absent fd part is an ordinary filename"
mkdir -p dev/fd
echo hi > dev/fd/x; echo "  dev/fd/x=[$(cat dev/fd/x)]"

rm -rf f.txt f2.txt g.txt dev
