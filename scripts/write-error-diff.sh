#!/bin/sh
# write-error-diff.sh — one question, asked of every utility that answers it:
# what happens when standard output or standard error cannot be written?
#
# ## Why a cross-utility harness, when each utility has its own
#
# The per-utility harnesses (`logname-diff.sh`, `wc-diff.sh`, …) each ask this
# among many other things, and they are the right place for a utility's *own*
# wording. But the machinery under the answer is shared — `coreutils::stdfd`,
# `guard_std_fds!`, `stdfd::restore`, `close_stdout` — and it is shared in a
# way that fails **silently and per binary**:
#
#   * `guard_std_fds!()` is a macro that must be expanded *in the binary*. The
#     library cannot do it, because the constructor it installs has to land in
#     that executable's `.init_array`. Omit it and nothing breaks, nothing warns
#     and no test fails — the program simply writes to the `/dev/null` the Rust
#     runtime opened over the closed descriptor and exits 0, reporting success
#     for output nobody received.
#   * `stdfd::restore()` must then actually be *called*, and called before the
#     first thing that reads or writes a standard descriptor.
#
# Both holes were live when this file was written: `head` and `nl` had been
# converted to `Stream` + `close_stdout`, were clippy-clean, passed their own
# harnesses, and still exited 0 on `head f >&-` because neither had the macro.
# `nohup` had the macro and called `restore` only inside its exec path, so
# `nohup --version >&-` was a silent 0 while `nohup true >&-` was correct.
#
# A per-utility harness cannot notice a gap in a utility it does not cover, and
# there are ~80 of these. This file covers the whole set in one pass, so a bin
# converted without the macro is caught by the sweep rather than by whoever
# eventually redirects its output into a full disk.
#
# ## The five shapes, and why all five
#
# | shape | what it exercises |
# |---|---|
# | `>&-`         | a descriptor that is *not open*. Only an attempted write fails, so what is caught is the flush at exit. |
# | `>/dev/full`  | a descriptor that is open and always fails. Every write fails, including a mid-run flush. |
# | `2>&-`        | a lost diagnostic: gnulib's `close_stdout` closes descriptor 2 too and `_exit`s with `exit_failure` if it cannot. |
# | `2>/dev/full` | the same, by the other route. |
# | both closed   | nothing left but the exit status, which is then the whole of the answer. |
#
# `>&-` and `>/dev/full` are not interchangeable and neither subsumes the
# other: the first fails one write, the second fails all of them, and a utility
# that buffers reaches them at different points in its run.
#
# ## What is compared: the exit status and standard error, and not the output
#
# Deliberately. Every utility here differs from GNU in the *content* of at
# least `--help` and `--version`, and several differ in output that is correct
# for this OS rather than for GNU/Linux — which is what the per-utility
# harnesses exist to adjudicate, case by case, with their own xfail lists.
# Repeating that judgement here would mean maintaining a second copy of it, and
# a second copy is a copy that disagrees. What this file asserts is the part
# that is genuinely common: that a failed write is *noticed*, reported in
# GNU's words, and reflected in the status.
#
# ## Cases that differ on purpose
#
# One, and it is `/dev/full` only. GNU prints a bare `prog: write error` there,
# without the `: No space left on device` every other write failure carries.
# That is a glibc artifact rather than a decision: gnulib's `close_stream` sets
# `errno = 0` when `ferror` was already set and the following `fclose`
# succeeds, and glibc's `new_do_write` discards the buffer on a failed write so
# that it does. We print the errno. The harness accepts "ours is GNU's message
# plus a `: <errno>` suffix" as an expected difference and nothing else — so a
# message that goes missing, changes shape, or comes with the wrong status is
# still a failure. See known-issues.md ->
# `B-COREUTILS-WRITE-ERROR-OMITS-THE-ERRNO-ON-A-PARTIAL-FLUSH`.
#
# Note the divergence is *not* about which utilities: the same binary prints
# the errno or not depending on whether a flush had already failed earlier in
# the run. `head f >/dev/full` carries it and `head -n 200000 huge nosuch
# >/dev/full` does not. That is why the exemption is recognised by *shape*
# rather than kept as a list of cases: a list keyed on the utility would be
# wrong for half the invocations of the utilities on it, and a list keyed on
# the exact invocation would have to be rewritten every time a case is added.
# Nothing here can go stale, so there is no XPASS to report.
#
# ## Broken pipes are deliberately not here
#
# `seq | head -1` exits 141 under GNU and 0 under ours, on purpose: the target
# has no signals to be killed by (design-decisions.md §377). A harness case for
# it would be a permanent xfail asserting a decision, which the decision record
# already does.
#
# Run `OURS=/usr/bin ./scripts/write-error-diff.sh` to confirm the harness still
# discriminates. `OURS` names a *directory* here, not a binary, since there is
# no single subject — pointing it at the reference makes both sides the same
# program, which must report zero differences **and zero on purpose**: the five
# accepted ones are a real difference between the two implementations, so they
# have to vanish when there is only one. A control that still shows them is a
# harness comparing something against itself by accident.
set -u

