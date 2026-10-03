#!/usr/bin/env bash
# Differential test: our `diff` against GNU diffutils.
#
# Since 2026-10-02 the subject is a port of diffutils 3.10's `diff`
# (`userspace/coreutils/src/bin/diff/`): `io.c`, `analyze.c` with gnulib's
# `diffseq.h`, `dir.c` and every output format. The sections below are the
# harness's history -- written when `diff` was still a duplicate pair §1005
# had to decide -- and still explain its fixtures. The random pairs near the
# end (`gen.py`) are what hold the port to GNU's *choice* of edit script,
# where a diff that is merely correct and GNU's part company: see
# `known-issues.md` -> TD-B-DIFF-CHOOSES-ITS-OWN-EDIT-SCRIPT.
#
# ## Why this harness exists
#
# `diff` is one of the sixteen duplicate pairs left under design decision §1005,
# and it is the one that most needs deciding by measurement rather than by
# counting. With `dup-bins-survey.py` reading `coreutils`' shared modules as well
# as its bin -- the fix that moved seventeen other pairs -- `diff` did not move
# at all: `userspace/coreutils/src/bin/diff.rs` is 414 lines and reaches no
# shared module, against the standalone crate's 1541.
#
# The option lists say the same thing more sharply. The `coreutils` half
# implements **three** options: `-q`, `--brief`, `-u`. The standalone mentions
# seventeen, including `-c`, `-r`, `-i`, `-w`, `-b`, `-B`, `-y`, `-N`, `--color`
# and `--width`. So this is the first pair where the standalone is plausibly the
# better program, and §1005 resolves that by PORTING the better half into
# `coreutils` rather than by deleting it -- which makes knowing which half is
# better the whole question.
#
# But "mentions seventeen options" is exactly the inference this tree has now
# been burned by five times. `dd` mentions `bs` and implements none of its
# suffixes. `join` mentions `-a` and rejects `-a1`. An option list is a claim,
# not a measurement, so the standalone gets measured before anything is moved.
#
# ## Why the reference is the installed binary
#
# Unlike every coreutils harness here, this one does not build its reference.
# `DIFF_GNU_SOURCE` fetches a *coreutils* tarball and `diff` is not in it -- it
# is GNU diffutils, a separate project. So the reference is `/usr/bin/diff`.
#
# The §726 caveat therefore applies, and applies less: this host's diffutils is
# `1:3.10-1ubuntu0.1` against its coreutils' `9.4-3ubuntu6.3`. Both are Ubuntu
# revisions and neither is pristine upstream, but one `ubuntu0.1` is a different
# order of divergence from `3ubuntu6.3`, which is what made the coreutils
# harnesses build their own. Recorded rather than waved past: a green run here
# certifies agreement with Ubuntu's diffutils 3.10.
#
# ## Why the fixtures have fixed timestamps
#
# `diff -u` and `diff -c` print each file's mtime in the header:
#
#     --- a.txt   2026-09-11 17:00:00.000000000 +0000
#
# Left alone that makes every run disagree with itself. Every fixture is
# therefore stamped with `touch -d` to one fixed instant, and `TZ` is pinned to
# UTC for both sides, so the header is deterministic and a difference in it is a
# real difference in formatting rather than noise. That is deliberately NOT done
# with `--label`: the standalone may not have it, and a harness that needs an
# option the subject might lack cannot test the subject that lacks it.
#
# ## Why the exit status is compared, not just the output
#
# `diff`'s exit status is part of its contract and is what scripts read: 0 the
# files are the same, 1 they differ, 2 trouble. A `diff` that printed the right
# text and answered 0 for "different" would break every `if diff ...; then` in
# existence while looking perfect in a side-by-side. Several cases here exist
# only to pin the status.
#
# ## Why `od -An -c`
#
# Its output is whitespace: leading `+`/`-`/space columns, tabs inside context
# lines, the exact blank line placement, and the `\ No newline at end of file`
# marker. Any comparison that trimmed or collapsed would agree with an
# implementation that got the column wrong, which is most of what there is to
# get wrong here.
set -u

DIFF_PROG='diff'
# Every invocation below is bounded with it, on both sides. `diff` on two large
# inputs is the classic quadratic-blowup program, so a subject that has stopped
# making progress and one that is merely working hard look identical from
# outside; 30 seconds separates them, since no fixture here is larger than a few
# hundred lines. The reference is wrapped too -- a harness that bounded only our
# side would hang on the day the reference was the buggy one.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Variables `diff` runs with, on both sides, and nothing else does:
# `POSIXLY_CORRECT` changes where option parsing stops, and exported it would
# reach this harness's own tools too. Empty unless a case block sets it.
ENVV=()

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# The one instant every fixture is stamped with. Any fixed value would do; this
# one is old enough never to collide with a file the harness makes later.
STAMP='2020-01-02 03:04:05'

