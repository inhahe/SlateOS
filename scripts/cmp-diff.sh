#!/usr/bin/env bash
# Differential test: our cmp against GNU diffutils' cmp.
#
# ## Why the subject is built for Linux, and why *both* sides run inside WSL
#
# `cmp` is one of the utilities whose body sits behind `#[cfg(unix)]`, because
# it opens `-` as descriptor 0, stats for `st_dev`/`st_ino` to recognise two
# names for one file, and seeks to honour `-i`. Built for Windows it compiles to
# a stub that prints `unix-only utility` and exits 2, so a harness pointed at a
# native build would run that stub 138 times and learn nothing.
#
# Building the subject inside WSL instead is the answer `du-diff.sh`,
# `find-diff.sh` and `ls-diff.sh` already use; see `design-decisions.md` §365
# and §374. It needs a Rust toolchain there — `rustup` under the WSL user's
# `$HOME`, which is not the Windows one. Without it this script skips with a
# message rather than failing, exactly as its siblings skip when glibc is
# unreachable.
#
# What this one does differently is re-exec *itself* under WSL, so the whole
# comparison happens on one side of the boundary. Its three siblings stay on
# Windows and reach across per case. Both work; running inside is simpler once
# the fixtures include names that are not valid UTF-16, which Windows cannot
# hold and this harness must create.
#
# ## Why the build lands in the WSL filesystem
#
# `$HOME/.cache/slateos-diff-target` inside WSL, not the repository's `target/`.
# A Linux build writing into the shared `target/` would fight the Windows build
# for the same fingerprint database, so the two would invalidate each other and
# every run of either would be a full rebuild. And `D:` is the drive that
# actually runs out of space — `CLAUDE.md` has a whole section on it — while the
# WSL volume does not.
#
# The directory is *shared* with du-diff, find-diff and ls-diff: same workspace,
# same triple, so their dependency artifacts are identical and a directory each
# stored the same objects four times over (§374). It is a deliberate long-lived
# cache, not scratch — delete it with
# `wsl rm -rf ~/.cache/slateos-diff-target` if it is ever in the way.
#
# ## Cases that differ on purpose
#
# Four kinds, every case of them recorded as `xfail`:
#
#   * `--help` omits the GNU project's `Report bugs to:` block, and `--version`
#     names SlateOS. As everywhere here.
#   * A file name that reaches the output is quoted and escaped. GNU
#     interpolates the raw bytes, so `cmp 'sp ace' $'nl\nname'` prints what
#     looks like two result lines, and a name that is not text lands on the
#     terminal as-is. See `design-decisions.md` §373 and `cmp.rs`'s header.
#   * A rejected `-i`/`-n` value is escaped inside its quotes for the same
#     reason; GNU passes the raw byte through.
#   * Under `-l`, we flush stdout before writing the `EOF on …` note to stderr,
#     so the note follows the rows it summarises. GNU's order is whatever its
#     buffering produces — the rows first on a terminal, the note first into a
#     pipe. Only `xfail_merged` can see this; with the two streams captured
#     apart, as every other case here does, the question does not arise.
#
# Run `OURS=/usr/bin/cmp ./scripts/cmp-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# --- get ourselves into WSL --------------------------------------------------
# `$0` may be an MSYS path (`/d/visual studio projects/...`) or a Windows one.
# `wslpath` translates whatever it is; it exists only inside WSL, which is also
# how we tell we are already there.
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "cmp-diff: no WSL on this host; skipping (our cmp is a unix-only binary)"
    exit 0
  fi
  here=$(cd "$(dirname "$0")" && pwd)
  # `wsl wslpath` converts a Windows path; MSYS's own `/d/...` form is not one,
  # so hand over the mixed form cygpath produces, which WSL does understand.
  if command -v cygpath >/dev/null 2>&1; then here=$(cygpath -m "$here"); fi
  inside=$(wsl wslpath -u "$here" 2>/dev/null) || {
    echo "cmp-diff: could not map $here into WSL; skipping"
    exit 0
  }
  exec wsl -e env "OURS=${OURS:-}" "VERBOSE=${VERBOSE:-}" \
    bash "$inside/cmp-diff.sh"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- the reference -----------------------------------------------------------
if ! command -v cmp >/dev/null 2>&1; then
  echo "cmp-diff: no GNU cmp inside WSL; skipping"
  exit 0
fi

