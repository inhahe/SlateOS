#!/usr/bin/env bash
# Differential test: our nl against GNU nl.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `nl` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll` rather
# than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `head-diff.sh`, `wc-diff.sh`, `cut-diff.sh` and
# `uniq-diff.sh`.
#
# ## Why `od -An -c`, and why every case pins `-w`
#
# `nl`'s entire output is whitespace placement: an unnumbered line is a run of
# spaces exactly as wide as the number would have been, `-nrz` differs from
# `-nrn` only in zeros where spaces were, a section delimiter turns into a bare
# newline, and the separator is arbitrary bytes. A comparison that collapsed or
# trimmed whitespace would agree with almost every wrong implementation, so
# stdout goes through `od -An -c` byte for byte.
#
# The corollary is that `-w` must never be large. `nl` accepts `-w 2147483647`
# and really does emit two gigabytes of spaces per line; a probe of exactly that
# produced a 2 GB transcript during measurement. Cases here stay in single
# digits, and the bounds of `-w` are tested through its *diagnostics* instead.
#
# ## Three cases that differ on purpose
#
# `-bp` compiles a POSIX **basic** regular expression. Ours goes through
# `ere::bre`, which refuses backreferences on purpose, and whose compile-error
# wording is its own rather than glibc's `Invalid regular expression`. Those are
# `xfail_case`d rather than silently tolerated.
#
# The locale is `C.UTF-8`, with the diagnostics that pass an argument through
# gnulib's `quote()` referenced under `LC_ALL=C` so the quote marks are ASCII on
# both sides. See `open-questions.md` → B-Q2.
set -u

# Our nl is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/nl.exe"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