stamp() { touch -d "$STAMP" "$@"; }

# --- fixtures ----------------------------------------------------------------
# The base file everything else is a variation of.
printf 'alpha\nbravo\ncharlie\ndelta\n'          > base.txt
# Identical content in another file: the exit-0, no-output case.
printf 'alpha\nbravo\ncharlie\ndelta\n'          > same.txt
# One line changed in the middle.
printf 'alpha\nbravo\nCHANGED\ndelta\n'          > mid.txt
# Fixtures for `-I`. `iga`/`igb` differ ONLY in a line matching `^#`, so the
# whole difference is ignorable; `igc`/`ige` differ in that line AND another,
# so it is not. `igf`/`igg` differ in case only, which is what the BRE-versus-
# ERE alternation rows compare.
printf 'keep1\n# version 1\nkeep2\n'             > iga.txt
printf 'keep1\n# version 2\nkeep2\n'             > igb.txt
printf 'keep1\n# version 1\nreal\nkeep2\n'       > igc.txt
printf 'keep1\n# version 2\nCHANGED\nkeep2\n'    > ige.txt
printf 'one\ntwo\n'                              > igf.txt
printf 'ONE\ntwo\n'                              > igg.txt
# One line changed at the very top and at the very bottom, which is where an
# off-by-one in the context window shows.
printf 'CHANGED\nbravo\ncharlie\ndelta\n'        > first.txt
printf 'alpha\nbravo\ncharlie\nCHANGED\n'        > last.txt
# A line added, and a line removed, rather than changed.
printf 'alpha\nbravo\nextra\ncharlie\ndelta\n'   > added.txt
printf 'alpha\ncharlie\ndelta\n'                 > removed.txt
# Every line different, so there is no common subsequence to anchor on.
printf 'one\ntwo\nthree\nfour\n'                 > allnew.txt
# Empty, and a single empty line, which are not the same file.
printf ''                                        > empty.txt
printf '\n'                                      > blankline.txt
# No trailing newline: the `\ No newline at end of file` marker, on one side
# and then on both.
printf 'alpha\nbravo\ncharlie\ndelta'            > nonl.txt
printf 'alpha\nbravo\nCHANGED\ndelta'            > nonl2.txt
# Trailing whitespace and interior whitespace, for -b and -w.
printf 'alpha \nbravo\ncharlie\ndelta\n'         > trailws.txt
printf 'al pha\nbravo\ncharlie\ndelta\n'         > interws.txt
printf 'alpha\t\nbravo\ncharlie\ndelta\n'        > tabws.txt
# Case, for -i.
printf 'ALPHA\nbravo\ncharlie\ndelta\n'          > upper.txt
# Blank lines, for -B.
printf 'alpha\n\nbravo\ncharlie\ndelta\n'        > blanks.txt
# A tab inside a line, which the normal format prints raw and which must not be
# expanded.
printf 'alpha\tX\nbravo\ncharlie\ndelta\n'       > tabbed.txt
# For -y: lines longer than a half, tabs at every phase -- one after exactly
# eight columns, where tab packing and column arithmetic disagree most --
# carriage returns, backspaces, form feeds, and characters wider and narrower
# than a byte each.
printf 'short\na line that is much longer than any half of the output is going to be wide\ntab\there\tand\tthere\n12345678\tafter eight\nx\ty\tz\ncaf\303\251 na\303\257ve \346\274\242\345\255\227 wide\nback\bspace\ncarriage\rreturn\nform\ffeed\nsame line\nextra one\n' > sbs1.txt
printf 'short\na line that is much longer than any half of the output is going to be WIDE\ntab\there\tand\tTHERE\n12345678\tafter eight\nx\ty\tz!\ncaf\303\251 na\303\257ve \346\274\242\345\255\227 WIDE\nback\bSPACE\ncarriage\rRETURN\nform\fFEED\nsame line\n' > sbs2.txt
# Bytes that are not text. `diff` calls these binary and says so rather than
# printing them, and the sentence it says is part of the contract.
printf 'alpha\n\x80\xff\nbravo\n'                > bytes.txt
printf 'alpha\n\x80\xfe\nbravo\n'                > bytes2.txt
# A NUL, which is what actually triggers the binary heuristic.
printf 'alpha\n\x00nul\nbravo\n'                 > nul.txt
printf 'alpha\n\x00NUL\nbravo\n'                 > nul2.txt
# Long enough that the default three lines of context do not cover the whole
# file, so the hunk headers have to be right.
{ for i in $(seq 1 40); do printf 'line %02d\n' "$i"; done; }            > long.txt
{ for i in $(seq 1 40); do
    if [ "$i" = 20 ]; then printf 'LINE TWENTY\n'; else printf 'line %02d\n' "$i"; fi
  done; }                                                               > long2.txt
