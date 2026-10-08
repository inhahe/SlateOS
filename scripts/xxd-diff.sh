#!/usr/bin/env bash
# Differential test: our `xxd` against vim 9.1.0016's.
#
# The reference is Ubuntu's package `xxd` 2:9.1.0016-1ubuntu7.20, whose
# source is upstream's, unpatched.
#
# What is compared: stdout, stderr and the exit status of each case -- and,
# where `xxd` writes a file or leaves a shared standard input somewhere, what
# is in the file or what the next reader got, printed by the case's script.
#
#   * every dump kind -- hex, -p, -i, -b, -e -- over inputs that end mid-line
#     and mid-group, with -u, -d, -E, and their columns (-c) and groups (-g)
#     at the edges: zero, one, the 256 maximum and past it, a group wider than
#     the line, a little-endian group that is not a power of two;
#   * -l, -s (absolute, relative with `+`, from the end with `-`, past either
#     end) and -o (with `+` and `-`, and negative under -d), on files, on a
#     standard input that is a file somewhere in the middle, and on a pipe,
#     where a seek becomes a read and one from the end is refused;
#   * -a's squeezing of zero lines, at the end of the input and before data;
#   * -i's variable names: from -n in each spelling, from the file name, none
#     for standard input, a leading digit, punctuation, -C;
#   * colour: -R always/never/auto and nonsense, every dump kind, short last
#     lines (whose padding upstream computes separately for -e), and the
#     terminal default with and without NO_COLOR;
#   * -r: dumps made by the reference in each kind, and hand-made ones with
#     gaps, lines out of order, garbage, CRLF, odd digits, long lines and
#     binary-looking character columns; into a pipe (zeros forward, refused
#     backward), into a file (seeking, and patching rather than truncating),
#     with -s and -c; the kinds it cannot revert;
#   * standard input shared with the next command: the block `xxd` read is
#     gone after it, except where it exits on an error, which gives the rest
#     back; and `-r` rewinds it first;
#   * the option parser's corners (prefix matching, `--` folding, attached and
#     separate values, `--`), the usage, and refusals;
#   * missing, unreadable and directory files; closed and full descriptors.
#
# Cases that differ on purpose: -v and the usage name this build. Elsewhere
# the version string is one word on both sides before comparing.
set -u

DIFF_PROG='xxd'
DIFF_NEED='timeout python3 od'
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
    path = os.path.join(d, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)
put("empty", b"")
put("one", b"A")
put("fifteen", bytes(range(65, 80)))
put("sixteen", bytes(range(48, 64)))
put("seventeen", bytes(range(97, 114)))
put("text", b"Hello, world!\nThe quick brown fox\tjumps.\r\n\x00\x07\x08\x0b\x0c\x1b\x7f\x80\xff end\n")
put("allbytes", bytes(range(256)))
put("zeros", b"\x00" * 100)
put("zeros32", b"\x00" * 32)
put("zeros48", b"\x00" * 48)
put("zeros64", b"\x00" * 64)
put("zeromix", b"\x00" * 16 + b"A" * 16 + b"\x00" * 48 + b"B" + b"\x00" * 47 + b"\x00" * 32)
put("big", bytes((i * 7) & 0xff for i in range(5000)))
put("1st.bin", b"\x01\x02\x03")
put("a-b.c d", b"xyz")
put("sub/x.y", b"\x10\x20")
# Dumps to revert, written by hand.
put("gaps.xxd", b"00000000: 4142\n00000010: 4344\n")
put("back.xxd", b"00000010: 4142\n00000000: 4344\n")
put("back-trailer.xxd", b"00000010: 4142\n00000000: 4344\nTRAILER\n")
put("garbage.xxd", b"hello world\nxyz\n00000000: 41 42 zz 43\n00000004: 4445 x 4647\n")
put("crlf.xxd", b"00000000: 4142 4344  ABCD\r\n00000004: 45\r\n")
put("upper.xxd", b"00000000: 4A4B 4c4D  JKLM\n")
put("odd.xxd", b"00000000: 414\n00000002: 4\n")
put("long.xxd", b"00000000: " + b"41" * 20 + b"\n")
put("noaddr.xxd", b"4142434445\n")
put("ascii-hex.xxd", b"00000000: 4142                                     AB\n00000002: 43  CAFE\n")
put("plain.txt", b"41 42 43\n44\t45\n\n46\r\n4 7\n")
put("plain-garbage.txt", b"4142xx4344\nzz\n4546\n")
put("bits.xxd", b"00000000: 01000001 01000010  AB\n00000002: 0100001  x\n00000003: 010000110  C\n")
put("bits-ascii.xxd", b"00000000: 00110000                                       01010101\n")
put("bits-garbage.xxd", b"zz\n00000000: 0100 0001 2 01000010\n")
put("patch.bin", b"0123456789abcdefghij")
os.mkdir(os.path.join(d, "dir"))
PY
# Dumps the reference makes, to revert.
for kind in '' -p '-p -c 0' -b -u '-c 8' -a; do
  # shellcheck disable=SC2086  # the kind is words, on purpose
  "$gnu_real" $kind "$fix/text" >"$fix/text${kind// /}.dump" 2>/dev/null
