#!/usr/bin/env bash
#
# Regenerate userspace/coreutils/tests/data/human-gnu.txt — the table of
# renderings that real GNU coreutils produces for a spread of byte counts,
# against which `coreutils::human` is checked.
#
# WHY A COMMITTED FIXTURE RATHER THAN A LIVE DIFF
#
# The other coreutils harnesses (scripts/test-diff.sh and friends) run our
# binary and GNU's side by side on every invocation. `human` is a library
# module with no binary of its own, and — more to the point — the GNU side of
# this particular comparison is expensive to produce: the only instrument that
# renders an arbitrary number is a utility that has already *counted* that many
# bytes or is looking at a file that long. Measuring is therefore a one-off,
# and the measurements are committed so the test is reproducible on a machine
# with no GNU coreutils at all.
#
# Re-run this only when you want to widen the sweep or re-measure against a
# different GNU version; the fixture records which version produced it.
#
# THE THREE INSTRUMENTS, AND WHY IT TAKES THREE
#
#   ls -l    -> human_ceiling, autoscale, both bases.
#   du -B N  -> human_ceiling, no autoscale, exercising the block-size
#               divisor branch (human.c:210-219) that the ls sweep never
#               reaches, because ls always passes from=to=1.
#   dd       -> human_round_to_nearest. Nothing else in coreutils uses that
#               style, so nothing else can measure it: `ls -h`, `du -h` and
#               `df -h` all round *up* so that a file or a filesystem never
#               looks smaller than it is. Checked here on 8.32: a 999499-byte
#               file is `1.0M` to ls and du but `999 kB` to dd.
#
# THE MAGNITUDE CEILING
#
# The ls/du sweeps stand files of the required length, which is free because
# GNU truncate(1) marks them sparse -- a 5 TB file costs no disk at all. (Note
# that Python's file.truncate() on Windows does not: it reserves, and fills the
# volume. Use truncate(1).) NTFS refuses lengths past ~16 TB, so the sweep
# stops at 5e12 and reaches exponent 4 in both bases -- four trips through the
# residue-propagation loop, which is the depth at which that loop's carry
# (`r2 + rounding`, `rounding >> 1`) is actually interesting. Exponents 5..10
# (P E Z Y R Q) are unreachable by measurement and are covered by the unit
# tests in src/human.rs against the letter table alone.
#
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/../userspace/coreutils/tests/data/human-gnu.txt"
work="${TMPDIR:-/tmp}/human-fixture.$$"

for tool in ls du dd truncate; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "gen-human-fixture: need GNU $tool" >&2
        exit 1
    fi
done

# Refuse to run anywhere the sparse-file trick is not actually sparse: if it
# is not, this script fills the volume and then reports nonsense. Stand up one
# large file and insist the free space did not move.
mkdir -p "$work" || exit 1
# The destination directory has to exist before the first redirect into
# "$out.tmp", not after the last one: a `>` into a missing directory fails per
# command, and the failures are easy to walk straight past.
mkdir -p "$(dirname "$out")" || exit 1
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

free_kb() { df -k "$work" | awk 'NR==2 {print $4}'; }
before="$(free_kb)"
if ! truncate -s 5000000000000 "$work/probe" 2>/dev/null; then
    echo "gen-human-fixture: cannot create a 5 TB file here" >&2
    exit 1
fi
after="$(free_kb)"
rm -f "$work/probe"
if [ "$((before - after))" -gt 1048576 ]; then
    echo "gen-human-fixture: truncate is not sparse on this volume" >&2
    echo "  (a 5 TB file consumed $((before - after)) KiB; refusing to fill the disk)" >&2
    exit 1
fi

