#!/usr/bin/env bash
# Differential test: our dd against GNU dd.
#
# ## Why this harness compares files, and a directory, and stderr
#
# `dd` is three programs wearing one name. It is a stdout filter, like `cat`;
# it is a file writer, like `split`; and it is a reporter, whose statistics on
# stderr are the only evidence for how the copy was actually divided into
# records. All three have to be compared, because each hides a different class
# of bug:
#
# * **stdout** catches the conversions — `conv=ascii`, `swab`, `block` — which
#   are pure byte transformations.
# * **the directory** catches everything about `of=`, and in particular the
#   defect this rewrite exists to fix: `dd of=big seek=10` used to leave `big`
#   at its original length instead of cutting it at the seek offset, which no
#   amount of stdout comparison could see (`known-issues.md` →
#   `B-dd-DESTROYS-THE-OUTPUT-FILE-WHEN-seek-IS-GIVEN`). The manifest carries
#   each file's *size* as well as its bytes, since a file truncated at the
#   wrong place and a file not truncated at all can hold identical prefixes.
# * **stderr** catches the record accounting. `0+1 records in` against
#   `1+0 records in` is the whole difference between a short read that was
#   re-aggregated and one that was not, and it is invisible in the output
#   bytes, which are the same either way.
#
# So each case runs both implementations in a private directory, with the
# fixtures copied into each, and compares stdout, stderr, the exit status and a
# manifest of the directory afterwards. The two runs cannot share a directory:
# both write `out`, and whichever ran second would win.
#
# ## Why stderr is scrubbed, and exactly how much of it
#
# The last statistics line ends in a duration and a transfer rate —
# `3 bytes copied, 7.0546e-05 s, 42.5 kB/s` — and no two runs of anything share
# those. Everything to the left of `copied,` is a function of the copy alone:
# the byte count, and its two human-readable renderings for counts of 1000 and
# over (`900000 bytes (900 kB, 879 KiB)`). Those are compared; the tail is
# replaced with `ELIDED`.
#
# It is worth being precise about why the elision stops where it does. The
# renderings come from `coreutils::human` with `SI|AUTOSCALE` and then the same
# set with `BASE_1024`, and getting either wrong — a `9.8 KiB` that should be
# `9.7 KiB`, a missing space, `kB` for `KB` — is a real defect in a shared
# module that nothing else in this harness would catch. Eliding the whole line
# would have been one character shorter to write and would have certified
# nothing.
#
# `status=progress` needs a second, blunter scrub: its interim records are
# emitted on a wall-clock timer, so both *how many* there are and what byte
# count each names depend on how fast the machine was. `progress_case`
# therefore normalises every `... copied, ...` line down to a single constant
# and collapses adjacent duplicates, which compares the *shape* of the interim
# record and the fact that there was at least one, while letting the count
# float. The records-in/records-out lines and the final report are unaffected,
# because they are not adjacent to the burst.
#
# ## Why some cases pipe and some redirect
#
# A regular file on standard input is seekable and a pipe is not, and `dd`
# behaves differently on each in ways that are the whole substance of several
# operands:
#
# * `skip=` seeks on a file and *reads and discards* on a pipe.
# * `seek=` on a non-seekable output is a warning on some paths and fatal on
#   others.
# * a read from a pipe returns short, so `bs=` (one buffer) reports `0+2
#   records in` where `ibs=`/`obs=` (two buffers) re-aggregates to `0+1 records
#   out`. That distinction *is* `iflag=fullblock`, and it cannot be provoked
#   from a file at all.
#
# `stdin_case` gives the file, `pipe_case` the pipe. `slowpipe_case` writes the
# payload in two halves with a pause between, which is the only way to make a
# short read happen on demand rather than by luck — a single `printf` under
# `PIPE_BUF` lands in the pipe as one atomic write and `dd` reads all of it.
#
# ## Why every payload is small
#
# Bounded on purpose, at a few kilobytes. A large payload through a pipe makes
# the *number* of short reads a function of scheduling, so the record counts
# stop being deterministic and the harness starts failing at random. The one
# fixture over a kilobyte is 10000 bytes, and it is only ever read from a file.
#
# ## The cases that differ on purpose
#
# `--help` and `--version`, whose text is ours; and the nine `iflag=`/`oflag=`
# names that GNU/Linux honours and this platform cannot — `direct`,
# `directory`, `dsync`, `noatime`, `nocache`, `noctty`, `nofollow`, `nonblock`,
# `sync`. Those are carried in our symbol table with value 0 and rejected, for
# the reason `dd.rs`'s module documentation gives: upstream's own loop
# condition is `!(operand_matches (...) && entry->value)`, so GNU rejects a
# zero-valued name too — it is simply that the set which is zero on Linux
# (`binary`, `text`, `cio`, `nolinks`, all four of which this harness checks as
# ordinary agreeing cases) is not the set which is zero here.
#
# Run `OURS=/usr/bin/dd ./scripts/dd-diff.sh` to confirm the harness still
# discriminates: every expected difference should turn into an XPASS.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `dd` so `argv[0]` matches. See `scripts/diff-wsl.sh`. `timeout` is named
# because every invocation below is bounded with it: a `dd` that mishandled a
# short read would not produce wrong output, it would spin — and the reference
# is wrapped too, so the harness cannot hang on the day the reference is the
# broken one.
# Quoted because shellcheck's SC2209 cannot tell an unquoted command *name* from
# a forgotten `$(...)`, and `dd` is a command name. The quotes say "the string".
DIFF_PROG='dd'
DIFF_NEED="timeout stat cksum"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
work=$DIFF_TMP/work
mkdir -p "$fixtures" "$work"

