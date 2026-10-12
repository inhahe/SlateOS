#!/usr/bin/env bash
# Differential test: journald's broadcast of an `emerg` message -- the
# `journalfwd` crate, which `syslogd` and `systemd-cat`'s helper call -- against
# systemd 255's own `wall()`.
#
# ## The reference, and why it is a library call
#
# The behaviour being ported is systemd-journald's `ForwardToWall`: on by
# default, it writes any message at `emerg` to the terminal of every logged-in
# user. Measuring it against the running journald would mean sending an
# `emerg` message through WSL's journal -- which broadcasts it to whatever
# terminals the operator has open. So the reference is the function journald
# calls, `wall()` in `libsystemd-shared-255.so`, which exports it: a few lines
# of C call it the way `server_forward_wall` does (`username`
# `systemd-journald`, no origin tty) and print what it returned.
#
# ## The terminals, and why nothing escapes the namespace
#
# Each side runs in private user, mount and UTS namespaces
# (`scripts/journalfwd-walltest.py`), under a tmpfs mounted over /run: the
# system's utmp and logind's session list are out of sight, and the only utmp
# is the one the helper writes, naming pseudo-terminals it holds itself. What
# each terminal receives is the comparison -- byte for byte, with only the
# banner's clock time masked, since the two sides run a moment apart.
#
# ## What is compared
#
# For `wall()`: which utmp entries are written to and which are skipped, the
# banner, the host name (including none at all), the origin tty, the date and
# the zone abbreviation under several `TZ`s (including one too long to print),
# and the returned status. For `server_forward_wall`, which journald does not
# export: our `forward_wall` against the reference's `wall()` given the line
# journald builds -- `IDENT[PID]: MESSAGE`, the writer's command name when
# there is no identifier -- so that is the half held to the source rather than
# to a binary.
set -u

DIFF_PROG='journalfwd'
DIFF_PKG='journalfwd'
DIFF_EXAMPLES='wall'
DIFF_NO_REF=1
# An example, with no counterpart in /usr/bin to put beside it on PATH.
DIFF_NO_BINDIR=1
DIFF_NEED='gcc unshare python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

shared=/usr/lib/x86_64-linux-gnu/systemd
lib=
for l in "$shared"/libsystemd-shared-*.so; do
  [ -e "$l" ] && lib=$l
done
if [ -z "$lib" ]; then
  echo "journalfwd-diff: no libsystemd-shared here, so no reference; skipping"
  exit 0
fi

ref=$DIFF_TMP/wallref
cat > "$DIFF_TMP/wallref.c" <<'EOF'
#include <stdbool.h>
#include <stdio.h>
int wall(const char *message, const char *username, const char *origin_tty,
         bool (*match_tty)(const char *tty, bool is_local, void *userdata),
         void *userdata);
int main(int argc, char **argv) {
        if (argc < 3)
                return 2;
        printf("%d\n", wall(argv[1], argv[2], argc > 3 ? argv[3] : NULL, NULL, NULL));
        return 0;
}
EOF
gcc -o "$ref" "$DIFF_TMP/wallref.c" "$lib" -Wl,-rpath,"$shared" || exit 1
helper=$root/scripts/journalfwd-walltest.py

pass=0; fail=0

# side SHAPE HOST -- PROGRAM ARGS...: what one side's terminals received.
side() {
  local shape=$1 host=$2; shift 3
  unshare -rmu env ${ENVV[@]+"${ENVV[@]}"} python3 "$helper" "$shape" "$host" "$@" 2>&1
}

