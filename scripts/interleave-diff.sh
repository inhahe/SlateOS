#!/bin/sh
# interleave-diff.sh — one question, asked of every utility that answers it:
# when output and diagnostics go to the same place, do they arrive in the same
# order GNU's do?
#
#     prog file missing > log 2>&1
#
# GNU puts the contents of `file` first and the complaint about `missing`
# after it, because glibc's `error()` opens with `fflush (stdout)`. A port that
# writes its diagnostic straight to descriptor 2 while its own output is still
# sitting in a block buffer produces the same two pieces in the opposite order —
# the complaint at the *front* of the log, ahead of everything it comes after.
#
# ## Why a cross-utility harness, when each utility has its own
#
# The same argument as `write-error-diff.sh`, one level along. The per-utility
# harnesses capture standard output and standard error into *separate* files,
# which is the right way to adjudicate the content of each and is precisely the
# arrangement in which an ordering bug cannot be seen: two streams captured
# apart have no relative order to be wrong about. Only the merged stream has
# one, and no existing harness looks at it.
#
# And the machinery under the answer is shared, so the bug is not a utility's
# own: descriptor 1's buffer lives in `coreutils::stdfd` (`STDOUT`), and
# `diag!` flushes it before writing. A utility acquires the correct behaviour
# by using `Stream` and `diag!` and cannot acquire it any other way — so a bin
# that prints its output through `Stream` and its diagnostics through
# `eprintln!`, or one that buffers output outside `Stream` entirely, is wrong
# in a way that shows up here and nowhere else.
#
# ## Which utilities, and why not all of them
#
# The ones whose output goes through `coreutils::stdfd::Stream`. Everything
# else still prints through `std::io::Stdout`, which is line-buffered
# unconditionally — even to a file, where stdio would block-buffer — so it
# happens to interleave correctly and has no ordering to regress. As bins are
# converted onto `Stream` they should be added to `DIFF_BINS` and given a case
# below; a converted bin missing from this list is exactly the silent gap this
# file exists to close.
#
# ## Each case adjudicates its own eligibility
#
# The question "is the *order* right" can only be asked where the two sides
# already agree on the *content*. So every case runs twice on each side:
#
#   1. `>out 2>err` — separate. If the two sides' stdout, stderr or status
#      differ at all, the case is reported `n/a` and counted apart. That is a
#      content difference, and content is the per-utility harnesses' business —
#      duplicating their xfail lists here would mean maintaining a second copy
#      of a judgement, and a second copy is a copy that disagrees.
#   2. `>log 2>&1` — merged. Compared byte for byte. Since step 1 established
#      that both pieces match, any difference here is purely where they sit
#      relative to each other.
#
# A case that produces no output, or no diagnostic, is reported `vacuous`
# rather than passed: it has no ordering to get right, so counting it as green
# would be counting a case that stopped testing anything.
#
# ## Cases that differ on purpose
#
# None. Unlike `write-error-diff.sh` there is no glibc artifact to accommodate
# here — `fflush (stdout)` before a diagnostic is behaviour we can reproduce
# exactly, and do.
#
# Run `OURS=/usr/bin ./scripts/interleave-diff.sh` to confirm the harness still
# discriminates. `OURS` names a *directory* here, not a binary, since there is
# no single subject.
set -u

DIFF_PROG=interleave
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_BINS="cat comm expand fold head join md5sum nl paste sha256sum tsort
           unexpand wc"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; na=0; vacuous=0; skipped=

# --- the two sides ------------------------------------------------------------
# One directory per side, each holding every utility under its bare name, so
# `argv[0]` — and therefore the `prog: ` prefix on every diagnostic — is
# identical on both sides.
mkdir -p "$bindir/ours" "$bindir/gnu"
for prog in $DIFF_BINS; do
  gnu_bin=
  for cand in /usr/bin/$prog /bin/$prog; do
    [ -x "$cand" ] && { gnu_bin=$cand; break; }
  done
  if [ -n "${OURS:-}" ]; then our_bin=$OURS/$prog; else our_bin=$(diff_ours "$prog"); fi
  if [ -z "$gnu_bin" ] || [ ! -x "$our_bin" ]; then
    skipped="$skipped $prog"
    continue
  fi
  ln -s "$our_bin" "$bindir/ours/$prog"
  ln -s "$gnu_bin" "$bindir/gnu/$prog"
done

have() { [ -e "$bindir/ours/$1" ]; }

# --- fixtures -----------------------------------------------------------------
fix=$DIFF_TMP/fix
mkdir -p "$fix"
printf 'alpha\nbravo\ncharlie\n' > "$fix/f"
printf 'alpha\ndelta\n'          > "$fix/g"
printf 'a\tb\tc\n\tindented\n'   > "$fix/tabs"
printf 'a b\nb c\nc d\n'         > "$fix/pairs"
printf '1 left\n2 left\n'        > "$fix/j1"
printf '1 right\n2 right\n'      > "$fix/j2"
# Bigger than the 4 KiB output buffer, so the run has already flushed several
# times before it fails and the *tail* is what is still pending. A utility can
# get the small case right by accident — its whole output fits in one write
# that happens to land first — and still misplace this one.
seq 1 200000                     > "$fix/huge"

# --- run one case on both sides -----------------------------------------------
VERDICT=na; REPORT=

