#!/usr/bin/env bash
# Differential test: our cut against GNU cut.
#
# `cut` copies bytes, so stdout is compared byte for byte with `od -An -c`. It
# has to be: half of what this implementation had to get right is invisible to
# a whitespace-trimming comparison — whether an unterminated last line gains a
# terminator, whether a suppressed line leaves a blank one behind, and whether
# `--output-delimiter` appears between two *touching* ranges or not.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `cut` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh` and `wc-diff.sh`.
#
# The locale is `C.UTF-8` throughout. Nothing `cut` does is locale-dependent —
# GNU 9.4 falls `-c` through to `-b`, so even "characters" are bytes — and the
# quote marks gnulib puts round a bad LIST agree here too: since §351 ours are
# U+2018/U+2019 in every locale, which is what GNU prints under any UTF-8 one.
# Those cases used to be referenced under `LC_ALL=C`, back when ours stayed
# ASCII (`open-questions.md` → B-Q2, since answered). `C` is now the setting in
# which the reference would be wrong.
set -u

# Our cut is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

# Built here, from the package named, rather than picked up out of `target/`.
# A harness that only *runs* that path measures whatever was written there
# last, which need not be current and need not even be this crate — see
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils cut "${OURS:-}") || exit 1
GNU=${GNU:-"wsl -e env LC_ALL=C.UTF-8 cut"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands
# on the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'probe\n' > .probe
if [ "$($GNU -b1-5 .probe 2>/dev/null)" = "probe" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "cut-diff: glibc cut not reachable in this directory (tried: $GNU); skipping"
fi
rm -f .probe

# --- fixtures ----------------------------------------------------------------
printf 'a:b:c\nd:e:f\n'                         > colon.txt
printf 'a\tb\tc\nd\te\tf\n'                     > tabs.txt
# Ragged: a line with one field, one with four, one with none at all.
printf 'a:b\np:q:r:s\nlonely\n'                 > ragged.txt
printf 'hello world\n'                          > nodelim.txt
printf 'abcdefghij\nklm\n'                      > bytes.txt
printf ''                                       > empty.txt
printf '\n\n\n'                                 > blanks.txt
printf 'a:b:c'                                  > unterminated.txt
printf 'lonely'                                 > unterm-nodelim.txt
printf 'a:b\0c:d\0'                             > nul.txt
printf 'a:b\0c:d'                               > nul-unterminated.txt
printf 'a\xffb:c\n\x80\xfe:z\n'                 > badbytes.txt
printf '\xc3\xa9\xc3\xa9:x\n'                   > utf8.txt
printf ':a::b:\n'                               > emptyfields.txt
printf 'a::b\n'                                 > adjacent.txt
printf 'q:r\n'                                  > w1.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 ref=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(cut | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    "$OURS_ABS" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $ref "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | "$OURS_ABS" "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $ref "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - "$GNU" "$@"; report "cut $*"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" "$GNU" "$@"
  report "printf '$input' | cut $*"
}

xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  compare - "$GNU" "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL cut %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS cut %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
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
    [ -n "${VERBOSE:-}" ] && printf 'OK   cut %s == cut %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF cut %s != cut %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- byte mode ---------------------------------------------------------------
run_case -b1 bytes.txt
run_case -b1,3,5 bytes.txt
run_case -b2-4 bytes.txt
run_case -b5- bytes.txt
run_case -b-3 bytes.txt
run_case -b1- bytes.txt
run_case -b99 bytes.txt
run_case -b99- bytes.txt
run_case -b1-99 bytes.txt
run_case -b 2-4 bytes.txt
run_case --bytes=2-4 bytes.txt
run_case --bytes 2-4 bytes.txt
# `-c` is `-b` in GNU 9.4: it counts bytes, and splits a multi-byte character.
run_case -c1 utf8.txt
run_case -c2 utf8.txt
run_case -c1-3 utf8.txt
run_case --characters=1-2 utf8.txt
# `-n` is accepted and does nothing whatever, including when it would matter.
run_case -b1 -n utf8.txt
run_case -n -b1-2 utf8.txt
run_case -nb1 utf8.txt
# Bytes come out in input order however the list was written, and once each.
run_case -b5,1,3 bytes.txt
run_case -b3,3,3 bytes.txt
run_case -b1-5,2-3 bytes.txt
run_case -b '1, 3, 5' bytes.txt
run_case -b '1	3' bytes.txt
# Empty and blank lines are still lines.
run_case -b1 empty.txt
run_case -b1 blanks.txt
run_case -b1-2 blanks.txt
run_stdin '' -b1
run_stdin 'ab' -b1
run_stdin 'ab' -b1-9
run_case -b1 unterminated.txt
run_case -b9 unterminated.txt
# Bytes are copied, never decoded.
run_case -b1-3 badbytes.txt
run_case -b2 badbytes.txt

# --- the output delimiter separates ranges, not bytes -------------------------
# The measurement this implementation was rewritten for: two ranges that merely
# touch stay two ranges, and two that overlap become one. Only
# `--output-delimiter` can see the difference.
run_case -b1-2,3-4 --output-delimiter=. bytes.txt
run_case -b1-2,2-4 --output-delimiter=. bytes.txt
run_case -b1,2,3 --output-delimiter=. bytes.txt
run_case -b1-3 --output-delimiter=. bytes.txt
run_case -b1,5- --output-delimiter=. bytes.txt
run_case -b1,3 --output-delimiter=ZZZ bytes.txt
run_case -b1,3 --output-delimiter= bytes.txt
run_case --complement -b2 --output-delimiter=. bytes.txt

# --- field mode --------------------------------------------------------------
run_case -f1 tabs.txt
run_case -f2 tabs.txt
run_case -f1,3 tabs.txt
run_case -f2- tabs.txt
run_case -f-2 tabs.txt
run_case -f9 tabs.txt
run_case -f1 -d: colon.txt
run_case -f2 -d: colon.txt
run_case -f2- -d: colon.txt
run_case -f3 -d: colon.txt
run_case -f1,3 -d: colon.txt
run_case --fields=2 --delimiter=: colon.txt
run_case --fields 2 --delimiter : colon.txt
run_case -d: -f1,3 --output-delimiter=+ colon.txt
run_case -d: -f1-2 --output-delimiter=+ colon.txt
# A ragged file: fields that do not exist contribute nothing, not an empty one.
run_case -f1 -d: ragged.txt
run_case -f2 -d: ragged.txt
run_case -f3 -d: ragged.txt
run_case -f2-3 -d: ragged.txt
run_case -f1- -d: ragged.txt
run_case -f2- -d: ragged.txt --output-delimiter=+
# Empty fields are fields.
run_case -f1 -d: emptyfields.txt
run_case -f2 -d: emptyfields.txt
run_case -f1-3 -d: emptyfields.txt
run_case -f1- -d: emptyfields.txt
run_case -f2 -d: adjacent.txt
run_case -f1-3 -d: adjacent.txt --output-delimiter=.

# --- lines with no delimiter --------------------------------------------------
# Printed whole by default, whatever the list says; suppressed entirely by -s.
run_case -f1 -d: nodelim.txt
run_case -f9 -d: nodelim.txt
run_case -f2- -d: nodelim.txt
run_case -s -f1 -d: nodelim.txt
run_case -s -f9 -d: nodelim.txt
run_case --only-delimited -f1 -d: nodelim.txt
run_case -f1 -d: ragged.txt -s
run_case -f2 -d: ragged.txt -s
# The `buffer_first_field` fork: whether field 1 is selected changes the code
# path taken, not just the output.
run_case -f1 -d: nodelim.txt -s
run_case -f2 -d: nodelim.txt -s
run_case -f1 -d: nodelim.txt
run_case -f2 -d: nodelim.txt
run_case -s -f1 -d: unterm-nodelim.txt
run_case -f1 -d: unterm-nodelim.txt
run_case -f1 -d: unterminated.txt
run_case -f3 -d: unterminated.txt
run_case -f1 -d: empty.txt
run_case -s -f1 -d: empty.txt
run_case -f1 -d: blanks.txt
run_case -s -f1 -d: blanks.txt
run_stdin 'a:b' -f2 -d:
run_stdin 'a:b' -f1 -d:
run_stdin 'no-delim' -f1 -d:
run_stdin 'no-delim' -s -f1 -d:

# --- delimiters that are not the default -------------------------------------
run_case -f1 -d' ' nodelim.txt
run_case -f2 -d' ' nodelim.txt
run_case -f1 --delimiter=' ' nodelim.txt
# An empty delimiter is the NUL byte, which makes `-z`-style records addressable
# from field mode.
run_case -f1 -d '' nul.txt
run_case -f2 -d '' nul.txt
run_case -f1- -d '' nul.txt
# The delimiter as the line terminator: every field becomes its own line.
run_case -f1 -d$'\n' colon.txt
run_case -f2 -d$'\n' colon.txt
run_case -f1- -d$'\n' colon.txt
run_case -f1 -d$'\n' unterminated.txt
run_case -f1 -d$'\n' empty.txt
# Regular-expression metacharacters are not: a delimiter is one byte, literally.
run_stdin 'a.b\n' -f2 -d.
run_stdin 'a*b\n' -f2 -d'*'
run_stdin 'a\\b\n' -f2 -d'\'

# --- NUL-terminated lines -----------------------------------------------------
run_case -z -f1 -d: nul.txt
run_case -z -f2 -d: nul.txt
run_case -z -b1 nul.txt
run_case -z -f1 -d: nul-unterminated.txt
run_case --zero-terminated -f1 -d: nul.txt
# Without `-z` the same file is one line whose newline never arrives.
run_case -f1 -d: nul.txt
run_case -b1-3 nul.txt
# `-z` makes a newline an ordinary byte, so it can be a delimiter or a field.
run_stdin 'a\nb\0' -z -f2 -d$'\n'
run_stdin 'a\nb\0' -z -b2

# --- complement ---------------------------------------------------------------
run_case --complement -b1 bytes.txt
run_case --complement -b1-3 bytes.txt
run_case --complement -b2,4 bytes.txt
run_case --complement -b1- bytes.txt
run_case --complement -b5- bytes.txt
run_case --complement -b1-2,3-4 bytes.txt
run_case --complement -f1 -d: colon.txt
run_case --complement -f2 -d: colon.txt
run_case --complement -f1,3 -d: colon.txt
run_case --complement -f1- -d: colon.txt
run_case --complement -f2 -d: ragged.txt
run_case --complement -s -f1 -d: nodelim.txt
run_case --comp -b1 bytes.txt

# --- operands -----------------------------------------------------------------
run_case -b1 colon.txt w1.txt
run_case -b1 colon.txt colon.txt
run_case -b1 -- colon.txt
run_stdin 'a:b\n' -f1 -d: -
run_stdin 'a:b\n' -f1 -d: - w1.txt
run_case -b1 nope.txt
run_case -b1 nope.txt colon.txt
run_case -b1 colon.txt nope.txt
run_case -b1 nope.txt alsonope.txt
run_case colon.txt -b1
run_case -b1 empty.txt colon.txt

# --- the cross-checks, in upstream's order ------------------------------------
run_case
run_case colon.txt
run_case -s colon.txt
run_case -d: colon.txt
run_case -b1 -c1 colon.txt
run_case -b1 -f1 colon.txt
run_case -f1 -c1 colon.txt
run_case -b1 -b2 colon.txt
run_case -f1 -f1 colon.txt
# A list is demanded before a bad combination is diagnosed.
run_case -d: -s colon.txt
# `-d` and `-s` are field-only, and the list-missing check comes first.
run_case -b1 -d: colon.txt
run_case -b1 -s colon.txt
run_case -c1 -d: colon.txt
run_case -c1 -s colon.txt
run_case -b1 -d: -s colon.txt
run_case -b1 -s -d: colon.txt
# `-n`, `-z` and `--output-delimiter` are not field-only.
run_case -b1 -n colon.txt
run_case -b1 -z colon.txt
run_case -b1 --output-delimiter=. colon.txt
# A tab delimiter given explicitly is still a delimiter for the -b check.
run_case -b1 -d$'\t' colon.txt

# --- getopt's five sentences --------------------------------------------------
run_case -x colon.txt
run_case -b
run_case -f
run_case -d
run_case --bytes
run_case --fields
run_case --delimiter
run_case --output-delimiter
run_case --zz colon.txt
run_case --complement=1 colon.txt
run_case --only-delimited=x colon.txt
run_case --zero-terminated=x colon.txt
# An empty prefix matches every option, printing the table in declaration order.
run_case --=x
# `--c` is ambiguous between `--characters` and `--complement`, and the
# candidates are listed in declaration order rather than alphabetically.
run_case --c1 colon.txt
run_case --c colon.txt
run_case --o colon.txt
# Abbreviations, every one of which the hand-written parser refused.
run_case --by=1 bytes.txt
run_case --ch=1 bytes.txt
run_case --fi=1 -d: colon.txt
run_case --de=: --fi=1 colon.txt
run_case --on --fi=1 --de=: nodelim.txt
run_case --ou=. --by=1,3 bytes.txt
run_case --z --by=1 nul.txt
run_case --com --by=1 bytes.txt

# --- the LIST grammar, which is gnulib's set_fields ---------------------------
run_case -f0 colon.txt
run_case -b0 colon.txt
run_case -c0 colon.txt
run_case -f'' colon.txt
run_case -b'' colon.txt
run_case -f- colon.txt
run_case -b- colon.txt
run_case -f, colon.txt
run_case -fx colon.txt
run_case -bx colon.txt
run_case -cx colon.txt
run_case -f1x colon.txt
run_case -f'1 2x' colon.txt
run_case -f1-2-3 colon.txt
run_case -b1-2-3 colon.txt
run_case -c1-2-3 colon.txt
run_case -f2-1 colon.txt
run_case -b9-1 colon.txt
run_case -f1-0 colon.txt
run_case -f0-1 colon.txt
run_case -f'1,' colon.txt
run_case -f',1' colon.txt
run_case -f'1,,2' colon.txt
run_case -f' ' colon.txt
run_case -f'-1-' colon.txt
run_case -f'1--2' colon.txt
run_case -f+1 colon.txt
run_case -f' 1' colon.txt
run_case -f'1 ' colon.txt
run_case -f'1  2' colon.txt
# A number that will not fit names the whole run of digits, and only the first
# such run — the scan stops there.
run_case -f99999999999999999999 colon.txt
run_case -b99999999999999999999 colon.txt
run_case -f18446744073709551615 colon.txt
run_case -f18446744073709551616 colon.txt
run_case -f1,99999999999999999999 colon.txt
run_case -f99999999999999999999,88888888888888888888 colon.txt
run_case -f1-99999999999999999999 colon.txt
run_case -f99999999999999999999- colon.txt
run_case -f0009 colon.txt
run_case -f00000000000000000000009 colon.txt
# The offending text is echoed back through gnulib's `quote()`, which escapes
# the way C does and not the way a shell would. The two styles agree on
# everything that holds neither a quote nor a backslash, so only these cases
# tell them apart: `quote()` gives 'a\'b' where shell-escaping gives "a'b".
run_case -f"a'b" colon.txt
run_case -f'a\b' colon.txt
run_case -f'a"b' colon.txt
run_case -c"a'b" colon.txt
run_case -b'a\b' colon.txt
# The delimiter is one byte, and the sentence says so.
run_case -f1 -d:: colon.txt
run_case -f1 -dab colon.txt
run_case -f1 --delimiter=xy colon.txt

# --- values large enough to be open-ended but not too large -------------------
run_case -f18446744073709551614 colon.txt
run_case -b18446744073709551614- bytes.txt
run_case -b1-18446744073709551614 bytes.txt

# --- differ on purpose --------------------------------------------------------
D=directory-operand-cannot-be-opened-on-a-windows-host
# On SlateOS, and on any POSIX host, opening a directory succeeds and the
# *read* fails, so GNU says `error reading '.': Is a directory` and so do we.
# This harness runs a Windows build, where `File::open` of a directory fails
# outright and we say `cannot open '.' for reading: …` instead. The difference
# is the host's, not the code's — see the same trap in `filekind.rs`, where a
# Windows `is_file()` calls a pipe a regular file.
xfail_case "$D" -b1 .

# `--help`'s body matches GNU's byte for byte; what follows it does not, and
# must not. GNU closes every `--help` with `emit_ancillary_info` — links to
# gnu.org, the Translation Project and `info '(coreutils) cut invocation'` —
# which name an upstream project this is not and documentation this does not
# ship. `--version` likewise names SlateOS coreutils rather than GNU coreutils
# 9.4 with its copyright and authors.
xfail_case help-closes-with-a-referral-to-the-gnu-project-which-this-is-not --he
xfail_case version-names-slateos-coreutils-not-gnu-coreutils --v
# The abbreviations above still have to resolve, which the comparison cannot
# show while the outputs are expected to differ.
selfsame --he --help
selfsame --v --version
selfsame --hel --help

# --- summary ------------------------------------------------------------------
if [ "$HAVE_GNU" != yes ]; then
  echo "cut-diff: skipped (no glibc cut)"
  exit 0
fi
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d XPASS' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
