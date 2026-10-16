#!/usr/bin/env bash
# Differential test: our `hexdump` against util-linux 2.39.3's.
#
# The reference is Ubuntu's build, in `bsdextrautils`.
#
# What is compared: stdout, stderr and the exit status of each case.
#
#   * the canned formats (-b -c -C -d -o -x and the default), alone and
#     together, over inputs that end mid-line and mid-unit -- where the
#     blank-padding of the last line and the end address are decided -- and
#     over runs of identical lines, squeezed to `*` and not (-v);
#   * -e and -f formats: repetition and byte counts, every conversion
#     (%d %i %o %u %x %X %e %f %g %E %G %c %s %_a %_A %_c %_p %_u), the
#     flags, precisions and widths, escapes, and every way a format is
#     refused (each message quotes the format as upstream has rewritten it
#     so far);
#   * -n and -s, with their size suffixes, across several files -- a skip
#     longer than a regular file passes on to the next one -- and on a pipe,
#     which cannot seek;
#   * the files: missing ones (counted, and the rest still read), a
#     directory, standard input, and all of them failing;
#   * colours: --color=always with _L[...] units -- names, values, strings,
#     offsets and ranges, inversion -- and the terminal-colors.d files that
#     decide `auto` on a terminal, with TERM, HOME and XDG_CONFIG_HOME set
#     for the case, which is what `ulcolors` (util-linux's lib/colors.c) is
#     measured by;
#   * refusals, and closed or full standard descriptors;
#   * a standard input shared with the next command, which `exit` gives back
#     what was read ahead of what was used -- unless `close_stdout` ended the
#     run with `_exit` first.
#
# Cases that differ on purpose: --version and -V name this build.
set -u

DIFF_PROG='hexdump'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
fix=$DIFF_TMP/fix
mkdir -p "$fix"

# ---------------------------------------------------------------------------
# The inputs.
# ---------------------------------------------------------------------------
python3 - "$fix" <<'PY'
import os, sys
d = sys.argv[1]
def put(name, data):
    with open(os.path.join(d, name), "wb") as f:
        f.write(data)
put("empty", b"")
put("one", b"A")
put("fifteen", bytes(range(65, 80)))
put("sixteen", bytes(range(48, 64)))
put("seventeen", bytes(range(97, 114)))
put("text", b"Hello, world!\nThe quick brown fox\tjumps.\r\n\x00\x07\x08\x0b\x0c\x1b\x7f\x80\xff end\n")
put("allbytes", bytes(range(256)))
put("repeats", b"A" * 64 + b"B" * 16 + b"A" * 48 + b"tail")
put("zeros", b"\x00" * 100)
put("floats", bytes.fromhex("0000803f000000400000000000000000" "000000000000f03f182d4454fb210940"))
put("big", bytes((i * 7) & 0xff for i in range(5000)))
put("a.txt", b"first file\n")
put("b.txt", b"second file, a little longer than the first\n")
put("fmt-ok", b'# a comment\n\n   "%08.8_ax  " 16/1 "%02x " "\\n"\n')
put("fmt-bad", b'"%Q"\n')
os.mkdir(os.path.join(d, "dir"))
# terminal-colors.d trees: one that disables hexdump, one that also has a
# scheme (which is what makes upstream read the disable), one for xterm only.
for name, files in {
    "tc-disable": {"hexdump.disable": b""},
    "tc-disable-scheme": {"hexdump.disable": b"", "scheme": b"red 31\n"},
    "tc-enable-scheme": {"hexdump.disable": b"", "hexdump.enable": b"", "scheme": b""},
    "tc-term": {"hexdump@xterm-256color.disable": b"", "scheme": b""},
    "tc-other": {"cal.disable": b"", "scheme": b""},
}.items():
    base = os.path.join(d, name, "terminal-colors.d")
    os.makedirs(base)
    for fname, data in files.items():
        put(os.path.join(name, "terminal-colors.d", fname), data)
PY

# ---------------------------------------------------------------------------
# ptyrun.py: COMMAND with standard output on a terminal; what it wrote comes
# out on our standard output.
# ---------------------------------------------------------------------------
ptyrun=$DIFF_TMP/ptyrun.py
cat >"$ptyrun" <<'PY'
import os, subprocess, sys

master, slave = os.openpty()
p = subprocess.Popen(sys.argv[1:], stdin=subprocess.DEVNULL, stdout=slave)
os.close(slave)
out = b""
while True:
    try:
        chunk = os.read(master, 65536)
    except OSError:
        break
    if not chunk:
        break
    out += chunk
sys.stdout.buffer.write(out.replace(b"\r\n", b"\n"))
sys.exit(p.wait())
PY