# Two changes far apart, so there are two hunks rather than one.
{ for i in $(seq 1 40); do
    case $i in 5|35) printf 'CHANGED %02d\n' "$i" ;; *) printf 'line %02d\n' "$i" ;; esac
  done; }                                                               > long3.txt
# Directories, for -r and for the "is a directory" diagnostics.
mkdir -p da db
printf 'alpha\n'                                 > da/one.txt
printf 'ALPHA\n'                                 > db/one.txt
printf 'only in a\n'                             > da/onlya.txt
printf 'only in b\n'                             > db/onlyb.txt
mkdir -p da/sub db/sub
printf 'nested\n'                                > da/sub/deep.txt
printf 'NESTED\n'                                > db/sub/deep.txt

# Names holding a byte that is not valid UTF-8, which on this OS is a legal
# filename -- every byte but `/` and NUL is.
#
# These exist because `diff` handled them in the worst possible way until
# 2026-09-14: `list_dir` read each entry with `.to_str()` and SKIPPED the ones
# that did not decode, so `diff -r` compared trees while silently omitting
# files, and reported no difference for a file it had never opened. A
# comparison tool answering "the same" about something it declined to read is
# the one failure it must not have. Separately, `main` used `env::args()`,
# whose iterator unwraps, so naming such a file on the command line killed the
# process before `diff` ran at all.
#
# `\351` is the Latin-1 encoding of `é` and is not valid UTF-8 on its own,
# which is exactly the property being tested.
# Both names go through `$(printf ...)`. Writing `"da/only\351.txt"` directly
# does NOT work and is the trap here: bash does not process `\351` inside
# double quotes, so that creates a file whose name contains a literal
# backslash, three digits and a dot -- valid ASCII throughout, and therefore
# testing nothing. The first version of this block did exactly that, and it
# looked right because both sides agreed about it.
nonutf8=$(printf 'odd\351name.txt')
onlyodd=$(printf 'only\351.txt')
printf 'alpha\n'                                 > "da/$nonutf8"
printf 'ALPHA\n'                                 > "db/$nonutf8"
printf 'lonely\n'                                > "da/$onlyodd"
printf 'top level\n'                             > "$nonutf8"
stamp "da/$nonutf8" "db/$nonutf8" "da/$onlyodd" "$nonutf8"

# For the cases the port of diffutils' diff added (2026-10-02): ed scripts
# with a line that is just a dot, blank lines for -B, tabs for -E and -t,
# trailing blanks for -Z, CRLF for --strip-trailing-cr, function lines for -p
# and -F, NULs for the binary path, a file long enough for context and
# horizon to matter, an exclusion list, and two symbolic links.
printf 'alpha\n.\nbravo\n'                       > dot1.txt
printf 'alpha\n.\nCHANGED\n.\n'                  > dot2.txt
printf 'a\n\nb\n\n\nc\n'                         > blank1.txt
printf 'a\nb\n\nc\n  \n'                         > blank2.txt
printf '\tx\n  y\n\tz\n'                         > tabs1.txt
printf '        x\n\ty\n\tZ\n'                   > tabs2.txt
printf 'x  \ny\t\nz\n'                           > trail1.txt
printf 'x\ny \nZ\n'                              > trail2.txt
printf 'alpha\r\nbravo\r\ncharlie\r\ndelta\r\n'  > crlf.txt
printf 'int main(void)\n{\n  a();\n  b();\n  c();\n  d();\n  e();\n}\n\nstatic int f(void)\n{\n  x();\n  y();\n  z();\n  w();\n}\n' > func1.c
printf 'int main(void)\n{\n  a();\n  B();\n  c();\n  d();\n  e();\n}\n\nstatic int f(void)\n{\n  x();\n  y();\n  Z();\n  w();\n}\n' > func2.c
printf 'bin\000ary one\n'                        > bin1
printf 'bin\000ary two!\n'                       > bin2
{ for i in $(seq 1 60); do printf 'l%02d\n' "$i"; done; }               > lng1.txt
{ for i in $(seq 1 60); do
    case $i in 10|11|30|50) printf 'L%02d\n' "$i" ;; 40) ;; *) printf 'l%02d\n' "$i" ;; esac
  done; printf 'tail\n'; }                                             > lng2.txt
printf '*.txt\nsub\n'                             > excl.lst
ln -s base.txt link1
ln -s mid.txt link2

