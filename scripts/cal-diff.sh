#!/usr/bin/env bash
# Differential test: our `cal` against util-linux `cal`.
#
# ## Why `cal` is worth a harness even though nothing depends on it
#
# It is pure computation. Every byte of output is a function of the arguments
# and of a calendar reform that happened in 1752, so unlike `date` or `uptime`
# there is nothing volatile to work around and a disagreement is always a defect
# in one side. That makes it the cheapest possible place to find out whether an
# implementation was written from the spec or from memory.
#
# And the spec has teeth. September 1752 is missing eleven days in the British
# reform, 1900 is not a leap year while 2000 is, ISO week numbers disagree with
# US ones at both ends of a year, and `-j` renumbers every cell. An
# implementation that gets an ordinary month right can be wrong about all five.
#
# ## The reference
#
# util-linux 2.39.3's `cal`, built from the release by util-linux-ref.sh with
# configure's defaults -- libtinfo included, as Ubuntu builds the util-linux
# programs it does ship. Ubuntu ships no util-linux `cal`: `/usr/bin/cal` is
# BSD's, from the `ncal` package. Until 2026-10-08 this compared against a
# `/usr/local/bin/cal` built by hand without libtinfo, which never coloured a
# terminal on its own, so no case here could see that decision.
#
# Its option set is read from the program rather than assumed: `--reform`,
# `--iso`, `-1`/`-3`, `--months`, `--span`, `--vertical`, `--columns` and
# `--color` are util-linux's, and are not the same list BSD `cal` carries.
#
# ## Why `od -An -c`
#
# `cal` pads every line to a fixed width with TRAILING SPACES, and the width
# depends on `-j` and on the number of months shown. A comparison that stripped
# them would call a three-column layout equal to a one-column one for any month
# whose text happened to match. The whole output is a few hundred bytes.
#
# ## The one case that is not a pure function of the arguments
#
# Bare `cal` shows the current month. It changes once a month rather than once a
# second, so unlike `date` it is not a flake source worth excluding -- the two
# sides would have to straddle midnight on the first of a month. It is included,
# and if it ever fails alone that is the first thing to suspect.
set -u

DIFF_PROG='cal'
# Every invocation is bounded, both sides. `cal 12 9999` and `-n 1000` ask for a
# lot of output from a loop over months, which is the shape that runs away.
DIFF_NEED='timeout python3'
# util-linux's own `cal`, built from the release -- Ubuntu's /usr/bin/cal is
# BSD's. See util-linux-ref.sh, which also says why the build that was here
# before it could not tell a colour terminal from any other.
# shellcheck source=util-linux-ref.sh
. "$(dirname "$0")/util-linux-ref.sh"
DIFF_REF=$UL_REF_CAL
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# ---------------------------------------------------------------------------
# Colour fixtures: terminal-colors.d trees, each under a directory that is
# then XDG_CONFIG_HOME. Whether `cal` colours at all is util-linux's
# lib/colors.c -- standard output a terminal, TERM's terminfo entry having
# colours, these files -- and with what is a scheme file's sequences for the
# five names cal.c asks for. Both sides read the same reference terminfo.
# ---------------------------------------------------------------------------
fix=$DIFF_TMP/fix
mkdir -p "$fix/none"
python3 - "$fix" <<'PY'
import os, sys
d = sys.argv[1]
trees = {
    # every name cal asks for, as escape sequences
    "scheme-all": {"cal.scheme": b"header 1;34\nworkday 32\nweekend 31\ntoday 4\nweeknumber 35\n"},
    # colour names -- `white` and `lightgray` are not found by upstream's
    # bsearch of its unsorted table, and are written out as text
    "scheme-names": {"cal.scheme": b"today red\nheader bold\nweekend lightgray\nworkday white\nweeknumber lightgray,\n"},
    # backslash escapes, a comment, blank lines and indentation
    "scheme-escapes": {"cal.scheme": b"# comment\n\n   today 1;\\_5\nheader \\e[7m\\x\nweekend 3\\#1\n"},
    # a disable file counts only once a scheme exists somewhere
    "disable": {"cal.disable": b"", "scheme": b""},
    "disable-noscheme": {"cal.disable": b""},
    "enable-beats-disable": {"disable": b"", "cal.enable": b"", "scheme": b""},
    # the best match wins: a terminal-specific file over a plain one
    "term-specific": {"cal.scheme": b"today 33\n", "cal@xterm-256color.scheme": b"today 32\n"},
    "other-util": {"dmesg.scheme": b"today 32\n"},
    "global": {"scheme": b"weekend 33\nheader 4\n"},
    # under HOME, for when XDG_CONFIG_HOME is unset
    "home/.config": {"cal.scheme": b"today 36\nworkday 32\n"},
}
for tree, files in trees.items():
    base = os.path.join(d, tree, "terminal-colors.d")
    os.makedirs(base)
    for name, data in files.items():
        with open(os.path.join(base, name), "wb") as f:
            f.write(data)
PY

# ptyrun.py: COMMAND with standard output on a terminal; what it wrote comes
# out on our standard output.
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