# --- fixtures ----------------------------------------------------------------

# Eleven bytes with a terminator: the ordinary case, and short enough that
# every `bs=` in the harness either splits it or does not.
printf 'abcdefghij\n'                 > "$fixtures/alpha"

# All 256 byte values in order. The translation tables are 256 entries each and
# a transcription error in one of them is a one-byte difference, so anything
# less than the whole domain tests the tables partially by construction.
{
  i=0
  while [ "$i" -lt 256 ]; do
    # shellcheck disable=SC2059  # the octal escape *is* the format.
    printf "\\$(printf '%03o' "$i")"
    i=$((i + 1))
  done
} > "$fixtures/ramp"

# Variable-length records, one of them empty, and no terminator on the last.
# This is what `conv=block` is for, and the empty record and the unterminated
# tail are the two corners it gets wrong.
printf 'ab\ncdef\n\nxyz'              > "$fixtures/text"

# Fixed-length records with trailing spaces, which is what `conv=unblock`
# consumes: it strips the padding and adds a newline.
printf 'ab  cd  '                     > "$fixtures/fixed"

# Mixed case and digits, for `conv=lcase` and `conv=ucase` — which must leave
# everything that is not a letter alone.
printf 'aBc DeF 123!\n'               > "$fixtures/mixed"

# A hundred identical bytes, so that a seek or a skip lands somewhere whose
# surroundings are uniform and the *offset* is the only thing on trial.
{ i=0; while [ "$i" -lt 100 ]; do printf 'A'; i=$((i + 1)); done; } > "$fixtures/hundred"

# 10000 bytes in a repeating ten-byte cycle, so a block boundary that moved is
# legible in a hex dump rather than merely unequal. 10000 is also the smallest
# round size whose statistics line exercises the human-readable renderings
# (`10000 bytes (10 kB, 9.8 KiB)`); under 1000 they do not appear at all.
{ i=0; while [ "$i" -lt 1000 ]; do printf '0123456789'; i=$((i + 1)); done; } > "$fixtures/k10"

# Five bytes, for the cases that need an input shorter than any block.
printf 'xxxxx'                        > "$fixtures/five"

cd "$work" >/dev/null || exit 1

# --- machinery ----------------------------------------------------------------

# One invocation of one side, in that side's own directory. `$1` is `ours` or
# `gnu`; each is reached through a symlink named `dd` in a directory that is the
# whole of `PATH` for that one invocation, so `argv[0]` is the bare word on both
# sides and the `dd: ` prefix on every diagnostic matches.
#
# `diff_run` keeps bash's own announcement of a child that died of a signal out
# of the stderr the caller captures; `diff-wsl.sh` says why. The subshell does
# not make it unnecessary — the subshell is the shell that waits on the child,
# and it inherited the caller's redirected stderr along with everything else.
run_side() {
  local side=$1 dir=$2; shift 2
  ( cd "$dir" && diff_run timeout -k 2 30 env PATH="$bindir/$side" dd "$@" )
}

