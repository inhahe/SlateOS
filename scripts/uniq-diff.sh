#!/usr/bin/env bash
# Differential test: our uniq against GNU uniq.
#
# ## The second operand is an output file, and this harness must respect that
#
# `uniq INPUT OUTPUT` opens OUTPUT for *writing* and truncates it. Every other
# harness in this directory passes a list of fixtures as operands; doing that
# here destroys the fixture, and the destruction is silent — it took one such
# accident during the measurement of this utility to notice. So `run_case` only
# ever passes one operand, and the two-operand cases go through `run_outfile`,
# which gives each side its own scratch name and compares the two *files*
# afterwards rather than stdout.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `uniq` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh`, `wc-diff.sh` and `cut-diff.sh`.
#
# stdout is compared byte for byte with `od -An -c`, which is not optional here:
# `--group` and `--all-repeated` differ from each other only in where blank
# lines fall, `-c` pads its counts to a fixed width, `-z` emits NULs, and an
# unterminated final line comes back terminated. A whitespace-trimming
# comparison would agree with everything.
#
# The locale is `C.UTF-8`, with the two `argmatch` cases referenced under
# `LC_ALL=C` so the quote marks are ASCII on both sides. See
# `open-questions.md` → B-Q2.
set -u

# Our uniq is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/uniq.exe"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# Environment variables to set on *both* sides. `uniq` is the only utility here
# whose parsing depends on the environment, and both variables it reads change
# what an operand means rather than merely how it is formatted, so they have to
# be part of the comparison rather than fixed once at the top.
ENVV=()