# `compare PROG ARGS…` sets `VERDICT` to one of: pass, fail, na, vacuous.
compare() {
  local prog="$1"; shift
  local side dir o_rc g_rc m_o_rc m_g_rc rc
  local d=$DIFF_TMP/case
  rm -rf "$d"; mkdir -p "$d"

  # Step 1: separate, to establish that the content agrees at all.
  for side in ours gnu; do
    ( cd "$fix" && env PATH="$bindir/$side" "$prog" "$@" ) \
      >"$d/$side.out" 2>"$d/$side.err"
    # On the very next line, before anything else runs — including a `[ ]`
    # test, whose own status would silently replace it.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  if [ "$o_rc" != "$g_rc" ] \
     || ! cmp -s "$d/ours.out" "$d/gnu.out" \
     || ! cmp -s "$d/ours.err" "$d/gnu.err"; then
    VERDICT=na
    REPORT="  the two sides differ in content or status already; ordering is not the question here"
    return 0
  fi

  # A case with nothing on one of the two streams has no ordering to check.
  if [ ! -s "$d/gnu.out" ] || [ ! -s "$d/gnu.err" ]; then
    VERDICT=vacuous
    REPORT="  produced no output, or no diagnostic -- nothing to interleave"
    return 0
  fi

  # Step 2: merged, which is the actual question.
  for side in ours gnu; do
    ( cd "$fix" && env PATH="$bindir/$side" "$prog" "$@" ) >"$d/$side.log" 2>&1
    rc=$?
    if [ "$side" = ours ]; then m_o_rc=$rc; else m_g_rc=$rc; fi
  done

  if [ "$m_o_rc" = "$m_g_rc" ] && cmp -s "$d/ours.log" "$d/gnu.log"; then
    VERDICT=pass
    REPORT=
    return 0
  fi

  VERDICT=fail
  # Both logs hold the same bytes by construction — step 1 proved it — so
  # printing them whole would be printing the fixture. What distinguishes them
  # is *where* the diagnostic lines sit, so report the line numbers the stderr
  # lines landed on in each merged log. Every one of them, since a case with
  # two diagnostics can have the first in the right place and the second not.
  local o_at g_at
  o_at=$(grep -n -F -x -f "$d/gnu.err" "$d/ours.log" 2>/dev/null | cut -d: -f1 | tr '\n' ',')
  g_at=$(grep -n -F -x -f "$d/gnu.err" "$d/gnu.log"  2>/dev/null | cut -d: -f1 | tr '\n' ',')
  REPORT=$(printf '  ours (rc=%s): diagnostics on lines %sof %s\n  gnu  (rc=%s): diagnostics on lines %sof %s' \
    "$m_o_rc" "${o_at:-<absent> }" "$(wc -l < "$d/ours.log")" \
    "$m_g_rc" "${g_at:-<absent> }" "$(wc -l < "$d/gnu.log")")
  return 0
}

# `run PROG ARGS…` — one case, reported.
run() {
  have "$1" || return 0
  local label
  label=$(printf '%s' "$*")
  compare "$@"
  case $VERDICT in
    pass)
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK      %s\n' "$label" ;;
    na)
      na=$((na+1))
      [ -n "${VERBOSE:-}" ] && printf 'n/a     %s\n%s\n' "$label" "$REPORT" ;;
    vacuous)
      vacuous=$((vacuous+1))
      printf 'VACUOUS %s\n%s\n' "$label" "$REPORT" ;;
    *)
      fail=$((fail+1))
      printf 'DIFF    %s\n%s\n' "$label" "$REPORT" ;;
  esac
  return 0
}

# --- output, then a diagnostic ------------------------------------------------
# The shape the fix is about: a file that is read and printed, then one that
# cannot be opened. The complaint belongs after the contents.
run cat        f nosuch
run nl         f nosuch
run wc         f nosuch
run md5sum     f nosuch
run sha256sum  f nosuch
run expand     tabs nosuch
run unexpand   -a tabs nosuch
run fold       -w 3 f nosuch
run head       f nosuch
# `paste` has no such shape: it opens every operand before it writes anything,
# so a missing one is reported with the output still empty and there is nothing
# to interleave. It stays in `DIFF_BINS` so the build covers it, and is left
# without a case rather than given one that would report `vacuous` every run.

# Two diagnostics around one piece of output, so a utility that flushes only
# once still has to flush in the right place twice.
run cat        nosuch f alsonosuch
run wc         nosuch f alsonosuch
run md5sum     nosuch f alsonosuch

# --- the same, past the buffer ------------------------------------------------
# Several flushes have already happened before the failure, so what is pending
# is a partial buffer rather than the whole output. A utility whose one flush
# is at exit gets the small cases right and this one wrong.
run cat        huge nosuch
run nl         huge nosuch
run wc         huge nosuch
run md5sum     huge nosuch

# --- a diagnostic that is not about a file ------------------------------------
# `comm` and `join` complain about their *input's order* mid-run, after they
# have already printed the part they read before noticing. Upstream reports
# only if the output survived, which puts the two in a fixed order.
if have comm; then
  printf 'b\na\n' > "$fix/unsorted"
  run comm       f unsorted
  run comm       unsorted f
fi
if have join; then
  printf '2 x\n1 y\n' > "$fix/jbad"
  run join       j1 jbad
  run join       jbad j2
fi

# `tsort` prints the vertices it could order and then complains about the cycle
# among the rest.
if have tsort; then
  printf 'a b\nb c\nc a\nd e\n' > "$fix/cycle"
  run tsort      cycle
fi

# --- what could not be asked --------------------------------------------------
printf '\n'
[ -n "$skipped" ] && printf 'not compared (no reference or no build):%s\n' "$skipped"

printf '%d passed, %d differed, %d not applicable, %d vacuous\n' \
  "$pass" "$fail" "$na" "$vacuous"
[ "$fail" -eq 0 ]