done
"$gnu_real" "$fix/allbytes" >"$fix/allbytes.dump"
"$gnu_real" "$fix/big" >"$fix/big.dump"
"$gnu_real" -p "$fix/big" >"$fix/big.pdump"
"$gnu_real" -b "$fix/allbytes" >"$fix/allbytes.bdump"
"$gnu_real" -a "$fix/zeromix" >"$fix/zeromix.adump"

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
# STDIN: a file on standard input. REDIR: redirections after the command.
# PTY: standard output on a terminal. ENVS: the environment's extra entries.
# NAME: what the program is called. SCRIPT: a shell script run instead of the
# command, with `$name` the program and "$@" the case's arguments, for the
# cases that need a pipe, a file to look at afterwards, or a second reader.
# RAW: compare the version string too (see compare).
STDIN=/dev/null
REDIR=
PTY=
ENVS=()
NAME=xxd
SCRIPT=
RAW=
reset_knobs() { STDIN=/dev/null; REDIR=; PTY=; ENVS=(); NAME=xxd; SCRIPT=; RAW=; }

# Our binary and the reference under another name, for the usage's `pname`.
for side in ours gnu; do
  ln -sf "$bindir/$side/xxd" "$bindir/$side/hexview"
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
  # The usage quotes the version string, which names this build: one word
  # for both, so that every refusal is not a difference. RAW keeps it.
  if [ -z "$RAW" ]; then
    sed -i -e 's/xxd 2023-10-25 by Juergen Weigert et al[.]/VERSION/g' \
      -e 's/xxd from SlateOS coreutils 0[.]1[.]0/VERSION/g' "$DIFF_TMP/o.err" "$DIFF_TMP/g.err"
  fi
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
    printf 'BROKEN %s -- never reached xxd on one or both sides\n%s\n' "$LABEL" "$REPORT"
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
run_case sixteen
if ! grep -q '^00000000: 3031 3233' "$DIFF_TMP/g.out"; then
  echo "xxd-diff: the reference did not dump the fixture:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- the dump kinds ------------------------------------------------------------
for f in empty one fifteen sixteen seventeen text allbytes zeros big; do
  for o in '' -p -i -b -e -u -d -E '-e -u' '-b -d' '-p -u' '-i -u'; do
    # shellcheck disable=SC2086  # the options are words, on purpose
    run_case $o "$f"
  done
done

# --- columns and groups -----------------------------------------------------------
for c in 0 1 2 3 5 8 15 16 17 32 255 256 257 -1 0x10 010 1x x 4294967312; do
  run_case -c "$c" seventeen
  run_case -p -c "$c" seventeen
  run_case -i -c "$c" seventeen
  run_case -b -c "$c" seventeen
  run_case -e -c "$c" seventeen
done
run_case -c 300 -p big
run_case -c 1000 -i allbytes
run_case -c8 text
run_case -cols 8 text
run_case --cols 8 text
run_case -cols8 8 text
run_case --cols=8 text
run_case -c
run_case -capitalize -i -n abc text
for g in 0 1 2 3 4 5 8 16 17 -1 0x2; do
  run_case -g "$g" seventeen
  run_case -e -g "$g" seventeen
  run_case -b -g "$g" seventeen
  run_case -c 6 -e -g "$g" seventeen
done
run_case -g3 text
run_case -group 3 text
run_case --group 3 text
run_case -g
run_case -e -c 3 seventeen
run_case -e -c 6 allbytes
run_case -e -c 12 -g 8 allbytes

# --- -l, -s, -o, -d -------------------------------------------------------------
for l in 0 1 5 16 17 -1 100000 0x10; do
  run_case -l "$l" text
  run_case -p -l "$l" text
  run_case -i -l "$l" text
  run_case -b -l "$l" text
done
run_case -l5 text
run_case -len 5 text
run_case -l
for s in 0 5 16 1000 -5 -1000 +5 +0 +-5 --5 0x10 -0x10 ' -5' +; do
  run_case -s "$s" text
  run_case -d -s "$s" text
  SCRIPT='cat text | "$name" "$@"'; run_case -s "$s"
  STDIN=$fix/text; run_case -s "$s"
  SCRIPT='{ head -c 3 >/dev/null; "$name" "$@"; } < text'; run_case -s "$s"