run_ours() { env "${ENVV[@]}" "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; wsl -e env "LC_ALL=$loc" "${ENVV[@]}" uniq "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands
# on the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'probe\nprobe\n' > .probe
if [ "$(run_gnu C.UTF-8 .probe 2>/dev/null)" = "probe" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "uniq-diff: glibc uniq not reachable in this directory; skipping"
fi
rm -f .probe

# --- fixtures ----------------------------------------------------------------
# Runs of every shape: a pair, a singleton, a triple, and a trailing singleton.
printf 'a\na\nb\nc\nc\nc\nd\n'          > runs.txt
printf 'a\na\na\n'                      > allsame.txt
printf 'a\nb\nc\n'                      > alldiff.txt
printf 'a\nb\na\n'                      > nonadjacent.txt
printf 'x\n'                            > one.txt
printf ''                               > empty.txt
printf '\n\n\n'                         > blanks.txt
printf 'a\na'                           > unterminated.txt
printf 'lonely'                         > unterm-one.txt
# Fields: a leading blank, tabs, and a common tail so -f can merge them.
printf ' x tail\n y tail\n z other\n'   > fields.txt
printf 'a\tp\tz\nb\tp\tz\n'             > tabfields.txt
printf '  lead a\n  lead a\n'           > leading.txt
# Case, so -i has something to fold, and non-ASCII so it has something to leave.
printf 'Ab\naB\nAB\n'                   > mixedcase.txt
printf '\xc3\xa9\n\xc3\x89\n'           > utf8case.txt
# Prefixes that agree for a while, for -w and -s.
printf 'abcd\nabxy\nabcd\n'             > prefixes.txt
printf 'a\nab\nabc\n'                   > growing.txt
# Bytes that are not text at all.
printf 'a\xff\na\xff\na\xfe\n'          > badbytes.txt
printf 'a\r\na\n'                       > crlf.txt
# NUL-delimited records, one of them containing a newline.
printf 'a\0a\0b\0'                      > nul.txt
printf 'p\nq\0p\nr\0'                   > nul-fields.txt
printf 'a\0b'                           > nul-unterminated.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(uniq | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_gnu "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_gnu "$loc" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of
  # the getopt module is that the sentences match, so a harness that only asked
  # "did it complain?" would pass on every wording this exists to fix.
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err"
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "uniq $*"; }
# The diagnostics that pass an argument through gnulib's `quote()` — the
# `argmatch` ones and `extra operand` — referenced under `LC_ALL=C` so that the
# quote marks are ASCII on both sides. Under a UTF-8 locale gnulib switches to
# U+2018/U+2019 and ours does not — one open question, not one per case. See
# B-Q2. `uniq`'s three *number* diagnostics quote nothing at all, so they stay
# in `run_case`, and that is itself worth certifying.
run_ascii() { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "uniq $* [C]"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C.UTF-8 "$@"
  report "printf '$input' | uniq $*"
}

# A case with an explicit OUTPUT operand. Each side writes to its own name and
# the two files are compared afterwards; stdout is expected to be empty on both
# and is compared as well, since a bug that wrote to both would otherwise pass.
run_outfile() {
  [ "$HAVE_GNU" = yes ] || return 0
  local o_err g_err o_rc g_rc o_out g_out
  o_err=$(mktemp); g_err=$(mktemp)
  rm -f ours.out gnu.out
  run_ours "$@" ours.out </dev/null >/dev/null 2>"$o_err"; o_rc=$?
  run_gnu C.UTF-8 "$@" gnu.out </dev/null >/dev/null 2>"$g_err"; g_rc=$?
  # A missing file is a distinct state from an empty one: `uniq` must not create
  # the output when the input could not be opened.
  o_out=$([ -e ours.out ] && od -An -c < ours.out || echo '<no file>')
  g_out=$([ -e gnu.out ] && od -An -c < gnu.out || echo '<no file>')
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err" ours.out gnu.out
  report "uniq $* OUT"
}

xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  compare - C.UTF-8 "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL uniq %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS uniq %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# Two of our own invocations compared against each other. The reference cannot
# arbitrate an abbreviation whose long form is *meant* to differ from GNU's, but
# the abbreviation must still resolve to the same option — which is the whole
# point of the getopt module — so that much is checked here.
selfsame() {
  local a="$1" b="$2" x y xr yr
  # shellcheck disable=SC2086  # both are single options by construction
  x=$("$OURS_ABS" $a </dev/null 2>&1); xr=$?
  y=$("$OURS_ABS" $b </dev/null 2>&1); yr=$?
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   uniq %s == uniq %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF uniq %s != uniq %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- merging, with and without a mode ------------------------------------------
run_case runs.txt
run_case allsame.txt
run_case alldiff.txt
run_case nonadjacent.txt
run_case one.txt
run_case empty.txt
run_case blanks.txt
run_case unterminated.txt
run_case unterm-one.txt
run_stdin '' ""
run_stdin 'a\na\n'
run_stdin 'a\na'
run_stdin 'a'
run_stdin '\n'

# --- counting ------------------------------------------------------------------
run_case -c runs.txt
run_case -c allsame.txt
run_case -c alldiff.txt
run_case -c empty.txt
run_case -c blanks.txt
run_case -c unterminated.txt
run_case -c unterm-one.txt
run_case --count runs.txt
run_case --coun runs.txt
# The count is padded to seven columns, which only a byte comparison can see.
run_stdin 'x\n'                 -c
run_stdin 'a\na\nb\n'           -c

# --- which part of a group is printed ------------------------------------------
run_case -d runs.txt
run_case -u runs.txt
run_case -D runs.txt
run_case -d -u runs.txt
run_case -u -d runs.txt
run_case --repeated runs.txt
run_case --unique runs.txt
run_case -d allsame.txt
run_case -u allsame.txt
run_case -D allsame.txt
run_case -d alldiff.txt
run_case -u alldiff.txt
run_case -D alldiff.txt
run_case -d empty.txt
run_case -u empty.txt
run_case -D empty.txt
run_case -d unterminated.txt
run_case -D unterminated.txt
run_case -u unterm-one.txt
run_case -cd runs.txt
run_case -cu runs.txt

# --- --all-repeated ------------------------------------------------------------
run_case --all-repeated runs.txt
run_case --all-repeated=none runs.txt
run_case --all-repeated=prepend runs.txt
run_case --all-repeated=separate runs.txt
run_case --all-repeated=s runs.txt
run_case --all-repeated=p runs.txt
run_case --all-repeated=n runs.txt
run_case --all-repeated=separate allsame.txt
run_case --all-repeated=prepend allsame.txt
run_case --all-repeated=separate alldiff.txt
run_case --all-repeated=separate empty.txt
# A later `-D` resets the method the long form asked for; the reverse does not.
run_case --all-repeated=separate -D runs.txt
run_case -D --all-repeated=separate runs.txt
run_case --all-repeated=separate --all-repeated=prepend runs.txt
# An optional argument is never taken from the next word: `separate` is a file.
run_case --all-repeated separate

# --- --group -------------------------------------------------------------------
run_case --group runs.txt
run_case --group=separate runs.txt
run_case --group=prepend runs.txt
run_case --group=append runs.txt
run_case --group=both runs.txt
run_case --group=b runs.txt
run_case --group=a runs.txt
run_case --group=p runs.txt
run_case --group=s runs.txt
run_case --group=both empty.txt
run_case --group=append empty.txt
run_case --group=both one.txt
run_case --group=both allsame.txt
run_case --group=both alldiff.txt
run_case --group=both unterminated.txt
run_case --group runs.txt
# Same rule for the optional argument here.
run_case --group separate
run_case --grou=append runs.txt
run_case --g runs.txt

# --- fields --------------------------------------------------------------------
run_case -f1 fields.txt
run_case -f2 fields.txt
run_case -f9 fields.txt
run_case -f0 fields.txt
run_case --skip-fields=1 fields.txt
run_case --skip-f 1 fields.txt
run_case -f 1 fields.txt
run_case -f1 tabfields.txt
run_case -f2 tabfields.txt
run_case -f1 leading.txt
run_case -f2 leading.txt
run_case -f1 alldiff.txt
run_case -f1 empty.txt
run_case -f1 blanks.txt
run_case -c -f1 fields.txt
run_case -f1 -c fields.txt
run_case -D -f1 fields.txt
# A field is its leading blanks plus the run that follows, so where -f stops is
# *before* the next blank — which is why -s counts differently after it.
run_stdin 'aa Xbc\naa Ybc\n' -f1 -s1
run_stdin 'aa Xbc\naa Ybc\n' -f1 -s2
run_stdin 'aa Xbc\naa Ybc\n' -s2
run_stdin '\tx a\n\ty a\n'   -f1
run_stdin '   \n   \n'       -f1
run_stdin ' \n\n'            -f1

# --- characters ----------------------------------------------------------------
run_case -s1 prefixes.txt
run_case -s2 prefixes.txt
run_case -s9 prefixes.txt
run_case -s0 prefixes.txt
run_case --skip-chars=2 prefixes.txt
run_case --skip-c 2 prefixes.txt
run_case -s1 growing.txt
run_case -s9 growing.txt
run_case -s1 empty.txt
run_case -c -s2 prefixes.txt

# --- check-chars ---------------------------------------------------------------
run_case -w1 prefixes.txt
run_case -w2 prefixes.txt
run_case -w3 prefixes.txt
run_case -w0 prefixes.txt
run_case -w0 alldiff.txt
run_case -w9 growing.txt
run_case -w1 growing.txt
run_case --check-chars=2 prefixes.txt
run_case --check-c 2 prefixes.txt
run_case -c -w1 prefixes.txt
run_case -w1 -s1 prefixes.txt
run_case -f1 -w1 fields.txt

# --- case folding ---------------------------------------------------------------
run_case -i mixedcase.txt
run_case mixedcase.txt
run_case --ignore-case mixedcase.txt
run_case --ign mixedcase.txt
run_case -i -c mixedcase.txt
run_case -i -d mixedcase.txt
# Above ASCII nothing folds, on either side, under either locale.
run_case -i utf8case.txt
run_ascii -i utf8case.txt
run_case -i badbytes.txt

# --- bytes that are not text ----------------------------------------------------
run_case badbytes.txt
run_case -c badbytes.txt
run_case crlf.txt
run_case -c crlf.txt
run_stdin 'a\r\na\r\n'

# --- NUL-terminated records ------------------------------------------------------
run_case -z nul.txt
run_case -z -c nul.txt
run_case -z -d nul.txt
run_case -z nul-unterminated.txt
run_case --zero-terminated nul.txt
run_case --z nul.txt
run_case -z --group=both nul.txt
# A newline inside a NUL record still separates fields, which is the one place
# the record delimiter and the field separator visibly disagree.
run_case -z -f1 nul-fields.txt
run_case -z nul-fields.txt
# Without -z the same file is one line whose newline never arrives.
run_case nul.txt
run_case -c nul.txt

# --- the obsolete -N skip-fields form --------------------------------------------
run_case -1 fields.txt
run_case -2 fields.txt
run_case -0 fields.txt
# Digits accumulate across separate arguments: this skips twelve fields.
run_case -1 -2 fields.txt
run_case -12 fields.txt
run_case -1 -i -2 fields.txt
# `-f` restarts the accumulator; the digits after it accumulate among themselves.
run_case -f3 -1 fields.txt
run_case -1 -f3 fields.txt
run_case -f3 -1 -2 fields.txt
run_case -1 -c fields.txt
run_case -1 -s1 fields.txt
# A run of digits long enough to overflow saturates rather than failing.
run_case -99999999999999999999999999 fields.txt

# --- the obsolete +N skip-chars form ----------------------------------------------
run_case +1 prefixes.txt
run_case +2 prefixes.txt
run_case +0 prefixes.txt
run_case prefixes.txt +2
run_case +2 -c prefixes.txt
# Disqualified by `--`, so it is a file name and the open fails.
run_case -- +2
run_case +2 -- prefixes.txt
# Disqualified by not being an exact number — where `-s` with the same digits
# saturates and runs.
run_case +99999999999999999999999
run_case -s 99999999999999999999999 prefixes.txt
run_case +x
run_case +
run_case +-1
run_case ' +2' prefixes.txt

# --- the environment ---------------------------------------------------------------
# `_POSIX2_VERSION` in the withdrawn window makes `+N` a file name; outside it,
# and when unparsable, it does not. The window is half-open at both ends.
for v in 200111 200112 200808 200809 200112x ' 200112' ''; do
  ENVV=("_POSIX2_VERSION=$v"); run_case +2 prefixes.txt
done
ENVV=()
# `POSIXLY_CORRECT` ends option parsing at the first operand, so `-c` after one
# becomes the OUTPUT file rather than an option. Both halves are checked.
ENVV=(POSIXLY_CORRECT=1)
run_case -c runs.txt
run_case runs.txt -c
run_case runs.txt +2
run_case -1 fields.txt
run_case fields.txt -1
ENVV=()
run_case runs.txt -c
run_case fields.txt -1

# --- operands and the OUTPUT file ----------------------------------------------------
run_outfile runs.txt
run_outfile -c runs.txt
run_outfile -z nul.txt
run_outfile runs.txt -c
run_outfile nope.txt
run_outfile -x runs.txt
run_case runs.txt -
run_case - runs.txt
run_stdin 'a\na\n' -
run_ascii runs.txt alldiff.txt extra.txt
run_ascii runs.txt alldiff.txt one.txt two.txt
run_case nope.txt
run_case -c nope.txt
run_case ''

# --- getopt's five sentences ----------------------------------------------------
run_case -x runs.txt
run_case -f
run_case -s
run_case -w
run_case --skip-fields
run_case --skip-chars
run_case --check-chars
run_case --zz runs.txt
run_case --count=3 runs.txt
run_case --unique=x runs.txt
run_case --zero-terminated=x runs.txt
run_case --help=x
# An empty prefix matches every option, printing the table in declaration order.
run_case --=x
# `--c` is ambiguous between `--count` and `--check-chars`, and `--s` between
# the two `--skip-*`; the candidates are listed in declaration order rather than
# alphabetically, which is the thing a "tidied" table would break.
run_case --c runs.txt
run_case --s runs.txt
run_case --skip runs.txt
run_case --a runs.txt
run_case --u runs.txt
run_case --r runs.txt
# Abbreviations, every one of which the hand-written parser refused outright.
run_case --coun runs.txt
run_case --rep runs.txt
run_case --uni runs.txt
run_case --ig mixedcase.txt
run_case --ze nul.txt
run_case --all runs.txt
run_case --gro runs.txt

# --- the three number diagnostics, which quote nothing ---------------------------
run_case -f x
run_case -s x
run_case -w x
run_case -f ''
run_case -s ''
run_case -f -5
run_case -f 0x10
run_case -f '5x'
run_case -f '5 '
run_case -f ' 5' fields.txt
run_case -f '+5' fields.txt
run_case -f "a'b"
run_case -f 'a\b'
run_case -f 'a b'
run_case --skip-fields=x
run_case --skip-chars=x
run_case --check-chars=x
# Too large is not an error: it saturates.
run_case -f 18446744073709551616 fields.txt
run_case -s 18446744073709551616 prefixes.txt
run_case -w 18446744073709551616 prefixes.txt

# --- argmatch's two diagnostics --------------------------------------------------
run_ascii --group=zz
run_ascii --group=
run_ascii --all-repeated=zz
run_ascii --all-repeated=
run_ascii --group=x
run_ascii "--all-repeated=a'b"

# --- the cross-checks, in upstream's order ----------------------------------------
run_case --group -c runs.txt
run_case --group -d runs.txt
run_case --group -D runs.txt
run_case --group -u runs.txt
run_case --group --repeated runs.txt
run_case --group --all-repeated=separate runs.txt
run_case -c --group runs.txt
run_case -c -D runs.txt
run_case -D -c runs.txt
run_case -c --all-repeated=separate runs.txt
run_case -c -D --group runs.txt
# `--group` alongside options that are not output-selecting is fine.
run_case --group -i runs.txt
run_case --group -f1 fields.txt
run_case --group -z nul.txt
run_case --group -1 fields.txt

# --- differ on purpose ------------------------------------------------------------
D=directory-operand-cannot-be-opened-on-a-windows-host
# On SlateOS, and on any POSIX host, opening a directory succeeds and the *read*
# fails, so GNU says `error reading '.': Is a directory` and so do we. This
# harness runs a Windows build, where `File::open` of a directory fails outright
# and we report the open instead. The difference is the host's, not the code's —
# see the same trap in `filekind.rs`, where a Windows `is_file()` calls a pipe a
# regular file.
xfail_case "$D" .

# `--help`'s body matches GNU's byte for byte; what follows it does not, and
# must not. GNU closes every `--help` with `emit_ancillary_info` — links to
# gnu.org, the Translation Project and `info '(coreutils) uniq invocation'` —
# which name an upstream project this is not and documentation this does not
# ship. `--version` likewise names SlateOS coreutils rather than GNU coreutils
# 9.4 with its copyright and authors.
xfail_case help-closes-with-a-referral-to-the-gnu-project-which-this-is-not --help
xfail_case version-names-slateos-coreutils-not-gnu-coreutils --version
# The abbreviations still have to resolve, which the comparison cannot show
# while the outputs are expected to differ.
selfsame --he --help
selfsame --v --version
selfsame --vers --version

# --- summary ------------------------------------------------------------------
if [ "$HAVE_GNU" != yes ]; then
  echo "uniq-diff: skipped (no glibc uniq)"
  exit 0
fi
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d XPASS' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