run_ours() { "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; wsl -e env "LC_ALL=$loc" nl "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands on
# the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'probe\n' > .probe
if [ "$(run_gnu C.UTF-8 -w1 .probe 2>/dev/null)" = "$(printf '1\tprobe')" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "nl-diff: glibc nl not reachable in this directory; skipping"
fi
rm -f .probe

# --- fixtures ----------------------------------------------------------------
printf 'a\nb\nc\n'                        > plain.txt
printf 'a\n\nb\n\n\nc\n'                  > blanks.txt
printf 'a\n\n\n\nb\n\n\nc\n'              > runs.txt
printf ''                                 > empty.txt
printf '\n\n\n'                           > allblank.txt
printf 'only'                             > unterm.txt
printf 'a\nb'                             > unterm2.txt
printf 'x\n'                              > one.txt
# The default section delimiter is a backslash and a colon, so a header line is
# `\:\:\:`, a body line `\:\:` and a footer line `\:`.
printf '\\:\\:\\:\nH1\n\\:\\:\nB1\nB2\n\\:\nF1\n\\:\\:\nB3\n' > sec.txt
# Delimiter-lookalikes: the right prefix but the wrong length, and the right
# length repeated once too often.
printf '\\:x\n\\:\\:\\:\\:\nB\n\\:\\:\nC\n'                   > near.txt
# Blank lines straddling a section boundary, for the counter that survives it.
printf 'a\n\n\\:\\:\n\n\nb\n'                                 > straddle.txt
# Custom delimiters.
printf 'abcabcabc\nH\nabcabc\nB\nabc\nF\n'                    > abc.txt
printf 'x:x:x:\nH\nx:x:\nB\nx:\nF\n'                          > xcolon.txt
printf 'xbcxbcxbc\nH\nxbcxbc\nB\nxbc\nF\n'                    > xbc.txt
# Text for -bp to match against.
printf 'foo\nbar\nfoobar\nxfoo\n'                             > words.txt
printf 'ax\nxb\nx\n'                                          > anchors.txt
# Bytes that are not text at all, and CRLF, which must survive verbatim.
printf 'a\xff\n\xfe\xfd\nb\n'                                 > badbytes.txt
printf 'a\r\nb\r\n'                                           > crlf.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(nl | od)` the recorded status is
  # od's, and `PIPESTATUS` is set in the substitution's subshell where it cannot
  # be read. See the same note in cat-diff.sh.
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

  # stderr is compared in full, not merely for emptiness: the whole point of the
  # getopt module is that the sentences match, so a harness that only asked "did
  # it complain?" would pass on every wording this exists to fix.
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "nl $*"; }
# The diagnostics that pass an argument through gnulib's `quote()`. Under a
# UTF-8 locale gnulib switches to U+2018/U+2019 and ours does not — one open
# question, not one per case. See B-Q2. `nl`'s *number* diagnostics quote too,
# so almost every error case here is `run_ascii`.
run_ascii() { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "nl $* [C]"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C.UTF-8 "$@"
  report "printf '$input' | nl $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1"; shift
  compare - C.UTF-8 "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS nl %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL nl %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the three numbering styles, and the default -----------------------------
# The default body style is `t`, not `a`; the shipped parser had it the other
# way round, so these cases fail against it before any option is typed.
run_case -w3 plain.txt
run_case -w3 blanks.txt
run_case -w3 allblank.txt
run_case -w3 empty.txt
for style in a t n; do
  run_case -w3 -b$style plain.txt
  run_case -w3 -b$style blanks.txt
  run_case -w3 -b$style allblank.txt
done
# Only the first byte of a style argument is looked at.
run_case -w3 -baXYZ blanks.txt
run_case -w3 -btZZZ blanks.txt
run_case -w3 -bnQ   blanks.txt
run_case -w3 --body-numbering=aXYZ blanks.txt

# --- -n, the three formats ---------------------------------------------------
for fmt in ln rn rz; do
  run_case -ba -w4 -n $fmt plain.txt
  run_case -ba -w1 -n $fmt plain.txt
  run_case -bt -w4 -n $fmt blanks.txt
done
# A number wider than the field is not truncated, in any format.
for fmt in ln rn rz; do
  run_case -ba -w1 -n $fmt -v 1234 plain.txt
  run_case -ba -w4 -n $fmt -v -5 plain.txt
done
run_ascii -n l plain.txt
run_ascii -n r plain.txt
run_ascii -n LN plain.txt
run_ascii -n rnx plain.txt
run_ascii -n '' plain.txt
run_ascii --number-format=RZ plain.txt

# --- -w, -s: the width of the field and what follows it ----------------------
for w in 1 2 3 6 9; do run_case -ba -w $w plain.txt; done
run_case -ba -w3 -s '' plain.txt
run_case -ba -w3 -s -- plain.txt
run_case -ba -w3 -s ':::' blanks.txt
run_case -bt -w3 -s ':::' blanks.txt
run_case -bt -w3 -s '' blanks.txt
run_case -bn -w3 -s 'XY' plain.txt
run_case -ba -w3 --number-separator=@@ blanks.txt
# The unnumbered field is `width + strlen(separator)` spaces, so a multi-byte
# separator widens it. A UTF-8 separator makes that a byte count, not a
# character count.
run_case -bt -w3 -s 'é' blanks.txt
run_ascii -w 0 plain.txt
run_ascii -w -1 plain.txt
run_ascii -w abc plain.txt
run_ascii -w '' plain.txt
run_ascii -w 2147483648 plain.txt
# Which of the two `strerror` sentences an out-of-range value gets is decided by
# a heuristic on the *value* — `INT_MIN / 2 <= v <= INT_MAX / 2` — and not by
# the limit that was violated. Every row above falls off a limit at a magnitude
# where the two possible rules agree, which is exactly how a wrong rule survived
# in this utility until `xnum.rs` was written: the old code said "Numerical
# result out of range" for anything below the floor, and GNU says "Value too
# large" once the value is past `INT_MIN / 2`. These are the rows that separate
# them, and every harness for a utility with a *bounded* number needs them.
run_ascii -w -3000000000 plain.txt
run_ascii -l -3000000000 plain.txt
run_ascii -l -5 plain.txt
run_ascii -w -1073741824 plain.txt
run_ascii -w -1073741825 plain.txt

# --- -v and -i: signed, and zero is legal ------------------------------------
run_case -ba -w4 -v 10 plain.txt
run_case -ba -w4 -v -3 plain.txt
run_case -ba -w4 -v 0 plain.txt
run_case -ba -w4 -i 5 plain.txt
run_case -ba -w4 -i 0 plain.txt
run_case -ba -w4 -i -2 plain.txt
run_case -ba -w4 -v 10 -i 5 plain.txt
run_case -ba -w4 -v -10 -i 3 plain.txt
run_case -ba -w4 --starting-line-number=7 --line-increment=2 plain.txt
# xdectoimax's grammar: leading blank and sign yes, trailing junk and multiplier
# suffixes no.
run_case -ba -w4 -v ' 5' plain.txt
run_case -ba -w4 -v '+5' plain.txt
run_ascii -v 5x plain.txt
run_ascii -v 0x10 plain.txt
run_ascii -v 1K plain.txt
run_ascii -v abc plain.txt
run_ascii -v '' plain.txt
run_ascii -i xyz plain.txt
# The two out-of-range messages are different sentences.
run_ascii -v 9223372036854775808 plain.txt
run_ascii -v -9223372036854775809 plain.txt
run_ascii -v 99999999999999999999999999999999 plain.txt
run_case -ba -w4 -v 9223372036854775807 plain.txt
run_case -ba -w4 -v 9223372036854775806 plain.txt
run_case -ba -w4 -v -9223372036854775808 plain.txt

# --- -l: runs of empty lines counted as one ----------------------------------
for n in 1 2 3 4; do
  run_case -ba -w3 -l $n runs.txt
  run_case -ba -w3 -l $n blanks.txt
  run_case -ba -w3 -l $n allblank.txt
done
# -l is consulted only for style `a`.
run_case -bt -w3 -l 3 runs.txt
run_case -bn -w3 -l 3 runs.txt
run_case -bp. -w3 -l 3 runs.txt
run_case -ba -w3 --join-blank-lines=2 runs.txt
run_ascii -l 0 plain.txt
run_ascii -l -1 plain.txt
run_ascii -l abc plain.txt

# --- sections ----------------------------------------------------------------
run_case -w3 sec.txt
run_case -w3 -ba sec.txt
run_case -w3 -ba -ha -fa sec.txt
run_case -w3 -ba -ht -ft sec.txt
run_case -w3 -bn -ha -fn sec.txt
run_case -w3 -ba -p sec.txt
run_case -w3 -ba --no-renumber sec.txt
run_case -w3 -ba -v 100 sec.txt
run_case -w3 -ba -v 100 -p sec.txt
run_case -w3 -ba -i 3 sec.txt
run_case -w3 near.txt
run_case -w3 -ba near.txt
# The blank-line counter survives a section switch, so this run of four empty
# lines is one run even though a delimiter line sits in the middle of it.
run_case -w3 -ba -l3 straddle.txt
run_case -w3 -ba -l2 straddle.txt
run_case -w3 -ba -l4 straddle.txt

# --- -d, including the aliasing ----------------------------------------------
run_case -w3 -ba -d 'x:' xcolon.txt
run_case -w3 -ba -d x xcolon.txt
run_case -w3 -ba -d abc abc.txt
run_case -w3 -ba --section-delimiter=abc abc.txt
# The one-byte form overwrites the front of the *current* delimiter and leaves
# the tail, so this is `xbc`, not `x:`.
run_case -w3 -ba -d abc -d x xbc.txt
run_case -w3 -ba -d abc -d x abc.txt
run_case -w3 -ba -d abc -d x xcolon.txt
run_case -w3 -ba -d abc -d yz abc.txt
# The empty argument replaces outright and switches section matching off.
run_case -w3 -ba -d '' sec.txt
run_case -w3 -ba -d '' -d x sec.txt
run_case -w3 -ba -d x -d '' xcolon.txt
run_case -w3 -ba -d '\:' sec.txt
run_case -w3 -ba -d '::' sec.txt
run_stdin '::::::\nH\n::::\nB\n::\nF\n' -w3 -ba -d '::'

# --- -bp: basic regular expressions ------------------------------------------
run_case -w3 -bp'^foo' words.txt
run_case -w3 -bp'foo' words.txt
run_case -w3 -bp'foo$' words.txt
run_case -w3 -bp'x$' anchors.txt
run_case -w3 -bp'^x' anchors.txt
run_case -w3 -bp'^x$' anchors.txt
run_case -w3 -bp'.' blanks.txt
run_case -w3 -bp'[bf]' words.txt
run_case -w3 -bp'o*bar' words.txt
# A BRE, not an ERE: `\(`…`\)` group and `\{n\}` repeat, while a bare `(` or `+`
# is a literal.
run_case -w3 -bp'\(foo\)bar' words.txt
run_case -w3 -bp'o\{2\}' words.txt
run_case -w3 -bp'foo\|bar' words.txt
# The empty expression is legal here and matches every line, though `ere`
# refuses one on its own account. See `Style::Matching` in nl.rs.
run_case -w3 -bp blanks.txt
run_case -w3 -bp plain.txt
run_case -w3 --body-numbering=p blanks.txt
# The same styles on the other two sections.
run_case -w3 -hp'H' -fp'F' -bp'B' sec.txt
run_case -w3 -bn -hp'.' sec.txt
xfail_case 'ere::bre refuses backreferences; glibc accepts them here' \
  -w3 -bp'\(ab\)\1' words.txt
xfail_case 'the regex compile-error wording is ere::bre own, not glibc regcomp' \
  -w3 -bp'[' words.txt
xfail_case 'the regex compile-error wording is ere::bre own, not glibc regcomp' \
  -w3 -bp'\(' words.txt

# --- style diagnostics, and the fact that they accumulate --------------------
run_ascii -b X plain.txt
run_ascii -h X plain.txt
run_ascii -f X plain.txt
run_ascii -b '' plain.txt
run_ascii -h '' plain.txt
run_ascii --body-numbering=Q plain.txt
run_ascii --header-numbering=Q plain.txt
run_ascii --footer-numbering=Q plain.txt
run_ascii -bX -nY plain.txt
run_ascii -bX -nY -fQ plain.txt
run_ascii -bX -hY -fZ -nW plain.txt
# A deferred diagnostic followed by a fatal one prints both, and then no
# referral: the fatal path exits without calling usage().
run_ascii -bX -w0 plain.txt
run_ascii -w0 -bX plain.txt
run_ascii -bX -v abc plain.txt
# A getopt diagnostic mixes into the same batch, in argv order: getopt prints
# the sentence and returns '?', and nl's `default:` only clears its `ok` flag,
# so neither kind of message can hide the other and the pair swaps with argv.
run_ascii -Z -bX plain.txt
run_ascii -bX -Z plain.txt
# Two of getopt's own, either side of an option that works: the rest of the
# cluster still takes effect, so this numbers every line despite both errors.
run_ascii -w3 -Zb a --zz plain.txt
run_ascii -w3 -bZa plain.txt
# A missing argument ends the cluster but not the parse.
run_ascii -bX -w plain.txt
# A fatal number after a getopt sentence still prints it first, and still
# refuses the referral.
run_ascii -Z -w0 plain.txt

# --- getopt's five sentences -------------------------------------------------
run_ascii -Z plain.txt
run_ascii -b
run_ascii -h
run_ascii -d
run_ascii -n
run_ascii -w
run_ascii --body-numbering
run_ascii --section-delimiter
run_ascii --zzz-bogus plain.txt
run_ascii --no-renumber=x plain.txt
run_ascii --help=x plain.txt
run_ascii --version=x plain.txt
# Abbreviation: accepted when unambiguous, refused with the table's own order
# when not. `--n` hits four options and `--num` three.
run_case -w3 --body=a plain.txt
run_case -w3 --number-f=ln -ba plain.txt
run_case -w3 --sec=abc -ba abc.txt
run_case -w3 --start=9 -ba plain.txt
run_ascii --n=ln plain.txt
run_ascii --num=ln plain.txt
run_ascii --number=x plain.txt
run_ascii --=x plain.txt
run_ascii --no plain.txt
run_case -w3 --no-r -ba sec.txt

# --- operands ----------------------------------------------------------------
run_case -w3 plain.txt one.txt
run_case -w3 -ba plain.txt plain.txt
run_case -w3 -ba sec.txt plain.txt
run_case -w3 -ba plain.txt sec.txt plain.txt
run_case -w3 -- -ba
run_case -w3 -ba -- plain.txt
run_stdin 'a\nb\n' -w3 -ba
run_stdin 'a\nb\n' -w3 -ba -
run_stdin 'a\nb\n' -w3 -ba - -
run_case -w3 -ba nope.txt
run_case -w3 -ba plain.txt nope.txt plain.txt
run_case -w3 -ba nope.txt nope2.txt
run_case -w3 -ba ''
# `-h` is `--header-numbering`, not `--help`: it swallows the operand.
run_case -w3 -h plain.txt
run_case -w3 -b plain.txt

# --- bytes, terminators and line endings -------------------------------------
run_case -w3 -ba unterm.txt
run_case -w3 -ba unterm2.txt
run_case -w3 -ba unterm2.txt plain.txt
run_case -w3 -bt unterm.txt
run_case -w3 -ba badbytes.txt
run_case -w3 -bt badbytes.txt
run_case -w3 -ba crlf.txt
run_case -w3 -bt crlf.txt
# A CR-terminated line is not empty, so `-bt` numbers it.
run_stdin 'a\n\r\nb\n' -w3 -bt
run_stdin 'x' -w3 -ba
run_stdin '' -w3 -ba
run_stdin '\n' -w3 -bt
run_stdin '\n' -w3 -ba
# A section delimiter as the unterminated final line still fires, because the
# reader supplies the terminator that the length comparison needs.
run_stdin 'a\n\\:' -w3 -ba
run_stdin 'a\n\\:\\:' -w3 -ba

# --- overflow ----------------------------------------------------------------
# The check runs *before* the number is printed, so the last representable
# number is printed and only the line after it is refused.
run_ascii -ba -w1 -v 9223372036854775807 plain.txt
run_ascii -ba -w1 -v 9223372036854775806 plain.txt
run_ascii -ba -w1 -v 9223372036854775805 -i 2 plain.txt
run_ascii -ba -w1 -v -9223372036854775808 -i -1 plain.txt
# `-p` means the overflow is not cleared at a section boundary; without it, it
# is.
run_ascii -ba -w1 -v 9223372036854775807 sec.txt
run_ascii -ba -w1 -v 9223372036854775807 -p sec.txt

# --- --help and --version ----------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