# A file's contents, rendered for comparison and for the failure report.
#
# `od -An -c` below a kilobyte and a checksum above it. The threshold is not
# thrift for its own sake: `k10` appears in both sides of most manifests, and
# rendering 10000 bytes as `od` output twice per case is several hundred
# thousand lines of shell string per run. A checksum detects the same
# differences; what it costs is the ability to *read* one, which is why the
# small files — every file a case actually produces — keep the dump.
bytes_report() {
  local f=$1
  if [ "$(stat -c %s "$f")" -le 1024 ]; then
    od -An -c < "$f" | tr -s ' \n' ' '
  else
    printf 'cksum %s' "$(cksum < "$f")"
  fi
}

# Every file in a run's directory, in name order, with its size and contents.
#
# The size is carried separately from the bytes because they answer different
# questions: a file truncated at the wrong offset and a file not truncated at
# all agree on every byte they both have. `seek=` is exactly that bug.
#
# Block count too, for files of a block or more, which is the only externally
# visible difference `conv=sparse` makes — the bytes it writes and the bytes a
# non-sparse copy writes are identical by definition, and the hole is
# observable only as allocation that did not happen.
#
# A glob, not `for f in $(ls)`: an unquoted command substitution is split on
# IFS, so a name containing a space arrives in pieces. Same fix, and same
# reason, as `csplit-diff.sh` and `split-diff.sh`.
manifest() {
  local dir=$1 f size
  ( cd "$dir" || return 0
    for f in *; do
      case $f in '*') [ -e "$f" ] || continue ;; esac
      size=$(stat -c %s "$f")
      if [ "$size" -ge 4096 ]; then
        printf '  %s: %s bytes, %s blocks: %s\n' \
          "$f" "$size" "$(stat -c %b "$f")" "$(bytes_report "$f")"
      else
        printf '  %s: %s bytes: %s\n' "$f" "$size" "$(bytes_report "$f")"
      fi
    done )
}

# The statistics line's duration and rate, gone. See the header.
scrub() { sed -e 's/ copied, .*$/ copied, ELIDED/'; }

# The same for `status=progress`, where the byte count on an interim record is
# a function of the clock too, and the number of records is as well. Every
# `copied,` line collapses to one constant and adjacent duplicates are removed,
# so a burst of any length compares equal to a burst of any other — but a burst
# of *none*, or one whose record was shaped differently, does not.
scrub_progress() {
  tr '\r' '\n' \
    | sed -e 's/^[0-9]* bytes\( ([^)]*)\)\{0,1\} copied, .*$/BYTES copied, ELIDED/' \
    | uniq
}