# Random pairs: `gen.py FIRST LAST` writes `rN.a`, `rN.b` and `rN.opts`.
cat > gen.py <<'PY'
import random, shlex, sys

# Pairs of files for `diff`, from seeds: `gen.py FIRST LAST` writes `rN.a`,
# `rN.b` and `rN.opts` (shell-quoted options) for each seed.
VOCAB = ["a", "b", "c", "d", "", "}", "{", "x = 1;", "return;", "foo", "bar",
         "  indented", "\ttabbed", "trailing  ", "Mixed Case", "#comment",
         "int main(void)", "  ", "\t", "café", ".", "a\tb"]
MODES = [[], ["-u"], ["-c"], ["-U1"], ["-U0"], ["-C2"], ["-e"], ["-f"],
         ["-n"], ["--normal"], ["-y", "-W", "60"], ["-y", "-W", "80"],
         ["-y", "--left-column"], ["-y", "--suppress-common-lines"],
         ["-q"], ["-s"], ["-D", "NAME"], ["--line-format=%L"],
         ["--unchanged-line-format=", "--old-line-format=-%l\n",
          "--new-line-format=+%l\n"],
         ["--old-group-format=<%dn,%dN>\n", "--new-group-format=[%dF]\n",
          "--changed-group-format=%(n=1?one:many)\n",
          "--unchanged-group-format="],
         ["-y", "-t"], ["-u", "-p"], ["-c", "-F", "^[a-z]"]]
EXTRA = [["-b"], ["-w"], ["-i"], ["-B"], ["-E"], ["-Z"], ["-t"], ["-T"],
         ["-d"], ["-H"], ["--horizon-lines=2"], ["-I", "^#"],
         ["--strip-trailing-cr"], ["--suppress-blank-empty"],
         ["--tabsize=4"], ["-a"], ["--label", "L1", "--label", "L2"]]


def lines(rng, vocab):
    n = rng.randint(0, 60)
    return [rng.choice(vocab) for _ in range(n)]


def edit(rng, a, vocab):
    b = list(a)
    for _ in range(rng.randint(0, 10)):
        op = rng.random()
        pos = rng.randint(0, len(b))
        if op < 0.35:
            for _ in range(rng.randint(1, 3)):
                b.insert(pos, rng.choice(vocab))
        elif op < 0.7 and b:
            del b[min(pos, len(b) - 1):min(pos, len(b) - 1) + rng.randint(1, 3)]
        elif b:
            k = min(pos, len(b) - 1)
            b[k] = rng.choice([b[k].upper(), b[k] + " ", " " + b[k],
                               b[k].replace(" ", "\t"), rng.choice(vocab)])
    return b


def text(rng, ls):
    t = "\n".join(ls)
    if ls and rng.random() < 0.85:
        t += "\n"
    if rng.random() < 0.05:
        t = t.replace("\n", "\r\n")
    return t.encode("utf-8")


for seed in range(int(sys.argv[1]), int(sys.argv[2]) + 1):
    rng = random.Random(seed)
    vocab = VOCAB[: rng.randint(3, len(VOCAB))]
    a = lines(rng, vocab)
    if rng.random() < 0.1:
        # Long and repetitive, past discard_confusing_lines' thresholds.
        a = [rng.choice(vocab[:4]) for _ in range(rng.randint(200, 800))]
    b = edit(rng, a, vocab)
    with open(f"r{seed}.a", "wb") as f:
        f.write(text(rng, a))
    with open(f"r{seed}.b", "wb") as f:
        f.write(text(rng, b))
    opts = list(rng.choice(MODES))
    for _ in range(rng.choice([0, 0, 1, 1, 2])):
        opts += rng.choice(EXTRA)
    with open(f"r{seed}.opts", "w", encoding="utf-8") as f:
        f.write(" ".join(shlex.quote(o) for o in opts) + "\n")
PY
python3 gen.py 1 300 || exit 1
stamp func1.c func2.c bin1 bin2 excl.lst r*.a r*.b