done
run_case -s5 text
run_case -s+5 text
run_case -s-5 text
run_case -skip 5 text
run_case -seek 5 text
run_case -s
run_case -s 5 -l 3 -p text
run_case -s 5 -i text
run_case -s -5 -i text
SCRIPT='cat big | "$name" "$@"'; run_case -s 4100 -l 40
run_case -s 4100 -l 40 big
STDIN=$fix/dir; run_case -s 5
for o in 16 0x100 -16 +16 +-16 --16 0xffffffffffffffff 99999999999999999999 x; do
  run_case -o "$o" seventeen
  run_case -d -o "$o" seventeen
  run_case -o"$o" seventeen
  run_case -b -o "$o" seventeen
done
run_case -offset 16 seventeen
run_case -o
run_case -d -o -1 -c 1 seventeen
run_case -s 3 -o 0x10 text

# --- -a ----------------------------------------------------------------------------
for f in zeros zeros32 zeros48 zeros64 zeromix text empty; do
  run_case -a "$f"
  run_case -a -a "$f"
  run_case -a -c 8 "$f"
  run_case -a -b "$f"
  run_case -a -e "$f"
done
run_case -autoskip zeros64
run_case -a -l 50 zeros
STDIN=$fix/zeromix; run_case -a

# --- -i variable names -----------------------------------------------------------------
run_case -i text
run_case -i -C text
run_case -i -n foo text
run_case -i -nfoo text
run_case -i -name foo text
run_case -i --name foo text
run_case -i --name=foo text
run_case -i -namefoo bar text
run_case -i -n 9lives text
run_case -i -n 'a-b.c' text
run_case -i -n '' text
run_case -i -C -n camelCase text
run_case -i 1st.bin
run_case -i 'a-b.c d'
run_case -i sub/x.y
run_case -i -C sub/x.y
run_case -i ./1st.bin
run_case -i empty
run_case -i -n e empty
STDIN=$fix/text; run_case -i
STDIN=$fix/text; run_case -i -
STDIN=$fix/text; run_case -i -n x
run_case -i -u -C text
run_case -n

# --- colour -------------------------------------------------------------------------------
for f in one seventeen text allbytes zeros; do
  for o in '' -b -e -u -E -d '-e -c 6' '-e -c 5' '-e -g 2 -c 7' '-c 1' '-c 3 -g 2' '-a' '-p' '-i'; do
    # shellcheck disable=SC2086  # the options are words, on purpose
    run_case -R always $o "$f"
  done
done
run_case -R never text
run_case -R auto text
run_case -Ralways text
run_case -Rnever text
run_case -R alwaysly text
run_case -R sometimes text
run_case -R
run_case -R always -c 64 text
run_case --R always text
PTY=1; run_case text
PTY=1; run_case -b seventeen
PTY=1; run_case -R never text
PTY=1; run_case -R auto text
PTY=1; ENVS=(NO_COLOR=1); run_case text
PTY=1; ENVS=(NO_COLOR=); run_case text
PTY=1; ENVS=(NO_COLOR=1); run_case -R auto text
PTY=1; ENVS=(NO_COLOR=1); run_case -R always text
ENVS=(NO_COLOR=1); run_case -R always text
SCRIPT='"$name" "$@"; rc=$?; od -c out-pty; rm -f out-pty; exit $rc'; PTY=1; run_case text out-pty

# --- -r -------------------------------------------------------------------------------------
for d in text.dump text-p.dump text-p-c0.dump text-b.dump text-u.dump text-c8.dump text-a.dump \
         allbytes.dump big.dump big.pdump allbytes.bdump zeromix.adump \
         gaps.xxd back.xxd garbage.xxd crlf.xxd upper.xxd odd.xxd long.xxd noaddr.xxd \
         ascii-hex.xxd plain.txt plain-garbage.txt bits.xxd bits-ascii.xxd bits-garbage.xxd empty; do
  # A plain dump read as a hex one: its first line, sixty digits, is all
  # offset, and what is left of it shifted through a `long` is a place far
  # ahead -- so both sides write zeros towards it without end. Compared on
  # the first 64 bytes, after which `head` leaves and the pipe kills both.
  # Into a file the same offset is a seek, and the file sparse but huge.
  bounded=
  [ "$d" = text-p.dump ] && bounded=1
  for o in -r '-r -p' '-r -b' '-r -c 8' '-r -c 4' '-r -s 0x10' '-r -s -0x10' '-r -s +4'; do
    if [ -n "$bounded" ] && [ "${o#*-p}" = "$o" ]; then
      SCRIPT='{ "$name" "$@"; echo "rc=$?" >&2; } | head -c 64 | od -c'
    else
      SCRIPT='{ "$name" "$@"; echo "rc=$?" >&2; } | od -c'
    fi
    # shellcheck disable=SC2086  # the options are words, on purpose
    run_case $o "$d"
  done
  [ -n "$bounded" ] && continue
  SCRIPT='"$name" "$@" > out; rc=$?; od -c out; rm -f out; exit $rc'; run_case -r "$d"
  SCRIPT='cp patch.bin out; "$name" "$@" out; rc=$?; od -c out; rm -f out; exit $rc'; run_case -r "$d"