# check LABEL SHAPE HOST OURS-ARGS... -- REF-ARGS...
check() {
  local label=$1 shape=$2 host=$3; shift 3
  local -a o_args=() g_args=()
  while [ $# -gt 0 ] && [ "$1" != -- ]; do o_args+=("$1"); shift; done
  shift
  g_args=("$@")
  local o g
  o=$(side "$shape" "$host" -- "$OURS" "${o_args[@]}")
  g=$(side "$shape" "$host" -- "$ref" "${g_args[@]}")
  if [ "$o" = "$g" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n--- ours\n%s\n--- systemd\n%s\n' "$label" "$o" "$g"
  fi
  return 0
}

# wall(): the same arguments both sides.
w() {
  local label=$1 shape=$2 host=$3; shift 3
  check "$label" "$shape" "$host" wall "$@" -- "$@"
}

ENVV=()
for shape in basic mixed notty missing empty; do
  w "wall to a $shape utmp" "$shape" - "an emergency" systemd-journald
done
w "a message with tabs, quotes and UTF-8" basic - $'tab\there "q" \'s\' caf\xc3\xa9 %s %n' systemd-journald
w "a message with a newline in it" basic - $'two\nlines' systemd-journald
w "an empty message" basic - "" systemd-journald
w "an origin tty" basic - "msg" systemd-journald pts/7
w "another user name" basic - "msg" root
w "a long host name" basic "a-rather-long-host-name.example.com" "msg" systemd-journald
w "no host name at all" basic "" "msg" systemd-journald
for tz in UTC America/New_York :Europe/Berlin EST5EDT '<+0330>-3:30' Asia/Kolkata \
          ABCDEFGHIJKLMN-3 Etc/GMT+5 Nowhere/Atlantis ''; do
  ENVV=(TZ="$tz")
  w "TZ=$tz" basic - "msg" systemd-journald
done
ENVV=()

# server_forward_wall: ours as journald calls it, against the line journald
# builds handed to systemd's wall().
f() {
  local label=$1 pri=$2 ident=$3 pid=$4 msg=$5 line=$6
  if [ "$line" = NONE ]; then
    # Not broadcast: nothing may reach a terminal, and wall() is never called.
    local o quiet
    o=$(side basic - -- "$OURS" forward "$pri" "$ident" "$pid" "$msg")
    quiet=$(printf '%s\n' "$o" | grep -c "^pty.: b''$")
    if [ "$quiet" = 3 ]; then
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
    else
      fail=$((fail+1))
      printf 'DIFF %s (should not be broadcast)\n%s\n' "$label" "$o"
    fi
    return 0
  fi
  # Ours prints nothing on stdout; the reference prints wall()'s status,
  # which journald only logs -- so its stdout line is dropped.
  local o g
  o=$(side basic - -- "$OURS" forward "$pri" "$ident" "$pid" "$msg")
  g=$(side basic - -- "$ref" "$line" systemd-journald | sed "s/^stdout: b'0\\\\n'\$/stdout: b''/")
  if [ "$o" = "$g" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n--- ours\n%s\n--- systemd\n%s\n' "$label" "$o" "$g"
  fi
  return 0
}

f "emerg with an identifier and a pid" 0 tag 42 "down" "tag[42]: down"
f "emerg in facility user" 8 tag 42 "down" "tag[42]: down"
f "emerg with no pid" 0 tag - "down" "tag: down"
f "emerg with neither" 0 - - "down" "down"
f "alert is not broadcast" 1 tag 42 "down" NONE
f "debug is not broadcast" 7 tag 42 "down" NONE
# No identifier: the writer's command name, as /proc/PID/comm has it --
# escaped as journald's cellescape escapes it.
sleep 30 & sl=$!
f "emerg naming the writer by its command" 0 - "$sl" "down" "sleep[$sl]: down"
cp "$(command -v sleep)" "$DIFF_TMP/we'ird"
"$DIFF_TMP/we'ird" 30 & wd=$!
f "a command name cellescape must escape" 0 - "$wd" "down" "we\\'ird[$wd]: down"
kill "$sl" "$wd" 2>/dev/null
f "a pid that is gone" 0 - 2147483646 "down" "[2147483646]: down"

printf '\n%d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