# --- knobs ------------------------------------------------------------------
STDIN=/dev/null
REDIR=
PTY=
ENVS=()
NAME=hexdump
# SCRIPT: a shell script run instead of the command, with `$name` the program
# and "$@" the case's arguments -- for a second reader after it.
SCRIPT=
reset_knobs() { STDIN=/dev/null; REDIR=; PTY=; ENVS=(); NAME=hexdump; SCRIPT=; }

# Our binary and the reference under the name the case asks for.
for side in ours gnu; do
  ln -sf "$bindir/$side/hexdump" "$bindir/$side/hd"
done

run_side() {
  local side=$1; shift
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=C.UTF-8" "${ENVS[@]}")
  local script=$SCRIPT
  [ -n "$script" ] || script="exec \"\$name\" \"\$@\" $REDIR"
  local -a cmd=(timeout -k 2 20 "${envs[@]}" sh -c 'cd "$1"; name=$2; script=$3; shift 3; eval "$script"' _ "$fix" "$NAME" "$script" "$@")
  if [ -n "$PTY" ]; then
    diff_run python3 "$ptyrun" "${cmd[@]}" <"$STDIN"
  else
    diff_run "${cmd[@]}" <"$STDIN"
  fi
}

compare() {
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  LABEL="$NAME $*"
  [ "$STDIN" != /dev/null ] && LABEL="$LABEL <${STDIN##*/}"
  [ -n "$PTY" ] && LABEL="$LABEL [pty]"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  [ -n "$SCRIPT" ] && LABEL="$LABEL {$SCRIPT}"
  reset_knobs
  if [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$DIFF_TMP/o.out" | head -30)" "$(cat -A "$DIFF_TMP/o.err" | head -10)" \
    "$g_rc" "$(cat -A "$DIFF_TMP/g.out" | head -30)" "$(cat -A "$DIFF_TMP/g.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached hexdump on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# A guard against vacuous agreement: the reference must have dumped a file.
run_case -C sixteen
if ! grep -q '^00000000  30 31 32 33' "$DIFF_TMP/g.out"; then
  echo "hexdump-diff: the reference did not dump the fixture:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- the canned formats ---------------------------------------------------------
for f in empty one fifteen sixteen seventeen text allbytes repeats zeros big; do
  run_case "$f"
  for o in -b -c -C -d -o -x; do
    run_case "$o" "$f"
  done
done
run_case -C -x seventeen
run_case -x -C seventeen
run_case -b -c -d text
run_case -v repeats
run_case -C -v zeros
run_case -v -x zeros
run_case --canonical --no-squeezing repeats
run_case --one-byte-octal --one-byte-char --two-bytes-decimal --two-bytes-octal --two-bytes-hex fifteen
NAME=hd; run_case sixteen
NAME=hd; run_case -x sixteen
NAME=hd; run_case -Z

# --- -e and -f ------------------------------------------------------------------------
for e in \
  '"%x"' '4/1 "%02x" "\n"' '16/1 "%c"' '"%_p"' '16/1 "%_u " "\n"' '16/1 "%_c|" "\n"' \
  '1/4 "%d " "\n"' '1/8 "%lld"' '2/2 "%i\n"' '4/1 "%o " "\n"' '1/2 "%X\n"' '1/1 "%u\n"' \
  '1/4 "%e\n"' '1/8 "%f\n"' '1/8 "%g\n"' '1/4 "%E\n"' '1/8 "%G\n"' '"%5.5s|"' '1/5 "%s|"' \
  '"%_ad: " 4/1 "%02x" "\n"' '"%_Ax\n" "%_ao " 8/1 "%3d" "\n"' '"%07.7_Ad\n"' \
  '"%-8x|"' '"%+d|" "% d|"' '"%#x|%#o|"' '8 "%x"' '3/1 "%x " "\n"' '"\101\t\\\q"' \
  '"%_a[x"' '"%_ad\n" 4 "%_ad"' '"text only\n"' '"x" "y" "z\n"' ; do
  run_case -e "$e" text
  run_case -e "$e" floats
done
run_case -e '"%Q"' text
run_case -e '"%5"' text
run_case -e '"%_Z"' text
run_case -e '"%_aQ"' text
run_case -e '3/3 "%x"' text
run_case -e '1/3 "%e"' text
run_case -e '1/2 "%c"' text
run_case -e '1/2 "%_p"' text
run_case -e '"%s"' text
run_case -e '4 "%x%x"' text
run_case -e '"unterminated' text
run_case -e 'x' text
run_case -e '4/1' text
run_case -e '4x "%x"' text
run_case -e '99999999999999999999/1 "%x"' text
run_case -f fmt-ok text
run_case -f fmt-bad text
run_case -f nosuch text
run_case -e '"%_ad\n"' -C seventeen
run_case -e '' text

# --- -n and -s ---------------------------------------------------------------------------
run_case -n 10 text
run_case -n 0 text
run_case -n 1K big
run_case -n 1KiB -C big
run_case -n x text
run_case -n -1 text
run_case -s 5 text
run_case -s 5 -n 5 -C text
run_case -s 100 text
run_case -s 11 a.txt b.txt
run_case -s 12 a.txt b.txt
run_case -s 0x10 -C big
run_case -s 1M big
run_case -s x text
STDIN=$fix/text; run_case -s 3
STDIN=$fix/text; run_case -C

# --- files ------------------------------------------------------------------------------
run_case -C a.txt b.txt
run_case a.txt nosuch b.txt
run_case nosuch
run_case nosuch1 nosuch2
run_case dir
run_case -C a.txt dir b.txt
run_case -C seventeen seventeen
run_case -C -- -n
STDIN=$fix/dir; run_case
STDIN=$fix/dir; run_case -C

# --- colours ---------------------------------------------------------------------------
for e in \
  '"%_ad " 16/1 "%02x_L[red] " "\n"' \
  '16/1 "%02x_L[green:0x41] " "\n"' \
  '16/1 "%02x_L[!blue:0x41] " "\n"' \
  '4/1 "%02x_L[red:@1-2] " "\n"' \
  '1/2 "%04x_L[red:AB] " "\n"' \
  '16/1 "%_p_L[yellow,cyan:0101] " "\n"' \
  '16/1 "%02x_L[white] " "\n"' \
  '16/1 "%02x_L[lightgray] " "\n"' \
  '16/1 "%02x_L[nosuchcolor] " "\n"' \
  '"%_Ax_L[red]\n" 16/1 "%02x " "\n"' \
  '16/1 "%02x_L[red" "\n"' ; do
  run_case --color=always -e "$e" text
  run_case -e "$e" text
done
run_case --color=never -e '16/1 "%02x_L[red] " "\n"' text
run_case -L -e '16/1 "%02x_L[red] " "\n"' text
run_case --color=sometimes text
run_case -Lalways -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; run_case -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; ENVS=(TERM=xterm-256color); run_case -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; ENVS=(TERM=vt100); run_case -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; ENVS=(TERM=dumb); run_case -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; ENVS=(TERM=xterm-256color); run_case --color=never -e '16/1 "%02x_L[red] " "\n"' text
PTY=1; ENVS=(TERM=xterm-256color); run_case --color=auto -e '16/1 "%02x_L[red] " "\n"' text
for tc in tc-disable tc-disable-scheme tc-enable-scheme tc-term tc-other; do
  PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/$tc"); run_case -e '16/1 "%02x_L[red] " "\n"' text
  PTY=1; ENVS=(TERM=xterm-256color "HOME=$fix/$tc/.." "XDG_CONFIG_HOME=$fix/$tc"); run_case -C text
done

# --- the command line --------------------------------------------------------------------
run_case -Z
run_case --nosuch
run_case -e
run_case -n
run_case --format
run_case -h
run_case --help
run_case --help -Z
xfail_case "our version string, not util-linux's" -- -V
xfail_case "our version string, not util-linux's" -- --version

# --- files after failed ones: a stream left with no descriptor ---------------------------
run_case nosuch1 nosuch2 a.txt
run_case -C nosuch1 nosuch2 nosuch3 b.txt
run_case -C a.txt nosuch1 nosuch2 b.txt
run_case -s 5 nosuch1 a.txt
REDIR='<&-'; run_case nosuch1 nosuch2
REDIR='<&-'; run_case nosuch1 a.txt

# --- a standard input shared with the next command ------------------------------------------
# `exit` seeks it back over what was read and not used; `fclose` would not,
# and `_exit` -- `close_stdout` on a failed write -- does nothing at all.
SCRIPT='{ "$name" "$@"; cat; } < text | od -c'; run_case -n 5
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -n 5
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -n 4097 -e '"%x"'
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -n 5000 -e '4096/1 "%x"'
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -s 10 -n 1
SCRIPT='{ "$name" "$@" >/dev/full; cat; } < big | od -c | tail -3'; run_case -n 5
SCRIPT='{ "$name" "$@" >&-; cat; } < big | od -c | tail -3'; run_case -n 5
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -n 5 nosuch
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -n 5 a.txt
SCRIPT='cat big | { "$name" "$@"; cat; } | od -c | tail -3'; run_case -n 5
SCRIPT='{ "$name" "$@"; cat; } < big | od -c | tail -3'; run_case -e '"%"'

# --- a standard descriptor that cannot be written ------------------------------------------
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  REDIR=$redir; run_case -C text
  REDIR=$redir; run_case big
  REDIR=$redir; run_case nosuch
  REDIR=$redir; run_case --help
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