# $1 = how stdin is supplied, $2 = the stderr scrubber, rest = the whole argv.
#
# stdin is one of `-` (nothing, from `/dev/null`), `f:NAME` (the fixture as a
# regular, seekable file), `p:SPEC` (a `printf '%b'` payload through a pipe),
# or `s:SPEC1|SPEC2` (the same in two halves with a pause between, so that the
# first read is guaranteed short).
compare() {
  local stdin=$1 scrubber=$2; shift 2
  local o_err g_err o_rc g_rc o_out g_out o_msg g_msg o_man g_man

  rm -rf o g; mkdir -p o g
  cp "$fixtures"/* o/; cp "$fixtures"/* g/

  # stdout goes to a file outside the run directories: through a pipe the
  # recorded status would be the reader's, and `PIPESTATUS` is set in the
  # substitution's subshell where it cannot be read (the same note as
  # `cat-diff.sh`). Outside, because anything inside would join the manifest —
  # and because a regular file for stdout is what makes `dd seek=10 > img`
  # reach the same seek-and-truncate path as `dd seek=10 of=img`, which is the
  # whole reason `filekind::borrowed(1)` exists.
  local o_bin g_bin
  o_bin=$(mktemp -p "$DIFF_TMP"); g_bin=$(mktemp -p "$DIFF_TMP")
  o_err=$(mktemp -p "$DIFF_TMP"); g_err=$(mktemp -p "$DIFF_TMP")

  case $stdin in
    -)
      run_side ours o "$@" >"$o_bin" 2>"$o_err" </dev/null; o_rc=$?
      run_side gnu  g "$@" >"$g_bin" 2>"$g_err" </dev/null; g_rc=$?
      ;;
    f:*)
      run_side ours o "$@" >"$o_bin" 2>"$o_err" <"$fixtures/${stdin#f:}"; o_rc=$?
      run_side gnu  g "$@" >"$g_bin" 2>"$g_err" <"$fixtures/${stdin#f:}"; g_rc=$?
      ;;
    p:*)
      printf '%b' "${stdin#p:}" | run_side ours o "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
      printf '%b' "${stdin#p:}" | run_side gnu  g "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
      ;;
    s:*)
      local head=${stdin#s:} tail
      tail=${head#*|}; head=${head%%|*}
      { printf '%b' "$head"; sleep 0.4; printf '%b' "$tail"; } \
        | run_side ours o "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
      { printf '%b' "$head"; sleep 0.4; printf '%b' "$tail"; } \
        | run_side gnu  g "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
      ;;
    *) echo "dd-diff: bad stdin spec '$stdin'" >&2; exit 1 ;;
  esac

  o_out=$(bytes_report "$o_bin"); g_out=$(bytes_report "$g_bin")
  o_msg=$($scrubber <"$o_err"); g_msg=$($scrubber <"$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  o_man=$(manifest o); g_man=$(manifest g)

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_man" = "$g_man" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n%s\n  gnu  (rc=%s): %s  {%s}\n%s' \
    "$o_rc" "$o_out" "$(printf '%s' "$o_msg" | tr '\n' '|')" "$o_man" \
    "$g_rc" "$g_out" "$(printf '%s' "$g_msg" | tr '\n' '|')" "$g_man")
  rm -rf o g
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# Operands only, nothing on standard input.
run_case()   { compare - scrub "$@"; report "dd $*"; }

# The fixture named by `$1` on standard input, as a regular file.
stdin_case() { local f=$1; shift; compare "f:$f" scrub "$@"; report "dd $* < $f"; }

# A `printf '%b'` payload on standard input, through a pipe.
pipe_case()  { local d=$1; shift; compare "p:$d" scrub "$@"; report "printf '$d' | dd $*"; }

# The same in two halves, with a pause between, so the first read is short.
slowpipe_case() {
  local d=$1; shift
  compare "s:$d" scrub "$@"
  report "printf '${d%%|*}'; sleep; printf '${d#*|}' | dd $*"
}

# `status=progress`, whose interim records need the blunter scrubber.
progress_case() { local d=$1; shift; compare "s:$d" scrub_progress "$@"; report "dd $* (progress)"; }

# A case we expect to differ, with the reason. Counted separately so that a
# case that starts agreeing is reported too — an xfail that silently becomes
# correct is a stale note in the harness.
xfail_case() {
  local why="$1"; shift
  compare - scrub "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS dd %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL dd %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the plain copy -----------------------------------------------------------
run_case if=alpha
run_case if=alpha of=out
run_case if=text of=out
run_case if=k10 of=out
run_case if=five of=out
run_case if=/dev/null
run_case if=/dev/null of=out
stdin_case alpha
stdin_case alpha of=out
stdin_case k10 of=out
pipe_case 'abc'
pipe_case ''
pipe_case 'abc' of=out
# No `if=` and no `of=`: both ends are the inherited descriptors, which is the
# shape `filekind::borrowed` was added for.
stdin_case k10

# --- block sizes --------------------------------------------------------------
# The record counts are the point here, not the bytes: every one of these
# copies the same 10000 bytes and differs only in how the statistics divide
# them, including the `N+1` partial record whenever the size does not divide.
for b in 1 2 3 7 512 1000 4096 9999 10000 10001 20000; do
  run_case if=k10 of=out bs=$b
done
run_case if=alpha of=out bs=1
run_case if=alpha of=out bs=11
run_case if=alpha of=out bs=12
run_case if=/dev/null of=out bs=1

# `ibs=`/`obs=` is a different program from `bs=`: two buffers, so a partial
# input record is combined with the next before anything is written.
run_case if=k10 of=out ibs=512 obs=512
run_case if=k10 of=out ibs=1000 obs=3
run_case if=k10 of=out ibs=3 obs=1000
run_case if=k10 of=out ibs=1 obs=1
run_case if=k10 of=out ibs=512 obs=1000
run_case if=k10 of=out ibs=10000 obs=1
run_case if=alpha of=out ibs=4 obs=3
run_case if=alpha of=out ibs=3 obs=4
# `bs=` after `ibs=`/`obs=` overrides both, and before them is overridden.
run_case if=k10 of=out ibs=100 bs=1000
run_case if=k10 of=out bs=1000 ibs=100
run_case if=k10 of=out obs=100 bs=1000
run_case if=k10 of=out bs=1000 obs=100
# The last spelling of an operand wins.
run_case if=k10 of=out bs=1000 bs=2000
run_case if=alpha of=out if=text
run_case if=k10 of=out count=1 bs=100 count=2

# --- short reads, which only a pipe produces ----------------------------------
# One buffer does not re-aggregate and reports two partial records in *and*
# out; two buffers report two in and one out. That difference is the whole of
# `iflag=fullblock`, and it is reachable only from a non-seekable input.
slowpipe_case 'aaaaa|bbbbb' bs=4096
slowpipe_case 'aaaaa|bbbbb' ibs=4096 obs=4096
slowpipe_case 'aaaaa|bbbbb' bs=4096 iflag=fullblock
slowpipe_case 'aaaaa|bbbbb' ibs=4096 obs=4096 iflag=fullblock
slowpipe_case 'aaaaa|bbbbb' bs=4096 of=out
slowpipe_case 'aaaaa|bbbbb' bs=3
slowpipe_case 'aaaaa|bbbbb' bs=3 iflag=fullblock
slowpipe_case 'aaaaa|bbbbb' bs=4096 count=1
slowpipe_case 'aaaaa|bbbbb' bs=4096 count=1 iflag=fullblock
slowpipe_case 'aaaaa|bbbbb' bs=4096 conv=sync
# The partial-read warning, which fires only when `bs=` is given, `fullblock`
# is not, and something is being counted or skipped.
slowpipe_case 'aaaaa|bbbbb' bs=4096 count=2
slowpipe_case 'aaaaa|bbbbb' bs=4096 skip=1
slowpipe_case 'aaaaa|bbbbb' ibs=4096 obs=4096 count=2

# --- numbers, and their suffixes ---------------------------------------------
for n in 1 2 10 1b 2b 1w 1c 1x2 2x3x4 1K 1k 1KB 1M 1MB; do
  run_case if=k10 of=out bs=$n count=1
done
run_case if=k10 of=out bs=1KiB count=1
run_case if=k10 of=out bs=1G count=0
run_case if=k10 of=out bs=100000000 count=1
# A number that cannot be a block size, and the four ways of saying so.
run_case if=alpha of=out bs=0
run_case if=alpha of=out ibs=0
run_case if=alpha of=out obs=0
run_case if=alpha of=out cbs=0
run_case if=alpha of=out bs=zzz
run_case if=alpha of=out bs=-1
run_case if=alpha of=out bs=+1
run_case if=alpha of=out bs=' 1'
run_case if=alpha of=out bs=1.5
run_case if=alpha of=out bs=1Z
run_case if=alpha of=out bs=99999999999999999999999
run_case if=alpha of=out bs=9999999999999
run_case if=alpha of=out ibs=9999999999999
run_case if=alpha of=out obs=9999999999999
# The zero-multiplier warning, which is a warning and not an error: the copy
# goes on with the block size it computed.
run_case if=alpha of=out bs=0x1
run_case if=alpha of=out bs=00x1
run_case if=alpha of=out bs=1x0
run_case if=alpha of=out count=0x1
run_case if=alpha of=out skip=0x1
# Counts and offsets have their own bounds.
run_case if=alpha of=out count=-1
run_case if=alpha of=out skip=-1
run_case if=alpha of=out seek=-1
run_case if=alpha of=out count=zzz
run_case if=alpha of=out skip=zzz
run_case if=alpha of=out seek=zzz
run_case if=alpha of=out count=99999999999999999999999

# --- count= -------------------------------------------------------------------
for c in 0 1 2 3 9 10 11; do
  run_case if=k10 of=out bs=1000 count=$c
done
run_case if=k10 of=out bs=3 count=4
run_case if=alpha of=out bs=1 count=5
run_case if=alpha of=out bs=4 count=1
# `count=NB` and `iflag=count_bytes` are the same request said two ways, and
# the byte count is divided by `ibs` — not by `obs`, which is the corner.
run_case if=k10 of=out bs=1000 count=2500B
run_case if=k10 of=out bs=1000 count=2500 iflag=count_bytes
run_case if=k10 of=out ibs=100 obs=10 count=1050B
run_case if=k10 of=out ibs=100 obs=10 count=1050 iflag=count_bytes
run_case if=alpha of=out bs=4 count=5B
run_case if=alpha of=out count=0B
# A count past the end of the input is not an error.
run_case if=alpha of=out bs=1 count=1000
run_case if=alpha of=out bs=1000 count=1000

# --- skip= --------------------------------------------------------------------
for s in 0 1 5 9 10 11 20; do
  run_case if=k10 of=out bs=1000 skip=$s
done
run_case if=alpha of=out bs=1 skip=5
run_case if=alpha of=out bs=4 skip=1
run_case if=k10 of=out ibs=100 obs=10 skip=250B
run_case if=k10 of=out ibs=100 obs=10 skip=250 iflag=skip_bytes
run_case if=alpha of=out bs=4 skip=1B
# Past the end of a *file*, where the seek succeeds and the read finds nothing.
run_case if=alpha of=out bs=1 skip=100
run_case if=alpha of=out bs=100 skip=100
run_case if=five of=out bs=1 skip=10
# Past the end of a *pipe*, where there is nothing to seek and the skip has to
# read and discard — and then reports that it could not skip that far. That is
# a warning and the status is still zero, which is the second defect this
# rewrite fixed.
pipe_case 'abc' bs=1 skip=10
pipe_case 'abc' bs=1 skip=2
pipe_case 'abc' bs=1 skip=3
pipe_case 'abcdefgh' bs=2 skip=2
pipe_case 'abcdefgh' bs=2 skip=10
pipe_case 'abcdefgh' bs=3 skip=1 count=1
pipe_case 'abcdefgh' skip=1 iflag=skip_bytes
pipe_case 'abcdefgh' bs=1 skip=4 count=2

# --- seek=, and the truncation that goes with it ------------------------------
# The defect: without `conv=notrunc` the output is cut *at the seek offset*,
# not left alone and not emptied. `hundred` is a hundred bytes, so every one of
# these must end far shorter than it started.
run_case if=five of=hundred bs=1 seek=10
run_case if=five of=hundred bs=1 seek=10 conv=notrunc
run_case if=five of=hundred bs=512 seek=5B
run_case if=five of=hundred bs=512 seek=5B conv=notrunc
run_case if=five of=hundred bs=1 seek=0
run_case if=five of=hundred bs=1 seek=200
run_case if=five of=hundred bs=10 seek=2
run_case if=five of=hundred bs=10 seek=2 conv=notrunc
run_case if=five of=hundred seek=1 oflag=seek_bytes
run_case if=/dev/null of=hundred bs=1 seek=10
run_case if=/dev/null of=hundred bs=1 seek=10 conv=notrunc
run_case if=/dev/null of=hundred
run_case if=/dev/null of=hundred conv=notrunc
# Onto a file that does not exist yet, where the seek has to extend it.
run_case if=five of=out bs=1 seek=10
run_case if=five of=out bs=1 seek=10 conv=notrunc
run_case if=five of=out bs=512 seek=1
run_case if=alpha of=out ibs=100 obs=10 seek=35B
# Onto standard output, which this harness has made a regular file — the
# `dd seek=10 > img` spelling.
run_case if=five bs=1 seek=10
run_case if=five bs=1 seek=10 conv=notrunc
run_case if=five bs=512 seek=1
# Onto a pipe, where there is nothing to seek.
pipe_case 'abc' seek=1 of=/dev/null
pipe_case 'abc' seek=1 of=/dev/null conv=notrunc

# --- conv=block ---------------------------------------------------------------
for c in 1 2 3 4 5 8; do
  run_case if=text of=out cbs=$c conv=block
  run_case if=text of=out cbs=$c conv=unblock
done
run_case if=text of=out conv=block
run_case if=text of=out conv=unblock
run_case if=fixed of=out cbs=4 conv=unblock
run_case if=fixed of=out cbs=2 conv=unblock
run_case if=fixed of=out cbs=3 conv=unblock
run_case if=fixed of=out cbs=8 conv=unblock
run_case if=alpha of=out cbs=4 conv=block
run_case if=alpha of=out cbs=4 conv=unblock
run_case if=/dev/null of=out cbs=4 conv=block
run_case if=/dev/null of=out cbs=4 conv=unblock
run_case if=text of=out cbs=4 conv=block,sync
run_case if=text of=out cbs=4 conv=block,ucase
run_case if=text of=out cbs=4 conv=ucase,block
pipe_case 'ab\ncdef\n\nxyz' cbs=4 conv=block
pipe_case 'ab\ncdef\n\nxyz' cbs=2 conv=block

# --- conv=swab ----------------------------------------------------------------
for b in 1 2 3 4 5 512; do
  run_case if=alpha of=out bs=$b conv=swab
done
run_case if=ramp of=out bs=2 conv=swab
run_case if=ramp of=out bs=3 conv=swab
run_case if=five of=out bs=2 conv=swab
run_case if=five of=out bs=1 conv=swab
run_case if=/dev/null of=out conv=swab
pipe_case 'abcde' conv=swab
pipe_case 'abcde' bs=2 conv=swab
slowpipe_case 'abc|de' bs=4096 conv=swab
slowpipe_case 'abc|de' ibs=4096 obs=4096 conv=swab

# --- the character-set conversions --------------------------------------------
# The whole 256-byte domain through each table, in both directions, plus the
# implied `unblock`/`block` each of them turns on.
for c in ascii ebcdic ibm; do
  run_case if=ramp of=out conv=$c
  run_case if=ramp of=out cbs=8 conv=$c
  run_case if=ramp of=out cbs=4 conv=$c
  run_case if=alpha of=out cbs=4 conv=$c
  run_case if=text of=out cbs=4 conv=$c
done
run_case if=ramp of=out conv=lcase
run_case if=ramp of=out conv=ucase
run_case if=mixed of=out conv=lcase
run_case if=mixed of=out conv=ucase
# Order does not matter to the result: the case fold is applied after the
# charset translation whichever way round they are written.
run_case if=mixed of=out conv=ucase,ebcdic
run_case if=mixed of=out conv=ebcdic,ucase
run_case if=mixed of=out conv=lcase,ascii
run_case if=mixed of=out conv=ascii,lcase
run_case if=ramp of=out cbs=8 conv=ibm,ucase
run_case if=ramp of=out bs=2 conv=ascii,swab
run_case if=ramp of=out bs=2 conv=swab,ascii

# --- conv=sync ----------------------------------------------------------------
# Pads every short input record — with NULs ordinarily, with spaces when
# `block` or `unblock` is also in force.
run_case if=alpha of=out bs=4 conv=sync
run_case if=alpha of=out bs=100 conv=sync
run_case if=k10 of=out bs=512 conv=sync
run_case if=k10 of=out bs=3000 conv=sync
run_case if=alpha of=out ibs=4 obs=4 conv=sync
run_case if=text of=out cbs=8 conv=sync,block
run_case if=text of=out cbs=8 conv=sync,unblock
run_case if=/dev/null of=out bs=4 conv=sync
pipe_case 'ab' bs=8 conv=sync
slowpipe_case 'aaaaa|bbbbb' bs=4096 conv=sync,noerror

# --- conv=, the file-creation half --------------------------------------------
run_case if=alpha of=out conv=excl
run_case if=alpha of=hundred conv=excl
run_case if=alpha of=out conv=nocreat
run_case if=alpha of=hundred conv=nocreat
run_case if=alpha of=hundred conv=notrunc
run_case if=alpha of=hundred conv=nocreat,notrunc
run_case if=alpha of=out conv=fdatasync
run_case if=alpha of=out conv=fsync
run_case if=alpha of=out conv=fsync,fdatasync
run_case if=alpha of=out conv=noerror
run_case if=alpha of=out conv=noerror,sync
run_case if=alpha conv=fsync
run_case if=alpha conv=fdatasync
# `conv=sparse` writes holes where a whole block of the input was zero. The
# manifest's block count is the only thing that can see it.
run_case if=/dev/zero of=out bs=4096 count=3 conv=sparse
run_case if=/dev/zero of=out bs=4096 count=3
run_case if=/dev/zero of=out bs=1024 count=12 conv=sparse
run_case if=/dev/zero of=out bs=4096 count=3 conv=sparse,notrunc
run_case if=alpha of=out conv=sparse
run_case if=/dev/zero of=out bs=4096 count=3 conv=sparse,fsync

# --- the mutual exclusions ----------------------------------------------------
run_case if=alpha of=out conv=ascii,ebcdic
run_case if=alpha of=out conv=ascii,ibm
run_case if=alpha of=out conv=ebcdic,ibm
run_case if=alpha of=out conv=ascii,ebcdic,ibm
run_case if=alpha of=out cbs=4 conv=block,unblock
run_case if=alpha of=out conv=block,unblock
run_case if=alpha of=out conv=lcase,ucase
run_case if=alpha of=out conv=excl,nocreat
run_case if=alpha of=out conv=ascii,ascii
run_case if=alpha of=out conv=ucase,ucase

# --- iflag= and oflag= --------------------------------------------------------
# The five names that carry a value on both platforms.
run_case if=alpha of=out iflag=fullblock
run_case if=alpha of=out iflag=count_bytes count=4
run_case if=alpha of=out iflag=skip_bytes skip=4
run_case if=alpha of=out oflag=seek_bytes seek=4
run_case if=alpha of=out iflag=append
run_case if=alpha of=hundred oflag=append
run_case if=alpha of=hundred oflag=append conv=notrunc
run_case if=alpha of=out oflag=append
run_case if=alpha of=out iflag=fullblock,count_bytes count=4
run_case if=alpha of=out iflag=count_bytes,skip_bytes count=4 skip=2
run_case if=alpha of=out oflag=seek_bytes,append seek=4
# `fullblock` is an input flag only, and saying so is a *usage* error rather
# than an unknown-symbol one — a different code path from the four below.
run_case if=alpha of=out oflag=fullblock
run_case if=alpha of=out iflag=fullblock oflag=fullblock
# The four names whose `O_` value is zero on GNU/Linux too, so both sides
# reject them and the sentences must match exactly.
for f in binary text cio nolinks; do
  run_case if=alpha of=out iflag=$f
  run_case if=alpha of=out oflag=$f
done
# Unknown names, and the empty and repeated-comma spellings.
run_case if=alpha of=out iflag=zzz
run_case if=alpha of=out oflag=zzz
run_case if=alpha of=out iflag=
run_case if=alpha of=out oflag=
run_case if=alpha of=out iflag=,,
run_case if=alpha of=out iflag=append,zzz
run_case if=alpha of=out iflag=zzz,append
run_case if=alpha of=out conv=
run_case if=alpha of=out conv=,,
run_case if=alpha of=out conv=zzz
run_case if=alpha of=out conv=notrunc,zzz
run_case if=alpha of=out conv=ASCII
run_case if=alpha of=out iflag=APPEND

# --- status= ------------------------------------------------------------------
run_case if=alpha of=out status=none
run_case if=alpha of=out status=noxfer
run_case if=k10 of=out bs=512 status=noxfer
run_case if=k10 of=out bs=512 status=none
run_case if=alpha of=out status=zzz
run_case if=alpha of=out status=
run_case if=alpha of=out status=none,noxfer
run_case if=alpha of=out status=noxfer,none
run_case if=alpha of=out status=none status=noxfer
run_case if=alpha of=out bs=0 status=none
run_case if=nosuch of=out status=none
# The interim records, whose count and byte totals float; `scrub_progress`
# compares their shape and their presence. The payload is written in two halves
# with a pause long enough that a record is certain on both sides.
progress_case 'aaaaa|bbbbb' status=progress of=out
progress_case 'aaaaa|bbbbb' status=progress bs=1 of=out
progress_case 'aaaaa|bbbbb' status=progress,none of=out
progress_case 'aaaaa|bbbbb' status=none,progress of=out

# --- operands that cannot be opened, and operands that are not operands --------
run_case if=nosuch
run_case if=nosuch of=out
run_case of=nosuchdir/out if=alpha
run_case if=. of=out
run_case of=. if=alpha
run_case if= of=out
run_case if=alpha of=
run_case if=alpha of=out zzz
run_case if=alpha of=out zzz=1
run_case if=alpha of=out bs
run_case alpha
run_case if=alpha=x of=out
run_case IF=alpha of=out
run_case if=alpha of=out --
run_case -- if=alpha of=out
run_case if=alpha of=out -
run_case -x if=alpha
run_case --bogus if=alpha
run_case --help=x
run_case --version=x

# --- differ on purpose --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block, the SIGUSR1 paragraph and the flags we cannot honour' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are xfails rather than ordinary cases for the same
# reason as in `fold-diff.sh`: they reach the same two texts, so an ordinary
# row would report the body difference we already accept and hide the thing
# they are actually for — that `--he` and `--vers` resolve at all.
xfail_case 'an abbreviation of --help reaches our help text' --he
xfail_case 'an abbreviation of --version reaches our version text' --vers
# The nine names GNU/Linux honours and this platform cannot. See the header.
for f in direct directory dsync noatime nocache noctty nofollow nonblock sync; do
  xfail_case "iflag=$f is honourable on GNU/Linux and is zero-valued here" if=alpha of=out iflag=$f
done
for f in direct dsync nocache noctty nofollow nonblock sync; do
  xfail_case "oflag=$f is honourable on GNU/Linux and is zero-valued here" if=alpha of=out oflag=$f
done

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