echo "gen-human-fixture: generating sizes..." >&2
python - "$work/sizes.txt" <<'PY'
import random, sys
random.seed(20260819)
LIMIT = 5 * 10**12
s = set()
for base in (1000, 1024):
    e = 1
    while True:
        e *= base
        if e > LIMIT:
            break
        # The exponent boundary itself, and one byte either side of it: this is
        # where a carry has to travel out of the mantissa into the exponent.
        for d in (-2, -1, 0, 1, 2):
            if 0 < e + d <= LIMIT:
                s.add(e + d)
        # The decimal-disappears-at-ten boundary, and the top of each scale.
        for m in (5, 9, 10, 11, 99, 100, 999, 1000, 1023, 1024):
            if 0 < e * m <= LIMIT:
                s.add(e * m)
        # Exact halves, where ceiling and round-to-nearest must disagree and
        # round-half-to-even's parity rule is the only thing deciding.
        for m in range(0, 24):
            v = e * m + e // 2
            if 0 < v <= LIMIT:
                s.add(v)
                s.add(v - 1)
                s.add(v + 1)
        # The carry boundary *under round-to-nearest*, which is not the
        # exponent boundary and is the single most informative point in the
        # whole sweep. At exponent e a value prints as round(v/e); that reaches
        # `base` -- and so has to carry into the next letter -- half a unit
        # early, at e*base - e/2. For e=1000 that is 999500, exactly the point
        # the human.c:301-vs-327 threshold confusion moves. Ceiling carries
        # somewhere else again, so both instruments learn something here.
        nxt = e * base
        for d in (-2, -1, 0, 1, 2):
            v = nxt - e // 2 + d
            if 0 < v <= LIMIT:
                s.add(v)
        # Ties in the *tenths*, for values that still show a decimal: the
        # rendering is round(v/e*10)/10, so the tie sits at e*(m + 0.05).
        # These exercise round-half-to-even's parity rule on the digit that is
        # actually printed, which the whole-unit halves above never reach.
        for m in range(1, 10):
            v = e * m + e // 20
            if 0 < v <= LIMIT:
                s.add(v)
                s.add(v - 1)
                s.add(v + 1)
for i in range(1, 1200):
    s.add(i)
for _ in range(1200):
    s.add(random.randrange(1, LIMIT))
for _ in range(600):
    s.add(random.randrange(1, 10**7))
# newline='' so a Windows Python does not write CRLF into a file that a shell
# `read` loop then hands to truncate(1) with the CR still attached.
with open(sys.argv[1], 'w', newline='') as fh:
    fh.write('\n'.join(str(x) for x in sorted(s)) + '\n')
print("  %d sizes" % len(s), file=sys.stderr)
PY
[ -s "$work/sizes.txt" ] || { echo "gen-human-fixture: no sizes" >&2; exit 1; }

# ---------------------------------------------------------------- ceiling ---
# One sparse file per size, named for the size so ls's own output carries the
# input alongside the rendering.
echo "gen-human-fixture: standing up sparse files..." >&2
mkdir -p "$work/f"
# Perl rather than a `while read; do truncate; done` loop: the loop is one
# process spawn per size, and on Windows that is ~30 ms, so 3500 sizes spend
# ~2 minutes doing nothing but fork. Perl's truncate() is the same sparse
# ftruncate underneath (verified: a 5 TB file through it costs zero bytes),
# and one interpreter does the whole set in under a second.
perl -e '
    my ($list, $dir) = @ARGV;
    open(my $in, "<", $list) or die "open $list: $!";
    while (my $n = <$in>) {
        chomp $n;
        $n =~ s/\r$//;
        next unless $n =~ /^[0-9]+$/;
        my $p = "$dir/n$n";
        open(my $fh, ">", $p) or die "create $p: $!";
        truncate($fh, $n + 0) or die "truncate $p to $n: $!";
        close $fh;
    }
' "$work/sizes.txt" "$work/f" || exit 1

version="$(ls --version | head -1)"

echo "gen-human-fixture: measuring ls..." >&2
{
    echo "# GNU renderings of human_readable, measured -- do not hand-edit."
    echo "# Regenerate with scripts/gen-human-fixture.sh"
    echo "# Produced by: $version"
    echo "#"
    echo "# <style> <base> <n> <expected>"
    echo "#"
    echo "# style: ceil    = AUTOSCALE|CEILING|SI                 (ls -l -h / --si)"
    echo "# style: ceilblk = CEILING, no autoscale, to_block_size (du --apparent-size -B N)"
    echo "# style: near    = AUTOSCALE|ROUND_TO_NEAREST|SI|B|SPACE (dd)"
    echo "# base:  1000 or 1024; for ceilblk, base is the block size."
    ls --si -l "$work/f" | awk 'NF>=9 && $9 ~ /^n[0-9]+$/ {
        n = substr($9, 2); print "ceil 1000 " n " " $5 }'
    ls -h -l "$work/f" | awk 'NF>=9 && $9 ~ /^n[0-9]+$/ {
        n = substr($9, 2); print "ceil 1024 " n " " $5 }'
} > "$out.tmp" || exit 1