# --- the subject -------------------------------------------------------------
OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cmp-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  target_dir=$HOME/.cache/slateos-diff-target
  # Built every run, for the reason `diff-subject.sh` spells out at length: a
  # harness that merely runs a path measures whatever was last written there.
  ( cd "$root" && cargo build -p coreutils --bin cmp \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  OURS=$target_dir/x86_64-unknown-linux-gnu/debug/cmp
fi
if [ ! -x "$OURS" ]; then
  echo "cmp-diff: $OURS is not executable" >&2
  exit 1
fi
# Absolute, because the symlinks below are followed from a different directory.
case $OURS in
  /*) ;;
  *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
esac

# Diagnostics are referenced under a UTF-8 locale, as everywhere since §351.
# It matters more here than usual: `cmp` picks the word `byte` or `char` for its
# result line from `hard_locale (LC_MESSAGES)`, so the locale is not merely
# cosmetic. `C.UTF-8` is present on every glibc build; a named territory locale
# such as `en_US.UTF-8` may not be generated, and GNU would then fall back to
# `C` and say `char` where we — reading the environment variable, having no
# `setlocale` — would say `byte`. Both are covered below, the second only if the
# system really has the locale.
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)

# --- one name for both sides -------------------------------------------------
# Each binary is reached through a symlink called `cmp`, in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word
# `cmp` on both sides. This is not tidiness: `cmp` with no operands prints
# `missing operand after '%s'`, and the `%s` is `argv[argc - 1]` — which, when
# there were no arguments at all, is `argv[0]`. Invoked by its build path our
# binary would name that path while GNU, found on `PATH`, named `cmp`, and the
# harness would report a difference it had manufactured itself.
gnu_real=$(command -v cmp)
bindir=$(mktemp -d)
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/cmp"
ln -s "$gnu_real" "$bindir/gnu/cmp"

trap 'chmod -R u+rwx "$fixtures" 2>/dev/null; rm -rf "$fixtures" "$bindir"' EXIT
cd "$fixtures" >/dev/null || exit 1

# --- fixtures ----------------------------------------------------------------
# Three lines, differing in the middle one, so the first difference is neither
# at the start nor at the end and both the byte and the line number are > 1.
printf 'abc\ndef\nghi\n'    > a
printf 'abc\ndeX\nghi\n'    > b
# A prefix of `a`, so one input runs out while the other continues — and it ends
# *on* a newline, which is the `line N` half of the EOF message.
printf 'abc\n'              > short
# Ends mid-line, which is the `in line N+1` half. The distinction between these
# two is the single easiest thing to get wrong in this program.
printf 'abc\ndef'           > noeol
printf ''                   > empty
# Differ at byte 2 with no newline anywhere, so `line` stays 1 and the shorter
# one ends inside it.
printf 'aXcdefgh'           > x1
printf 'aYc'                > x2
# Big enough to cross the 64 KiB read buffer several times, differing well past
# it: a comparison that only ever looked at the first buffer would pass every
# case above and fail here.
head -c 200000 /dev/urandom > big1
cp big1 big2
printf 'Q' | dd of=big2 bs=1 seek=150000 conv=notrunc status=none
# A second name for the same inode, for the shortcut that answers without
# reading. `cmp a a` on a large image must not cost the image.
ln a hardlink
mkdir subdir
# Names that are not text, which is the defect this rewrite exists for: the
# version it replaces collected argv as `Vec<String>` and panicked outright on
# the first of these.
printf 'aaa\n'              > $'\xff\xfe-bad'
printf 'bbb\n'              > $'\xff\xfe-bad2'
# Names that can forge a line of `cmp`'s own output.
printf 'sp ace content\n'   > 'sp ace'
printf 'other\n'            > $'nl\nname'

compare() {
  local stdin=$1; shift
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(cmp | od)` the recorded status is
  # od's. See the same note in cat-diff.sh.
  if [ "$stdin" = "-" ]; then
    timeout -k 2 60 env PATH="$bindir/ours" cmp "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    timeout -k 2 60 env PATH="$bindir/gnu"  cmp "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | timeout -k 2 60 env PATH="$bindir/ours" cmp "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | timeout -k 2 60 env PATH="$bindir/gnu"  cmp "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  # `od -An -c`, not the text: under `-l` the output is columns of octal whose
  # *width* is the thing being measured, and a comparison that collapsed runs of
  # spaces would agree with every wrong width.
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  # stderr in full. Every EOF message and every usage diagnostic lives there, so
  # a harness that only asked "did it complain?" would pass on all four EOF
  # wordings and on the `cmp: ` prefix that diffutils puts on its referral line
  # and GNU coreutils does not.
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

run_case()  { compare - "$@"; report "cmp $*"; }
run_stdin() {
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | cmp $*"
}
# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "cmp $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "cmp $*" "$why"
  fi
  return 0
}

# The same comparison with the two streams merged into one, which is the only
# way the *interleaving* of stdout and stderr is observable at all: `compare`
# captures them to separate files, where the question of which came first
# cannot arise. Only `-l` output has anything to interleave — the difference
# rows on stdout, the `EOF on …` note on stderr — so there is exactly one
# customer, but without this the header's claim about ordering would be a claim
# no test could see.
compare_merged() {
  local o_txt g_txt o_rc g_rc o_all g_all
  o_all=$(mktemp); g_all=$(mktemp)
  timeout -k 2 60 env PATH="$bindir/ours" cmp "$@" </dev/null >"$o_all" 2>&1; o_rc=$?
  timeout -k 2 60 env PATH="$bindir/gnu"  cmp "$@" </dev/null >"$g_all" 2>&1; g_rc=$?
  o_txt=$(od -An -c <"$o_all"); g_txt=$(od -An -c <"$g_all")
  rm -f "$o_all" "$g_all"
  if [ "$o_txt" = "$g_txt" ] && [ "$o_rc" = "$g_rc" ]; then AGREED=yes; else AGREED=no; fi
  REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s' \
    "$o_rc" "$(printf '%s' "$o_txt" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_txt" | tr -s ' \n' ' ')")
}

xfail_merged() {
  local why="$1"; shift
  compare_merged "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "cmp $* 2>&1" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "cmp $* 2>&1" "$why"
  fi
  return 0
}

# --- equal, and the shortcut -------------------------------------------------
run_case a a
run_case big1 big1
run_case empty empty
run_case /dev/null /dev/null
run_case a hardlink
run_case -l big1 big1
# Two names for one file that are *not* at the same offset after skipping, which
# is the case the shortcut must not swallow.
run_case -i 1:0 a hardlink

# --- the first difference ----------------------------------------------------
run_case a b
run_case x1 x2
run_case big1 big2
run_case -b a b
run_case -b big1 big2
run_case -c a b

# --- running out ------------------------------------------------------------
run_case a short
run_case short a
run_case a empty
run_case empty a
run_case a noeol
run_case noeol a
run_case /dev/null a
run_case a /dev/null

# --- -l ----------------------------------------------------------------------
run_case -l a b
run_case -l big1 big2
run_case -bl a b
run_case -l -b big1 big2
run_case -l -n 3 x1 x2

# --- -s ----------------------------------------------------------------------
run_case -s a a
run_case -s a b
run_case -s nosuch a

# --- -n ----------------------------------------------------------------------
run_case -n 0 big1 big2
run_case -n 1 a b
run_case -n 6 a b
run_case -n 7 a b
run_case -n 100000 big1 big2
run_case -n 1M big1 big2
run_case -n 1kB a b
run_case -n 5k a b
run_case -n 1Z a b
run_case -n 9223372036854775807 a b
run_case --bytes=4 a b
run_case -n5 a b
# Twice: the smaller wins, which is not what a plain assignment would do.
run_case -n 10 -n 3 a b
run_case -n 3 -n 10 a b

# --- -i ----------------------------------------------------------------------
run_case -i 4 a b
run_case -i 4:4 a b
run_case -i 1:9 a b
run_case -i 5 a b
run_case -i 0x4 a b
run_case -i 010 a b
run_case -i 1T a b
run_case -i 1000000 a b
run_case -i 149999 big1 big2
run_case -i 150000 big1 big2
run_case --ignore-initial=4 a b
run_case -i2 a b
# A skip so large the kernel refuses the seek outright. Both files end up at
# EOF, so they are equal; a run that let the `lseek` error escape said
# `Invalid argument` and exited 2 instead.
run_case -i 9223372036854775807 a b
run_case -i 9223372036854775807 a a
run_case -i 9223372036854775807:0 a b
run_case -i 9223372036854775807 big1 big2
run_case -s -i 9223372036854775807 a b
# Every way of naming a skip raises it and none of them lowers it, so a later
# `-i`, and a positional operand, can only ever move a slot up. `-l` is used for
# most of these because its first row names the two bytes actually reached, and
# so reads back both skips at once, where the default result line only says how
# far in the first difference was.
run_case -i 1:9 -i 5 a b
run_case -l -i 5 -i 1:9 x1 x1
run_case -i 5 -i 1:9 a b
# The third and fourth operands are SKIP1 and SKIP2. They go through the same
# maximum — `-i 5 a b 0 0` still skips 5 — and a lone SKIP1, unlike a lone `-i`,
# says nothing about the second file.
run_case a b 4 4
run_case a b 4
run_case a b 0 0
run_case -i 5 a b 0 0
run_case -l -i 5 a b 0 0
run_case -i 5 a b 10 10
run_case -i 10 a b 5 5
run_case -i 5 a b 3
run_case -l -i 5:6 big1 big2 7 2
run_case -l a b 3 4
run_case big1 big2 149999 149999
run_case a b 9223372036854775807 9223372036854775807

# --- stdin -------------------------------------------------------------------
run_case a - < a
run_stdin 'abc\ndef\nghi\n' a -
run_stdin 'abc\ndeX\nghi\n' a -
run_stdin 'abc\ndef\nghi\n' - a
run_stdin 'abc\n'           a -
run_stdin ''                a -
run_stdin 'abc\ndef\nghi\n' - -
run_stdin 'XXXXabc\ndef\nghi\n' -i 0:4 a -
run_stdin 'XXXXabc\ndef\nghi\n' -i 4 a -
# A skip on a stream that cannot seek has to be read and thrown away.
run_stdin 'abc\n' -i 9223372036854775807 - a
# Unbounded on the left, so `-n` is the only thing that ends the run.
run_case -n 10 /dev/zero /dev/zero
run_case -n 10 /dev/zero a
run_case -i 5 /dev/zero a

# --- option syntax -----------------------------------------------------------
run_case -- a b
run_case -- -l b
run_case a -- b
run_case a b -l
run_case -ls a b
run_case -sl a b
run_case -lb a b
run_case -bi4 a b
run_case --verb a b
run_case --print a b
run_case --quie a b
run_case --sil a b
run_case --byt=4 a b
run_case --ign=4 a b
run_case --print-chars a b

# --- diagnostics -------------------------------------------------------------
run_case
run_case a
run_case -s
run_case -Z a b
run_case --bogus a b
run_case -l -s a b
run_case a b c d e
run_case a b 1 2 3
run_case -n a b
run_case -i a b
run_case --bytes a b
run_case --ignore-initial a b
run_case -n= a b
run_case -i '' a b
run_case -n '' a b
run_case -i x a b
run_case -i 1: a b
run_case -i 2: a b
run_case -i :2 a b
run_case -i '1 ' a b
run_case -i 1:2:3 a b
run_case -n -1 a b
run_case -n 5Y a b
# 2^63 exactly: one past what an `off_t` holds, and the boundary GNU checks.
run_case -n 8E a b
run_case -n 1E a b
run_case -i 9223372036854775808 a b
run_case nosuch a
run_case a nosuch
run_case a subdir
run_case subdir a
run_case subdir subdir

# --- names that are not text -------------------------------------------------
# The point of these is that they run at all: the version this file replaces
# collected argv as `Vec<String>` and panicked on the first of them before
# reaching any of `cmp`'s own logic. Where the name reaches the *output* the two
# disagree on purpose — we escape it, GNU emits the raw bytes — so the two forms
# are split between a plain case and an xfail.
run_case -s $'\xff\xfe-bad' $'\xff\xfe-bad2'
xfail_case 'a name that is not text is escaped in the result line' \
  $'\xff\xfe-bad' $'\xff\xfe-bad2'
# `-l` names nothing but offsets, so the difference does not arise there.
run_case -l $'\xff\xfe-bad' $'\xff\xfe-bad2'
run_case $'\xff\xfe-bad' $'\xff\xfe-bad'

# --- the locale decides `byte` or `char` -------------------------------------
( export LC_ALL=C; compare - a b; report 'cmp a b [LC_ALL=C]' )
( export LC_ALL=C; compare - -b a b; report 'cmp -b a b [LC_ALL=C]' )
if locale -a 2>/dev/null | grep -qix 'en_US\.utf-\?8'; then
  ( export LC_ALL=en_US.UTF-8; compare - a b; report 'cmp a b [LC_ALL=en_US.UTF-8]' )
else
  # Without the locale generated, GNU's `setlocale` fails and the effective
  # locale stays `C`, so GNU says `char` while we — reading the environment
  # variable, `setlocale` being unavailable from Rust — say `byte`. That is a
  # property of the system, not of either program, so the case is skipped
  # rather than recorded as a difference. `localedef -i en_US -f UTF-8
  # ~/locales/en_US.UTF-8` and `LOCPATH=~/locales` make it run, and it passes.
  [ -n "${VERBOSE:-}" ] && printf 'skip en_US.UTF-8 case: locale not generated\n'
fi

# --- differences on purpose --------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --version names SlateOS' -v a b
xfail_case 'a name that could forge a line of output is quoted' 'sp ace' a
xfail_case 'a name holding a newline is quoted' $'nl\nname' a
xfail_case 'a rejected value is escaped inside its quotes' -i $'\xff' a b
# The rows go to stdout and the note to stderr; we flush between them so the
# note follows what it summarises. GNU leaves the order to its buffering: onto a
# terminal stdout is line-buffered and the rows win, into anything else it is
# block-buffered and the note jumps ahead. Only visible with the streams merged
# — with them captured separately, as everywhere above, the two agree. See the
# header.
run_case -l x1 x2
xfail_merged 'we flush stdout before the EOF note; GNU does not' -l x1 x2
xfail_merged 'we flush stdout before the EOF note; GNU does not' -bl x1 x2
# Not `-l a short`: `short` is a prefix of `a`, so there are no rows to order
# the note against and the two agree. It takes a pair that both differs and
# runs out.

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