# Everything gets the same mtime, including the directories, so no header and no
# `-r` listing can differ for a reason that is not the program's.
stamp ./*.txt da/*.txt db/*.txt da/sub/*.txt db/sub/*.txt da/sub db/sub da db .

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file rather than a pipe: in `x=$(diff | od)` the status
  # recorded is od's, and `diff`'s status is the greater half of its answer.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_side ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_side gnu  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

# One invocation of one side. Each is reached through a symlink named `diff` in
# a directory that is the whole of `PATH` for that invocation, so `argv[0]` is
# the bare word and the `diff: ` prefix on every diagnostic matches. `TZ` is
# pinned so the `-u` and `-c` headers format identically on both sides.
run_side() {
  local side=$1; shift
  diff_run timeout -k 2 30 env ${ENVV[@]+"${ENVV[@]}"} TZ=UTC LC_ALL=C.UTF-8 PATH="$bindir/$side" diff "$@"
}

report() {
  local label="$1"; shift
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case()  { compare - "$@"; report "${ENVV[*]:+${ENVV[*]} }diff $*"; }
run_stdin() { local i="$1"; shift; compare "$i" "$@"; report "printf '$i' | diff $*"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: an xfail that silently becomes correct is a
# stale note in the harness rather than a success.
# `xfail_case` for a case that feeds stdin. Same counting, same XPASS report.
xfail_stdin() {
  local why="$1"; local i="$2"; shift 2
  compare "$i" "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "printf '$i' | diff $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n' "printf '$i' | diff $*" "$why"
  fi
  return 0
}

xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "diff $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n' "diff $*" "$why"
  fi
  return 0
}

# --- the default output format -----------------------------------------------
# Normal format is what `diff` prints with no options at all, and it is the one
# an implementation is most likely to skip in favour of unified.
run_case base.txt same.txt
run_case base.txt mid.txt
run_case base.txt first.txt
run_case base.txt last.txt
run_case base.txt added.txt
run_case base.txt removed.txt
run_case base.txt allnew.txt
run_case base.txt empty.txt
run_case empty.txt base.txt
run_case empty.txt empty.txt
run_case empty.txt blankline.txt
run_case base.txt long.txt

# --- the exit status, which is half the contract -----------------------------
run_case -q base.txt same.txt
run_case -q base.txt mid.txt
run_case --brief base.txt mid.txt
run_case -s base.txt same.txt
run_case --report-identical-files base.txt same.txt
run_case -q -s base.txt same.txt

# --- unified ------------------------------------------------------------------
run_case -u base.txt same.txt
run_case -u base.txt mid.txt
run_case -u base.txt first.txt
run_case -u base.txt last.txt
run_case -u base.txt added.txt
run_case -u base.txt removed.txt
run_case -u base.txt allnew.txt
run_case -u empty.txt base.txt
run_case -u long.txt long2.txt
run_case -u long.txt long3.txt
run_case -u0 long.txt long2.txt
run_case -u1 long.txt long2.txt
run_case -U 5 long.txt long2.txt
run_case -U 0 long.txt long3.txt
run_case --unified long.txt long2.txt
run_case --unified=1 long.txt long2.txt

# --- context ------------------------------------------------------------------
run_case -c base.txt mid.txt
run_case -c base.txt added.txt
run_case -c long.txt long3.txt
run_case -C 1 long.txt long2.txt
run_case --context long.txt long2.txt

# --- the no-newline marker ----------------------------------------------------
# `\ No newline at end of file` is the single most commonly omitted piece of
# diff output, and it changes what `patch` does with the result.
run_case base.txt nonl.txt
run_case nonl.txt base.txt
run_case -u base.txt nonl.txt
run_case -u nonl.txt base.txt
run_case -u nonl.txt nonl2.txt
run_case -c base.txt nonl.txt

# --- whitespace and case ------------------------------------------------------
run_case base.txt trailws.txt
run_case -b base.txt trailws.txt
run_case -w base.txt trailws.txt
run_case -w base.txt interws.txt
run_case -b base.txt interws.txt
run_case -b base.txt tabws.txt
run_case -Z base.txt trailws.txt
run_case --ignore-trailing-space base.txt trailws.txt
run_case --ignore-space-change base.txt interws.txt
run_case --ignore-all-space base.txt interws.txt
run_case base.txt upper.txt
run_case -i base.txt upper.txt
run_case --ignore-case base.txt upper.txt
run_case base.txt blanks.txt
run_case -B base.txt blanks.txt
run_case --ignore-blank-lines base.txt blanks.txt
run_case -u -i base.txt upper.txt
run_case -u -w base.txt interws.txt

# --- tabs, which must survive untouched ---------------------------------------
run_case base.txt tabbed.txt
run_case -u base.txt tabbed.txt
run_case -t base.txt tabbed.txt
run_case -T base.txt tabbed.txt

# --- bytes that are not text --------------------------------------------------
run_case bytes.txt bytes2.txt
run_case -q bytes.txt bytes2.txt
run_case nul.txt nul2.txt
run_case -q nul.txt nul2.txt
run_case -a nul.txt nul2.txt
run_case --text nul.txt nul2.txt
run_case base.txt bytes.txt
run_case -u bytes.txt bytes2.txt

# --- stdin --------------------------------------------------------------------
run_stdin 'alpha\nbravo\ncharlie\ndelta\n' base.txt -
run_stdin 'alpha\nbravo\nCHANGED\ndelta\n' base.txt -
run_stdin 'alpha\nbravo\ncharlie\ndelta\n' - base.txt
# STDIN HAS NO MTIME, so `-u` stamps its `--- -` header with the CURRENT
# time -- measured, that is what GNU does too -- and the two sides of this
# harness run milliseconds apart. The nanosecond field cannot agree, so
# this case can never pass and is not a defect.
#
# WHAT IT STOPS CHECKING, stated so nobody has to work it out later: this
# is the only case exercising UNIFIED output with stdin as an operand, so
# a regression in that combination would hide here. The other four stdin
# cases use the normal format, which prints no header, so the reading of
# stdin is still checked; and every other unified case still checks the
# header. Only the intersection is unwatched.
xfail_stdin 'the --- header stamps stdin with the current time' \
  'alpha\nbravo\nCHANGED\ndelta\n' -u - base.txt
run_stdin '' - empty.txt

# --- directories ---------------------------------------------------------------
run_case da db
run_case -r da db
run_case --recursive da db

# A non-UTF-8 filename named on the command line, and one reached by the
# directory walk. See the fixture block for what these caught.
run_case "$nonutf8" base.txt
run_case base.txt "$nonutf8"
run_case -u "da/$nonutf8" "db/$nonutf8"
run_case -q -r da db
run_case -N da db
run_case --new-file da db
run_case -r -N da db
run_case da/one.txt db
run_case da db/one.txt
run_case -u da/one.txt db/one.txt

# --- side by side ---------------------------------------------------------------
run_case -y base.txt mid.txt
run_case --side-by-side base.txt mid.txt
run_case -y -W 40 base.txt mid.txt
run_case --width=40 -y base.txt mid.txt
run_case -y --suppress-common-lines base.txt mid.txt
# Identical files still print under -y, unless nothing common is to be shown.
run_case -y base.txt same.txt
run_case -y -s base.txt same.txt
run_case -y -q base.txt same.txt
run_case -y --suppress-common-lines base.txt same.txt
run_case -y base.txt added.txt
run_case -y base.txt removed.txt
run_case -y base.txt allnew.txt
run_case -y first.txt last.txt
run_case -y empty.txt base.txt
run_case -y base.txt empty.txt
run_case -y blankline.txt empty.txt
# A missing final newline: `\` or `/` in the gutter, and no newline at the end.
run_case -y nonl.txt base.txt
run_case -y base.txt nonl.txt
run_case -y nonl.txt nonl2.txt
run_case -y nonl.txt nonl.txt
# Each column shows its own file's copy of a line the options call common.
run_case -y -i base.txt upper.txt
run_case -y -b base.txt trailws.txt
run_case -y -w base.txt interws.txt
# Ignored hunks are printed as common lines, paired off in order.
run_case -y -I '^#' iga.txt igb.txt
run_case -y -I '^#' igc.txt ige.txt
run_case -y -B base.txt blanks.txt
run_case -y --left-column base.txt mid.txt
run_case -y --left-column base.txt added.txt
run_case -y --left-column -I '^#' iga.txt igb.txt
# Tabs, widths and the characters print_half_line treats specially.
run_case -y -t base.txt tabbed.txt
run_case -y tabbed.txt base.txt
run_case -y --tabsize=4 tabbed.txt base.txt
run_case -y sbs1.txt sbs2.txt
run_case -y -t sbs1.txt sbs2.txt
run_case -y --tabsize=3 -W 50 sbs1.txt sbs2.txt
run_case -y -W 21 sbs1.txt sbs2.txt
run_case -y -W 33 sbs1.txt sbs2.txt
run_case -y -W 60 -t sbs1.txt sbs2.txt
run_case -y -W 1 base.txt mid.txt
run_case -y -W 2 base.txt mid.txt
run_case -y -W 7 base.txt mid.txt
# -W and --tabsize values, and the two refusals upstream words differently.
run_case -W 0 -y base.txt mid.txt
run_case -W x -y base.txt mid.txt
run_case -W 40 -W 50 -y base.txt mid.txt
run_case -W 40 -W 40 -y base.txt mid.txt
run_case --tabsize=0 base.txt mid.txt
run_case --tabsize=x base.txt mid.txt
run_case --tabsize=4 --tabsize=8 base.txt mid.txt
run_case -t --tabsize=4 base.txt tabbed.txt

# --- refusals and operand errors ------------------------------------------------
run_case
run_case base.txt
run_case nosuch.txt base.txt
run_case base.txt nosuch.txt
run_case nosuch.txt nosuch2.txt
run_case base.txt same.txt extra.txt
run_case -Q base.txt same.txt
run_case --nosuchoption base.txt same.txt
run_case -U notanumber long.txt long2.txt
run_case -U -1 long.txt long2.txt
# `-C` is `-U`'s twin and was missing entirely until 2026-09-16, so its error
# paths had never been compared either. Both spellings of "no argument" are
# here because GNU words them DIFFERENTLY by option length, which is the part
# that was wrong on our side: a SHORT option is
# `option requires an argument -- 'C'` and a LONG one is
# `option '--width' requires an argument`. We had the long form's phrasing on
# both, so every short option that takes a value printed a sentence GNU does
# not.
# --- -I: a change whose lines all match is not a change -------------------------
#
# `ALL` is the word doing the work, and it spans BOTH sides: a hunk holding one
# matching and one non-matching changed line is printed in full. The `ignored`
# and `mixed` rows are that pair, and a suite with only the first would pass on
# an implementation that ignores any hunk containing a match.
#
# The dialect is BRE, measured: `-I 'o\|O'` ignores and `-I 'o|O'` does not.
# Both are rows because an implementation compiling ERE passes the second and
# fails the first, and one compiling BRE does the reverse -- neither row alone
# says which dialect is in use.
run_case -I '^#' iga.txt igb.txt
run_case -I '^#' igc.txt ige.txt
run_case -I 'o\|O' igf.txt igg.txt
run_case -I 'o|O' igf.txt igg.txt
run_case -I '^[oO]NE$' igf.txt igg.txt
run_case -I '^#' -I '^real$' igc.txt ige.txt
# `-I` decides what IS a difference, not what is printed about one, so `-q` and
# the other formats have to honour it too.
run_case -q -I '^#' iga.txt igb.txt
run_case -u -I '^#' iga.txt igb.txt
run_case -e -I '^#' iga.txt igb.txt
run_case -I '[' iga.txt igb.txt
run_case -I

# --- the two remaining classic output formats ----------------------------------
#
# `-e` writes an ed script and `-n` writes an RCS delta. Both were absent until
# 2026-09-16 -- `diff -e` exited 2 with `invalid option`, so a script using it
# stopped rather than getting different output.
#
# The reversal is why `-e` gets more than one row. An ed script is APPLIED in
# order and each command renumbers the lines after it, so the hunks come out
# back to front; a single-hunk case cannot tell a correct implementation from
# one that forgot. `long.txt`/`long3.txt` has several.
run_case -e base.txt mid.txt
run_case -e base.txt added.txt
run_case -e long.txt long3.txt
run_case -e base.txt base.txt
run_case -n base.txt mid.txt
run_case -n base.txt added.txt
run_case -n long.txt long3.txt
run_case -n base.txt base.txt
# A change is a delete AND an append in RCS, and the append is positioned past
# the deleted lines -- `d2 1` then `a2 1`, not `a1 1`.
run_case -n mid.txt base.txt
run_case -C notanumber long.txt long2.txt
run_case -C -1 long.txt long2.txt
run_case -C
run_case -U
run_case -W
run_case --width
run_case . base.txt
run_case base.txt .

# --- the two whose text is ours -------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

# --- the command line, as diffutils 3.10 reads it --------------------------------
# On the shared parser since 2026-09-25; the ladder of exact spellings it
# replaced took the last of two styles, the last of two context lengths, and no
# abbreviation at all.
run_case --unif base.txt mid.txt
run_case --side base.txt mid.txt
run_case -u -c base.txt mid.txt
run_case -y --normal base.txt mid.txt
run_case -U 5 -U 1 base.txt mid.txt
run_case -u -U 1 base.txt mid.txt
run_case -u2 base.txt mid.txt
run_case -1 -2 -u base.txt mid.txt
run_case -U 0 -2 base.txt mid.txt
run_case --context=1 base.txt mid.txt
run_case -U '' base.txt mid.txt
run_case base.txt -u
run_case -- -u base.txt
run_case --no-color base.txt mid.txt
run_case --color=alw base.txt mid.txt
run_case --color=never base.txt mid.txt
run_case --horizon-lines=x base.txt mid.txt
run_case -d base.txt mid.txt
run_case --bogus --help
run_case -D X base.txt mid.txt
run_case -L one -u base.txt mid.txt
# POSIXLY_CORRECT: the first operand ends option parsing, so the `-u` after two
# operands is a third one.
ENVV=(POSIXLY_CORRECT=1)
run_case base.txt mid.txt -u
run_case -u base.txt mid.txt
ENVV=()

# --- the port: formats, filters and walks the hand-written diff lacked ---------
# -D and the format options.
run_case -D X base.txt added.txt
run_case -D X base.txt removed.txt
run_case -D X base.txt same.txt
run_case --line-format='%L' base.txt mid.txt
run_case --old-line-format='-%l
' --new-line-format='+%l
' --unchanged-line-format='=%l
' base.txt mid.txt
run_case --old-group-format='<%dn %df %dl %de %dm>
' --new-group-format='[%dN %dF %dL %dE %dM]
' --changed-group-format='{%(n=1?one:many)|%(N=1?one:many)}
' --unchanged-group-format='' base.txt mid.txt
run_case --line-format='%5dn|%-5dn|%05dn|%xn|%Xn|%on|%c'\''x'\''|%c'\''\101'\''|%%|%L' base.txt mid.txt
run_case --unchanged-group-format='%=' --old-group-format='%<' --new-group-format='%>' base.txt last.txt
run_case --changed-group-format='%(a=b?x:y)' base.txt mid.txt
run_case --line-format='%q' base.txt mid.txt
run_case -D X -D Y base.txt mid.txt
# Labels.
run_case -L one -L two -c base.txt mid.txt
run_case --label=one --label two -u base.txt same.txt
# ed scripts cannot say a file lacks its last newline: the diff, then the
# complaint, and status 2.
run_case -e base.txt nonl.txt
run_case -f base.txt nonl.txt
run_case -n base.txt nonl.txt
run_case -e dot1.txt dot2.txt
# Ignoring: blank lines, tab expansion, trailing space, carriage returns.
run_case -B blank1.txt blank2.txt
run_case -B -u blank1.txt blank2.txt
run_case -E tabs1.txt tabs2.txt
run_case -Z trail1.txt trail2.txt
run_case -E -Z tabs1.txt trail2.txt
run_case --strip-trailing-cr crlf.txt base.txt
run_case -u --strip-trailing-cr crlf.txt base.txt
run_case -u crlf.txt base.txt
# Function lines.
run_case -p func1.c func2.c
run_case -u -p func1.c func2.c
run_case -c -F '^[a-z]' func1.c func2.c
run_case -u -F 'zzz' func1.c func2.c
# Binary.
run_case bin1 bin2
run_case -q bin1 bin2
run_case -a bin1 bin2
run_case -s bin1 bin1
# Context and horizon.
run_case -U 0 base.txt mid.txt
run_case -C 1 lng1.txt lng2.txt
run_case --horizon-lines=0 -u lng1.txt lng2.txt
run_case -d lng1.txt lng2.txt
run_case -H lng1.txt lng2.txt
# Tabs on output.
run_case -t tabs1.txt tabs2.txt
run_case -T base.txt mid.txt
run_case -u -T base.txt mid.txt
run_case --tabsize=4 -t tabs1.txt tabs2.txt
run_case --suppress-blank-empty blank1.txt blank2.txt
run_case -u --suppress-blank-empty blank1.txt blank2.txt
# Colour, forced, and its palette.
run_case --color=always base.txt mid.txt
run_case --color=always -u base.txt mid.txt
run_case --color=always --palette='ad=1;32:de=1;31:hd=4:ln=35' -u base.txt mid.txt
run_case --color=always --palette='zz=1' base.txt mid.txt
run_case --color=always --palette='ad' base.txt mid.txt
run_case --color=bogus base.txt mid.txt
# Directories: exclusions, a starting file, absent files.
run_case -r -x 'b*' da db
run_case -r -x '*.txt' da db
run_case -r -X excl.lst da db
run_case -r -S b.txt da db
run_case -r -N da db
run_case -r -P da db
run_case -N base.txt nonexistent.txt
run_case -P nonexistent.txt base.txt
run_case -P base.txt nonexistent.txt
run_case --from-file=base.txt mid.txt same.txt
run_case --to-file=base.txt mid.txt same.txt
run_case --from-file=base.txt --to-file=mid.txt
run_case da base.txt
run_case base.txt da
run_case - da
run_case -D X da db
run_case --ignore-file-name-case -r da db
run_case --no-dereference link1 link2
run_case --no-dereference link1 base.txt
run_case -s base.txt same.txt
run_case -s base.txt base.txt
run_case -q base.txt mid.txt

# --- random pairs, random options ----------------------------------------------
# `gen.py` writes seeded pairs of files and options for them: small vocabularies
# so lines repeat, edits that insert, delete and change runs of lines, some long
# repetitive files past `discard_confusing_lines`' thresholds, CRLF and missing
# final newlines, and every output format and filter. Each pair is one case, so
# a difference names its seed.
for i in $(seq 1 300); do
  eval "run_case $(cat "r$i.opts") r$i.a r$i.b"
done

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