# --- knobs: PTY puts standard output on a terminal; ENVS adds to (and, coming
# later, overrides) the pinned environment; UNSET names variables the case
# runs without, pinned ones included; CWD is the directory it runs in. Reset
# after every case.
PTY=
ENVS=()
UNSET=()
CWD=
reset_knobs() { PTY=; ENVS=(); UNSET=(); CWD=; }

run_side() {
  local side=$1; shift
  # TERM and the absence of a tty both matter: `cal` highlights today, and it
  # must not do so when its output is a pipe. Pinned so neither side can be
  # colouring on the strength of the harness's own environment -- HOME and
  # XDG_CONFIG_HOME too, so no terminal-colors.d of the host's is read.
  local -a pinned=(TZ=UTC LC_ALL=C.UTF-8 TERM=dumb "HOME=$fix/none" "XDG_CONFIG_HOME=$fix/none")
  local -a unset=() keep=()
  local p u skip
  for u in "${UNSET[@]}"; do unset+=(-u "$u"); done
  for p in "${pinned[@]}"; do
    skip=
    for u in "${UNSET[@]}"; do [ "${p%%=*}" = "$u" ] && skip=1; done
    [ -n "$skip" ] || keep+=("$p")
  done
  local -a cmd=(timeout -k 2 15 env "${unset[@]}" "${keep[@]}" "${ENVS[@]}"
    PATH="$bindir/$side")
  if [ -n "$CWD" ]; then
    # /bin/sh by its path: PATH is already only the side's binaries.
    # shellcheck disable=SC2016  # expanded by that shell, not this one
    cmd+=(/bin/sh -c 'cd "$1" && shift && exec cal "$@"' sh "$CWD" "$@")
  else
    cmd+=(cal "$@")
  fi
  if [ -n "$PTY" ]; then
    diff_run python3 "$ptyrun" "${cmd[@]}"
  else
    diff_run "${cmd[@]}"
  fi
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# The case's label: its arguments, and the knobs, a fixture tree by its name.
label_of() {
  local l="cal $*"
  [ -n "$PTY" ] && l="$l [pty]"
  [ "${#ENVS[@]}" -gt 0 ] && l="$l [${ENVS[*]##*/}]"
  [ "${#UNSET[@]}" -gt 0 ] && l="$l [-u ${UNSET[*]}]"
  [ -n "$CWD" ] && l="$l [in ${CWD##*/}]"
  printf '%s' "$l"
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"; reset_knobs
  report "$label"
}

xfail_case() {
  local why=$1; shift
  compare "$@"; reset_knobs
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS cal %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail cal %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the current month, the one case that is not purely argument-driven --------
run_case

# --- ordinary months, to establish the layout ----------------------------------
run_case 1 2021
run_case 2 2021
run_case 6 2021
run_case 12 2021

# --- THE GREGORIAN REFORM, which is the whole reason this program is hard -------
run_case 9 1752
run_case 1752
run_case 8 1752
run_case 10 1752
run_case 2 1752
run_case 1751
run_case 1753

# --- leap years, including the century rule --------------------------------------
run_case 2 2000
run_case 2 1900
run_case 2 2024
run_case 2 2023
run_case 2 1600
run_case 2 2100

# --- the ends of the range ---------------------------------------------------------
run_case 1 1
run_case 12 9999
run_case 1 1000
run_case 9999

# --- first day of the week ----------------------------------------------------------
run_case -s 6 2021
run_case -m 6 2021
run_case --sunday 6 2021
run_case --monday 6 2021
run_case -s 9 1752
run_case -m 9 1752

# --- day-of-year numbering ------------------------------------------------------------
run_case -j 1 2021
run_case -j 12 2021
run_case -j 9 1752
run_case -j 2 2000
run_case --julian 6 2021
run_case -j 2021

# --- one, three and N months ------------------------------------------------------------
run_case -1 6 2021
run_case -3 6 2021
run_case -3 1 2021
run_case -3 12 2021
run_case -3 9 1752
run_case --three 6 2021
run_case -n 5 6 2021
run_case --months 5 6 2021
run_case -n 1 6 2021
run_case -n 0 6 2021
run_case -S -3 6 2021
run_case --span -n 4 6 2021

# --- whole years --------------------------------------------------------------------------
run_case -y 2021
run_case -y
run_case --year 2021
run_case -Y 2021
run_case --twelve 2021
run_case -y -j 2021
run_case -y -m 2021

# --- week numbers ---------------------------------------------------------------------------
run_case -w 6 2021
run_case --week 6 2021
run_case -w 1 2021
run_case -w 12 2021
run_case -w -m 1 2021
run_case --week=4 6 2021
run_case -w 9 1752

# --- the reform knob -------------------------------------------------------------------------
run_case --reform=1752 9 1752
run_case --reform=gregorian 9 1752
run_case --reform=julian 9 1752
run_case --reform=iso 9 1752
run_case --iso 9 1752
run_case --iso 2 1900
run_case --reform=nosuch 9 1752
run_case --reform

# --- layout knobs ------------------------------------------------------------------------------
run_case -v 6 2021
run_case --vertical 6 2021
run_case -v -j 6 2021
run_case -c 40 -3 6 2021
run_case --columns=40 -3 6 2021
run_case -c 0 6 2021
run_case --color=never 6 2021
run_case --color=always 6 2021
run_case --color=auto 6 2021
run_case --color=nosuch 6 2021
run_case --color==never 6 2021
run_case --color==always 6 2021
run_case --color===never 6 2021
run_case --color==x 6 2021
run_case --color= 6 2021
run_case --color=ALWAYS 6 2021
run_case --color 6 2021

# --- colour on a terminal: lib/colors.c's decision, and a scheme's sequences -----
# Each tree under each layout that colours differently: the plain month (header,
# workdays, weekends), Monday first (the workdays move), day-of-year, vertical
# (no header or workday colour at all), week numbers with one asked for, three
# months, a year, and the current month, where today is marked.
for tc in none scheme-all scheme-names scheme-escapes disable disable-noscheme \
          enable-beats-disable term-specific other-util global; do
  for args in '6 2021' '-m 6 2021' '-j 6 2021' '-v 6 2021' '--week=25 6 2021' \
              '-v --week=25 6 2021' '-3 6 2021' '-y 2021' '15 6 2021' ''; do
    PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/$tc")
    # shellcheck disable=SC2086  # the arguments are words, on purpose
    run_case $args
  done
done
# The scheme through HOME when XDG_CONFIG_HOME is unset; and each set but
# empty, which upstream reads as the absolute /terminal-colors.d and
# /.config/terminal-colors.d, not as paths relative to where cal runs.
PTY=1; UNSET=(XDG_CONFIG_HOME); ENVS=(TERM=xterm-256color "HOME=$fix/home"); run_case 6 2021
PTY=1; UNSET=(XDG_CONFIG_HOME HOME); ENVS=(TERM=xterm-256color); run_case 6 2021
PTY=1; ENVS=(TERM=xterm-256color XDG_CONFIG_HOME=); run_case 6 2021
PTY=1; UNSET=(XDG_CONFIG_HOME); ENVS=(TERM=xterm-256color HOME=); run_case 6 2021
# ... run from a directory that has a terminal-colors.d (and a .config/ with
# one), which a relative reading would have found.
PTY=1; CWD=$fix/scheme-all; ENVS=(TERM=xterm-256color XDG_CONFIG_HOME=); run_case 6 2021
PTY=1; CWD=$fix/home; UNSET=(XDG_CONFIG_HOME); ENVS=(TERM=xterm-256color HOME=); run_case 6 2021
# Terminals without colours, and none at all.
for term in dumb vt100 no-such-terminal ''; do
  PTY=1; ENVS=("TERM=$term" "XDG_CONFIG_HOME=$fix/scheme-all"); run_case 15 6 2021
  PTY=1; ENVS=("TERM=$term" "XDG_CONFIG_HOME=$fix/scheme-all"); run_case --color=always 15 6 2021
done
# The mode beats the files and the terminal, in both directions.
PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/scheme-all"); run_case --color=never 15 6 2021
PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/disable"); run_case --color=auto 15 6 2021
PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/disable"); run_case --color=always 15 6 2021
PTY=1; ENVS=(TERM=xterm-256color "XDG_CONFIG_HOME=$fix/scheme-all"); run_case --color==never 15 6 2021
# Into a pipe, only --color=always colours -- with the scheme's sequences.
ENVS=("XDG_CONFIG_HOME=$fix/scheme-all"); run_case --color=always 15 6 2021
ENVS=("XDG_CONFIG_HOME=$fix/scheme-all"); run_case --color=always -v --week=25 15 6 2021
ENVS=("XDG_CONFIG_HOME=$fix/scheme-all"); run_case 15 6 2021

# --- the day/month/year form and names ------------------------------------------------------------
run_case 15 9 1752
run_case 1 1 2021
run_case 31 12 2021
run_case 32 12 2021

# --- refusals -------------------------------------------------------------------------------------
run_case 0 2021
run_case 13 2021
run_case -1 2021
run_case 6 0
run_case 6 10000
run_case notamonth 2021
run_case 6 notayear
run_case 1 2 3 4
run_case -Q
run_case --nosuchoption
run_case -n
run_case -c

# --- long-option abbreviation -------------------------------------------------------------------------
run_case --mon 6 2021
run_case --sund 6 2021
run_case --jul 6 2021
run_case --thr 6 2021
run_case --ye 2021
run_case --ver 6 2021
run_case --col=40 -3 6 2021

# --- the two whose text is ours ---------------------------------------------------------------------------
# NOT an xfail: measured, and the help text matches util-linux's exactly.
# It was written here as an xfail on the assumption every other harness's help
# case holds -- and the XPASS reported that the assumption was wrong rather than
# letting it stand. An exemption that has stopped being true is worth more as a
# noisy failure than as a quiet allowance.
run_case -h
run_case --help
xfail_case "our version string, not util-linux's" -V
xfail_case "our version string, not util-linux's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