done
SCRIPT='cp patch.bin out; "$name" "$@" out; rc=$?; od -c out; rm -f out; exit $rc'; run_case -r -s 0x8 gaps.xxd
SCRIPT='"$name" "$@" > out; rc=$?; od -c out; rm -f out; exit $rc'; run_case -r -s -0x10 back.xxd
run_case -r -i text.dump
run_case -r -e text.dump
SCRIPT='"$name" "$@" new; rc=$?; ls new 2>&1; rm -f new; exit $rc'; run_case -r -e text.dump
STDIN=$fix/text.dump; SCRIPT='"$name" "$@" | od -c'; run_case -r
STDIN=$fix/text.dump; SCRIPT='"$name" "$@" - - | od -c'; run_case -r
SCRIPT='cat text.dump | "$name" "$@" | od -c'; run_case -r
SCRIPT='cat text.dump | "$name" "$@" | od -c'; run_case -r -s 5
run_case -r dir
run_case -r nosuch
run_case -revert text.dump
run_case --revert text.dump

# --- a standard input shared with the next command ----------------------------------------------
SCRIPT='{ "$name" "$@" > /dev/null; cat; } < text | od -c'; run_case -l 5
SCRIPT='{ "$name" "$@" > /dev/null; cat; } < big | od -c | tail -3'; run_case -l 5
SCRIPT='{ "$name" "$@" > /dev/null; cat; } < big | od -c | tail -3'; run_case -p -l 4097
SCRIPT='{ "$name" "$@" > /dev/null; cat; } < big | od -c | tail -3'; run_case -s 10 -l 1
SCRIPT='{ read -r line; "$name" "$@"; } < text.dump | od -c'; run_case -r
SCRIPT='{ read -r line; "$name" "$@" | od -c; cat; } < text.dump'; run_case -r -l 5
SCRIPT='{ "$name" "$@" | cat; cat; } < back-trailer.xxd'; run_case -r
SCRIPT='{ "$name" "$@" >&-; cat; } < big | od -c | tail -3'; run_case -l 5

# --- the command line -------------------------------------------------------------------------
run_case -Z
run_case --nosuch
run_case -h
run_case --help
run_case -
STDIN=$fix/seventeen; run_case -
STDIN=$fix/seventeen; run_case - -
run_case -- seventeen
run_case --- seventeen
run_case -- -c
run_case seventeen -c 8
run_case seventeen - extra
run_case -abc zeros
run_case -bits seventeen
run_case -upper seventeen
run_case -ps seventeen
run_case -include seventeen
run_case -decimal seventeen
run_case -EBCDIC seventeen
run_case -i -p seventeen
run_case -p -i seventeen
run_case -b -e seventeen
run_case -e -b seventeen
NAME=hexview; run_case -Z
NAME=hexview; run_case seventeen
RAW=1; xfail_case "our version string, not vim's" -- -v
RAW=1; xfail_case "our version string, not vim's" -- -version
RAW=1; xfail_case "our version string, not vim's" -- --version
RAW=1; xfail_case "the usage quotes our version string" -- -h
run_case -v
run_case -version

# --- files and descriptors -------------------------------------------------------------------------
run_case nosuch
run_case dir
run_case -p dir
run_case -i dir
run_case seventeen dir
run_case seventeen nosuchdir/out
run_case nosuch nosuchdir/out
SCRIPT='"$name" "$@" out; rc=$?; od -c out; rm -f out; exit $rc'; run_case seventeen
SCRIPT='cp patch.bin out; "$name" "$@" out; rc=$?; od -c out; rm -f out; exit $rc'; run_case -p one
STDIN=$fix/dir; run_case
STDIN=$fix/dir; run_case -r
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full' '<&-'; do
  REDIR=$redir; run_case seventeen
  REDIR=$redir; run_case big
  REDIR=$redir; run_case -p big
  REDIR=$redir; run_case -i empty
  REDIR=$redir; run_case empty
  REDIR=$redir; run_case nosuch
  REDIR=$redir; run_case -r text.dump
  REDIR=$redir; run_case -r
  REDIR=$redir; run_case
  REDIR=$redir; run_case -Z
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