DIFF_PROG=write-error
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED="timeout"
DIFF_BINS="basename cat comm dirname echo expand fold head join logname md5sum
           nice nl nohup paste printf pwd seq sha256sum tsort tty unexpand wc
           whoami yes"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; skipped=

# --- the two sides ------------------------------------------------------------
# One directory per side, each holding every utility under its bare name, so
# `argv[0]` — and therefore the `prog: ` prefix on every diagnostic — is
# identical on both sides. `OURS`, when set, names a *directory* here rather
# than a binary, since this harness has no single subject.
mkdir -p "$bindir/ours" "$bindir/gnu"
for prog in $DIFF_BINS; do
  # `command -v` finds the shell's builtin for `echo`, `printf` and `true`,
  # which is not what is being compared. Look on the filesystem instead.
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
# Large enough that the 4 KiB output buffer flushes mid-run, which is the only
# way to reach the "a flush already failed" branch — a different code path from
# the exit-time flush, and the one where GNU's wording changes.
seq 1 200000                     > "$fix/huge"

# --- run one case on both sides -----------------------------------------------
# `MODE` is one of: plain, closed, full, errclosed, errfull, both.
MODE=plain
AGREED=no; REPORT=; OUT_DIFFERS_BY_ERRNO=no

compare() {
  local prog="$1"; shift
  local o_out g_out o_err g_err o_rc g_rc side out err rc
  o_out=$(mktemp); g_out=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_out; err=$o_err
    else out=$g_out; err=$g_err; fi
    # A spelling per combination rather than a variable holding a redirection:
    # the shell expands redirections before variables, so `$REDIR` would arrive
    # as an argument and not as a redirection.
    case $MODE in
      plain)     ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >"$out" 2>"$err" ;;
      closed)    ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >&-     2>"$err" ;;
      full)      ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >/dev/full 2>"$err" ;;
      errclosed) ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >"$out" 2>&- ;;
      errfull)   ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >"$out" 2>/dev/full ;;
      both)      ( timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) >&-     2>&- ;;
    esac
    # On the very next line, before anything else runs — including a `[ ]`
    # test, whose own status would silently replace it.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_out" "$g_out" "$o_err" "$g_err"

  # Is ours GNU's message with an errno appended? That is the one accepted
  # difference — see the header.
  OUT_DIFFERS_BY_ERRNO=no
  case $MODE in
    full)
      case $o_msg in
        "$g_msg: "*) [ -n "$g_msg" ] && OUT_DIFFERS_BY_ERRNO=yes ;;
      esac ;;
  esac

  if [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s}\n  gnu  (rc=%s): err{%s}' \
    "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK    %s\n' "$label"
  elif [ "$OUT_DIFFERS_BY_ERRNO" = yes ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (ours names the errno; GNU drops it after a failed flush)\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF  %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# `run PROG ARGS...` — the current `MODE`, then back to `plain`.
run() {
  local label
  label=$(printf '[%-9s] %s' "$MODE" "$*")
  compare "$@"
  report "$label"
  MODE=plain
}

# `sweep PROG ARGS...` — the same invocation through every shape. This is the
# unit the harness is really made of: a utility is covered when all five agree.
sweep() {
  have "$1" || return 0
  local m
  for m in closed full errclosed errfull both; do
    MODE=$m
    run "$@"
  done
}

# --- every funnelled utility, doing its actual job ----------------------------
# The operand matters: a utility that writes nothing has nothing to fail to
# write, so `--version` alone would certify a program that never reaches its
# own output path.
sweep basename   /usr/local/lib/libfoo.so
sweep cat        "$fix/f"
sweep comm       "$fix/f" "$fix/g"
sweep dirname    /usr/local/lib/libfoo.so
sweep echo       alpha bravo
sweep expand     "$fix/tabs"
sweep fold       -w 3 "$fix/f"
sweep head       "$fix/f"
sweep join       "$fix/j1" "$fix/j2"
sweep logname
sweep md5sum     "$fix/f"
sweep nl         "$fix/f"
sweep paste      "$fix/f" "$fix/g"
sweep printf     '%s\n' alpha
sweep pwd
sweep seq        1 5
sweep sha256sum  "$fix/f"
sweep tsort      "$fix/pairs"
sweep tty
sweep unexpand   -a "$fix/tabs"
sweep wc         "$fix/f"
sweep whoami

# `nice` and `nohup` exec the command they are given, after which their own
# standard output is the command's and no longer theirs to report on. What is
# being asked of them here is the path that writes: their own `--version`.
sweep nice       --version
sweep nohup      --version
# And the exec path, which must *not* report a write error for output that was
# never theirs. `nohup` redirects a tty stdout to `nohup.out`; there is no tty
# here, so it passes the descriptor through and the closed one stays closed.
sweep nice       true
( cd "$DIFF_TMP" && sweep nohup true )

# `yes` never stops on its own, so it appears only in the shapes that stop it.
# That is not a gap: a write failure is the only thing that ends it.
if have yes; then
  for m in closed full; do MODE=$m; run yes alpha; done
fi

# --- the mid-run flush, where the wording changes -----------------------------
# Output long enough to fill the 4 KiB buffer before the run ends, so the
# failure is reached by a flush rather than by the exit-time close. `head`
# has an entirely different sentence here — `error writing 'standard output'`
# rather than `write error` — and reaching it is the only way to check that.
sweep head       -n 200000 "$fix/huge"
sweep cat        "$fix/huge"
sweep nl         "$fix/huge"
sweep seq        1 200000
sweep wc         -L "$fix/huge"
# A second operand after the failure: upstream's `head` stops at once and never
# opens it, which is visible only as the *absence* of a cannot-open complaint.
sweep head       -n 200000 "$fix/huge" "$fix/nosuch"
sweep cat        "$fix/huge" "$fix/nosuch"
sweep nl         "$fix/huge" "$fix/nosuch"

# --- the usage error, which never reaches a flush -----------------------------
# Upstream's `usage (EXIT_FAILURE)` reaches `atexit (close_stdout)` with an
# empty buffer, so a closed stdout adds nothing to it and the operand complaint
# is the whole output. A port that built its stream before parsing would print
# a spurious write error here.
sweep basename
sweep dirname
sweep comm       "$fix/f"
sweep join
sweep wc         --zzz-bogus
sweep head       --zzz-bogus
sweep nl         --zzz-bogus
sweep md5sum     --zzz-bogus
sweep nohup

# --- the ancillary output paths -----------------------------------------------
# `--help` and `--version` differ in *content* from GNU's on every utility here,
# which is why `MODE=plain` is never used for them. Their write failures do not
# differ, and they are the only output path some of these have.
sweep pwd        --version
sweep tty        --version
sweep whoami     --version
sweep logname    --version
sweep yes        --version
sweep echo       --version

# --- what could not be asked --------------------------------------------------
printf '\n'
[ -n "$skipped" ] && printf 'not compared (no reference or no build):%s\n' "$skipped"

printf '%d passed, %d differed, %d differ on purpose\n' "$pass" "$fail" "$xfail"
[ "$fail" -eq 0 ]