# --------------------------------------------------- ceiling, block divisor ---
# du's --block-size drives human_readable's to_block_size with autoscale off,
# which is the branch that turns `n` into a count of some other unit. Several
# divisors, including ones that are not powers of the base, so the `r10`/`r2`
# residue arithmetic is exercised rather than a clean shift.
echo "gen-human-fixture: measuring du -B..." >&2
for bs in 512 1024 1000 4096 3 7 1000000; do
    # -a is essential, not cosmetic: without it du reports directory totals
    # only, so a whole directory of files yields exactly one line. The first
    # version of this script omitted it and silently produced zero du rows --
    # which is why the row-count floor at the end of this script exists.
    du -a --apparent-size -B "$bs" "$work/f" 2>/dev/null \
        | awk -v bs="$bs" '$2 ~ /n[0-9]+$/ {
            n = $2; sub(/^.*\/n/, "", n)
            if (n ~ /^[0-9]+$/) print "ceilblk " bs " " n " " $1 }'
done >> "$out.tmp" || exit 1

# ------------------------------------------------------- round to nearest ---
# dd is the only round-to-nearest instrument, and it has to move every byte it
# reports, so this sweep is confined to sizes worth actually copying. It still
# reaches exponent 2 in both bases, which covers the loop carry and both sides
# of the 999500 boundary.
dd_total="$(awk '$1 <= 20000000' "$work/sizes.txt" | wc -l)"
echo "gen-human-fixture: measuring dd on $dd_total sizes (this one moves bytes)..." >&2
dd_i=0
awk '$1 <= 20000000' "$work/sizes.txt" | while read -r n; do
    dd_i=$((dd_i + 1))
    if [ $((dd_i % 250)) -eq 0 ]; then
        echo "  dd $dd_i/$dd_total" >&2
    fi
    # dd emits three different shapes, and only one of them has both forms.
    # It suppresses a human form that would merely repeat the byte count:
    #
    #   n <  1000          "5 bytes copied"                    -- neither form
    #   1000 <= n < 1024   "1012 bytes (1.0 kB) copied"        -- SI only
    #   n >= 1024          "1024 bytes (1.0 kB, 1.0 KiB) copied"
    #
    # The middle shape is the trap: `${line#*, }` on a string with no comma
    # returns the string unchanged, so a naive split files the *SI* rendering
    # under base 1024 and the fixture then demands `1.0 kB` where the correct
    # IEC answer is `1012 B`. That produced 24 spurious failures on the first
    # run. Rows are emitted only for forms dd actually printed -- the suppressed
    # one could be reconstructed (it is by definition the byte count), but
    # inferring an expectation instead of measuring it is precisely the habit
    # this whole fixture exists to avoid.
    line="$(dd if=/dev/zero of=/dev/null bs="$n" count=1 2>&1 | sed -n 's/.*bytes (\(.*\)) copied.*/\1/p')"
    [ -n "$line" ] || continue
    case "$line" in
        *,*)
            echo "near 1000 $n ${line%%,*}"
            echo "near 1024 $n ${line#*, }"
            ;;
        *)
            echo "near 1000 $n $line"
            ;;
    esac
done >> "$out.tmp" || exit 1

# Refuse to install a fixture that is obviously short: every failure mode above
# this point (a redirect that could not open, an awk that matched nothing, a du
# that rejected a block size) produces a small-but-valid file, and a small
# fixture is a test that passes by measuring almost nothing.
rows="$(grep -vc '^#' "$out.tmp")"
if [ "$rows" -lt 20000 ]; then
    echo "gen-human-fixture: only $rows rows -- something above failed; not installing" >&2
    echo "  left at $out.tmp for inspection" >&2
    exit 1
fi
mv "$out.tmp" "$out" || exit 1
echo "gen-human-fixture: wrote $out ($rows rows)" >&2
