#!/usr/bin/env bash
# Differential test: systemd 255's systemd-cat, ours against Ubuntu's.
#
# ## What is compared
#
# Every case is a line of shell that runs `systemd-cat` (found on PATH, so
# `argv[0]` is the bare name on both sides) and is compared on its standard
# output, its standard error and its exit status -- and, for the cases that
# send anything to the journal, on the records that arrive there:
#
#   PRIORITY  the name journalctl shows  which process  _LINE_BREAK  _TRANSPORT  MESSAGE
#
# "The name journalctl shows" is SYSLOG_IDENTIFIER, else _COMM, else
# `unknown`; ours is the record's `service`. "Which process" is not the number
# but its place among the case's writers -- pid1 for the first that wrote,
# pid2 for the next -- so a line from a process the command started is told
# apart from the command's own on both sides without the numbers having to
# agree. A record of systemd-cat's own (`_TRANSPORT=journal`) also carries
# CODE_FILE, CODE_LINE, CODE_FUNC and ERRNO.
#
# ## Keeping ours off the host's journal
#
# Ours writes the SlateOS journal, `/var/log/syslog.jsonl`. Each of our runs
# happens in a user, mount and PID namespace of its own (`unshare`), with a
# scratch directory bound over `/var/log`, so the host's logs are neither
# written nor read. The namespace's first process waits for every other one in
# it to finish before it exits: the helpers that file a stream outlive the
# command, as journald does, and the journal is read only once they are gone.
#
# ## Reading the reference's records back
#
# The reference's go to the host's journald. Before each case the journal's
# cursor is noted; after it, `journalctl --after-cursor` is read -- this user's
# records, of the transports the case can produce -- until it holds at least as
# many records as ours did and has stopped growing. journald files a stream
# while the case is still being compared, so a fixed sleep either wastes time
# or loses the tail.
#
# ## What is deliberately not compared
#
#   * `--version` names this build (an xfail).
#   * A record's numbers: _PID, _UID (ours runs as root in its namespace), the
#     time.
#   * The name of a process that wrote a line and was gone before journald
#     looked: journald names it `unknown`, ours by what it ran, when it is the
#     command. Cases whose command could finish first sleep a moment at the
#     end, so that both sides can name it.
#   * `SYSTEMD_LOG_TARGET=syslog`, whose message reaches the host's journald
#     from both sides; only what is printed is compared.
set -u

DIFF_PROG='systemd-cat'
DIFF_NEED='timeout python3 unshare journalctl script stat'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; xfail=0

work=$DIFF_TMP/work
varlog=$DIFF_TMP/varlog
mkdir -p "$work" "$varlog"
uid=$(id -u)

# Fixtures.
printf 'not a program\n' > "$work/nonexec"
chmod 644 "$work/nonexec"
python3 -c 'import sys; sys.stdout.buffer.write(b"L" * 50000)' > "$work/big"
seq 1 1000 > "$work/many"

# Inside the namespace: our /var/log, the case, then the helpers' end.
inns=$DIFF_TMP/inns.sh
# Ours forwards an `emerg` line to every logged-in user's terminal, as
# journald does (`userspace/journalfwd`): a private /run puts the system's
# utmp out of its reach, so no case of ours can broadcast. The reference
# cannot be fenced in that way -- it is WSL's own journald -- so no case here
# may file a line at `emerg`; scripts/journalfwd-diff.sh measures that
# instead, where both sides are fenced in.
cat > "$inns" <<'EOF'
mount --bind "$1" /var/log || exit 99
mount -t tmpfs tmpfs /run || exit 99
cd "$2" || exit 99
bash -c "$3"
rc=$?
# kill -0 -1 from a PID namespace's first process asks whether any other is
# left in it.
while kill -s 0 -- -1 2>/dev/null; do sleep 0.01; done
exit "$rc"
EOF

# The reference's runs go through the same shape of wrapper as ours -- a
# shell that runs the case as its child and exits with its status -- so that
# what a wrapping shell prints of its own (bash's "Terminated" for a child a
# signal killed) is printed on both sides or on neither.
plain=$DIFF_TMP/plain.sh
cat > "$plain" <<'EOF'
cd "$1" || exit 99
bash -c "$2"
rc=$?
exit "$rc"
EOF

# Records, one normalised line each. $1 = ours|gnu, $2 = the records, $3 =
# "sorted" for a case whose two streams may interleave either way.
norm=$DIFF_TMP/norm.py
cat > "$norm" <<'EOF'
import json, sys
side, path = sys.argv[1], sys.argv[2]
LEVELS = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"]
def as_bytes(v):
    if v is None:
        return None
    if isinstance(v, list):
        return bytes(v)
    return str(v).encode("utf-8", "surrogateescape")
pids = {}
def which(p):
    if p is None:
        return "pid?"
    pids.setdefault(p, "pid%d" % (len(pids) + 1))
    return pids[p]
rows = []
try:
    data = open(path, "rb").read()
except FileNotFoundError:
    data = b""
for line in data.splitlines():
    if not line.strip():
        continue
    r = json.loads(line)
    if side == "gnu":
        prio = int(r.get("PRIORITY", -1))
        name = as_bytes(r.get("SYSLOG_IDENTIFIER")) or as_bytes(r.get("_COMM")) or b"unknown"
        pid = r.get("_PID")
        msg = as_bytes(r.get("MESSAGE"))
    else:
        prio = LEVELS.index(r["level"]) if r.get("level") in LEVELS else -1
        name = as_bytes(r.get("service")) or b"unknown"
        pid = None if r.get("pid") is None else str(r.get("pid"))
        msg = as_bytes(r.get("msg"))
    transport = r.get("_TRANSPORT", "")
    code = ""
    if transport == "journal":
        code = " %s:%s:%s:%s" % (r.get("CODE_FILE"), r.get("CODE_LINE"), r.get("CODE_FUNC"), r.get("ERRNO"))
    rows.append("%d %r %s %s %s %r%s" % (prio, name, which(pid), r.get("_LINE_BREAK") or "-", transport, msg, code))
if len(sys.argv) > 3 and sys.argv[3] == "sorted":
    rows.sort()
for row in rows:
    print(row)
EOF

# What each side printed, with the parts that differ by nature taken out: the
# directory each binary is reached through, a thread id, a clock.
normalize() {
  sed -E \
    -e "s#$bindir/(ours|gnu)/#BIN/#g" \
    -e 's/\([0-9]+\) /(TID) /g' \
    -e 's/[A-Z][a-z]{2} [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2} [A-Za-z0-9+-]+ /<TIME> /g'
}

# run_ours SNIPPET: in the namespace; out, err and rc into the files named.
run_ours() {
  rm -f "$varlog/syslog.jsonl"
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 60 \
    unshare --user --map-root-user --mount --pid --fork --mount-proc \
    bash "$inns" "$varlog" "$work" "$1" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"
}

run_gnu() {
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 60 \
    bash "$plain" "$work" "$1" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"
}

# gnu_records CURSOR WANT TRANSPORTS ORDER: the reference's records since
# CURSOR, waited for until there are at least WANT and no more are coming.
gnu_records() {
  local cursor=$1 want=$2 transports=$3 order=$4 n prev=-1 i match=()
  local t
  for t in $transports; do match+=("_TRANSPORT=$t"); done
  for i in $(seq 1 100); do
    journalctl --after-cursor="$cursor" --all -o json --no-pager _UID="$uid" "${match[@]}" \
      >"$DIFF_TMP/g.json" 2>/dev/null
    n=$(wc -l <"$DIFF_TMP/g.json")
    if [ "$n" -ge "$want" ] && [ "$n" = "$prev" ]; then
      break
    fi
    prev=$n
    [ "$i" = 100 ] || sleep 0.1
  done
  python3 "$norm" gnu "$DIFF_TMP/g.json" "$order"
}

cursor_now() {
  journalctl -n 0 --show-cursor --no-pager 2>/dev/null | sed -n 's/^-- cursor: //p'
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# check NAME KIND SNIPPET [TRANSPORTS]
#   KIND: cli  -- what is printed and the status only
#         rec  -- and the records, in order
#         recu -- and the records, in either order
check() {
  local name=$1 kind=$2 snippet=$3 transports=${4:-stdout}
  local o_rc g_rc o_rec='' g_rec='' cursor want order=keep
  [ "$kind" = recu ] && order=sorted
  run_ours "$snippet"; o_rc=$?
  if [ "$kind" != cli ]; then
    o_rec=$(python3 "$norm" ours "$varlog/syslog.jsonl" "$order")
    cursor=$(cursor_now)
  fi
  run_gnu "$snippet"; g_rc=$?
  if [ "$kind" != cli ]; then
    want=$(printf '%s' "$o_rec" | grep -c . || true)
    g_rec=$(gnu_records "$cursor" "$want" "$transports" "$order")
  fi
  local o_out g_out o_err g_err
  o_out=$(normalize <"$DIFF_TMP/o.out" | od -An -c)
  g_out=$(normalize <"$DIFF_TMP/g.out" | od -An -c)
  o_err=$(normalize <"$DIFF_TMP/o.err" | od -An -c)
  g_err=$(normalize <"$DIFF_TMP/g.err" | od -An -c)
  REPORT=$(printf '  ours rc=%s  gnu rc=%s\n  ours out:%s\n  gnu  out:%s\n  ours err:%s\n  gnu  err:%s\n  ours records:\n%s\n  gnu records:\n%s' \
    "$o_rc" "$g_rc" "$o_out" "$g_out" "$o_err" "$g_err" "$o_rec" "$g_rec")
  if [ "$o_rc" = 99 ] || [ "$g_rc" = 99 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
       && [ "$o_rec" = "$g_rec" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  report "$name: $snippet"
}

# ------------------------------------------------------------ the command line

check 'help' cli 'systemd-cat --help'
check 'help, short' cli 'systemd-cat -h'
check 'help before a bad option' cli 'systemd-cat -h -z'
check 'a bad option before help' cli 'systemd-cat -z -h'
check 'invalid option' cli 'systemd-cat -z'
check 'unrecognized option' cli 'systemd-cat --zzz'
check 'missing argument' cli 'systemd-cat -p'
check 'missing argument, long' cli 'systemd-cat --priority'
check 'argument to a flag' cli 'systemd-cat --help=x'
check 'invoked by its path' cli '"$(command -v systemd-cat)" -z'
for p in bogus 8 0x9 '' INFO warn ' 3x' -1 08 0b1000; do
  check "bad priority [$p]" cli "systemd-cat -p '$p' true"
done
check 'bad stderr priority' cli 'systemd-cat --stderr-priority=bogus true'
for lp in maybe '' 'ye s' 'yes '; do
  check "bad level prefix [$lp]" cli "systemd-cat --level-prefix='$lp' true"
done
check 'a bad value with a newline in it' cli "systemd-cat --level-prefix=\$'a\\nb' true"
check 'a message cut where upstream cuts it' cli \
  "systemd-cat --level-prefix=\$(printf 'x%.0s' \$(seq 1 3000)) true"

# The log's own knobs.
check 'colours' cli 'SYSTEMD_COLORS=1 systemd-cat -p bogus'
check 'sixteen colours' cli 'SYSTEMD_COLORS=16 SYSTEMD_LOG_LOCATION=1 systemd-cat -p bogus'
check '256 colours with the location' cli 'SYSTEMD_COLORS=256 SYSTEMD_LOG_LOCATION=1 systemd-cat -p bogus'
check 'log colour off' cli 'SYSTEMD_LOG_COLOR=0 SYSTEMD_COLORS=1 systemd-cat -p bogus'
check 'level crit hides the error' cli 'SYSTEMD_LOG_LEVEL=crit systemd-cat -p bogus'
check 'level err shows it' cli 'SYSTEMD_LOG_LEVEL=3 systemd-cat -p bogus'
check 'target null' cli 'SYSTEMD_LOG_TARGET=null systemd-cat -p bogus'
check 'target console-prefixed' cli 'SYSTEMD_LOG_TARGET=console-prefixed systemd-cat -p bogus'
check 'target kmsg, not writable' cli 'SYSTEMD_LOG_TARGET=kmsg systemd-cat -p bogus'
check 'location' cli 'SYSTEMD_LOG_LOCATION=1 systemd-cat --stderr-priority=x'
check 'location of the boolean' cli 'SYSTEMD_LOG_LOCATION=yes systemd-cat --level-prefix=x'
check 'thread id' cli 'SYSTEMD_LOG_TID=1 systemd-cat -p bogus'
check 'time' cli 'SYSTEMD_LOG_TIME=1 systemd-cat -p bogus'
check 'everything at once' cli \
  'SYSTEMD_COLORS=1 SYSTEMD_LOG_TARGET=console-prefixed SYSTEMD_LOG_TIME=1 SYSTEMD_LOG_TID=1 SYSTEMD_LOG_LOCATION=1 systemd-cat -p bogus'
for v in TARGET LEVEL COLOR LOCATION TIME TID RATELIMIT_KMSG; do
  check "bad SYSTEMD_LOG_$v" cli "SYSTEMD_LOG_$v=bogus systemd-cat -p bogus"
done
check 'a bad knob, dressed by the ones before it' cli \
  'SYSTEMD_COLORS=1 SYSTEMD_LOG_COLOR=1 SYSTEMD_LOG_LOCATION=1 SYSTEMD_LOG_TIME=x systemd-cat -p bogus'
check 'a bad knob below the level shown' cli 'SYSTEMD_LOG_LEVEL=err SYSTEMD_LOG_COLOR=x systemd-cat -p bogus'
check 'help, coloured and linked' cli 'SYSTEMD_COLORS=1 systemd-cat --help'
check 'help, coloured, not linked' cli 'SYSTEMD_COLORS=1 SYSTEMD_URLIFY=0 systemd-cat --help'
check 'help, linked, not coloured' cli 'SYSTEMD_COLORS=0 SYSTEMD_URLIFY=1 systemd-cat --help'
check 'NO_COLOR loses to SYSTEMD_COLORS' cli 'NO_COLOR=1 SYSTEMD_COLORS=yes systemd-cat --help'

# On a terminal: colour, and a carriage return before each newline.
check 'an error on a terminal' cli "script -qec 'systemd-cat -p bogus' /dev/null"
check 'TERM=dumb' cli "TERM=dumb script -qec 'systemd-cat -p bogus' /dev/null"
check 'NO_COLOR' cli "NO_COLOR=1 script -qec 'systemd-cat -p bogus' /dev/null"
check 'help on a terminal' cli "script -qec 'systemd-cat --help' /dev/null"
check 'a failed exec on a terminal' cli "script -qec 'systemd-cat /nonexistent-xyz' /dev/null"

# What cannot be run.
check 'no such file' cli 'systemd-cat /nonexistent-xyz'
check 'nothing on PATH' cli 'systemd-cat no-such-cmd-xyz'
check 'a directory' cli 'systemd-cat /'
check 'not executable' cli 'systemd-cat ./nonexec'
check 'an empty name' cli 'systemd-cat ""'

# Closed descriptors.
check 'standard input closed, with a command' cli 'systemd-cat true <&-'
check 'standard input closed, no command' cli 'systemd-cat <&-'
check 'standard output closed' cli 'systemd-cat true >&-'
check 'help with standard output closed' cli 'systemd-cat --help >&-'

# ---------------------------------------------------------------- the records

check 'three lines, one prefixed, the last unfinished' rec \
  "printf 'one\\n<3>two\\nthree' | systemd-cat -t T"
check 'where journald cuts a line, and what it trims' rec \
  "printf 'a\\n\\nb\\r\\nc\\0d\\n<7>p7\\n<07>p07\\n<007>p007\\n<8>p8\\n<12>p12\\n<1>\\n<3>  sp\\n< 3>x\\n<3x>y\\n<>e\\n<3\\ntr  \\n   \\na\\rb\\nx\\r\\r\\ncr-nul\\r\\0q\\n<1>   \\n\\t\\n<3>\\r\\n end\\n' | systemd-cat -t T"
check 'a line longer than LineMax' rec 'systemd-cat -t T < big'
check 'a thousand lines' rec 'systemd-cat -t T < many'
check 'level prefixes off' rec "printf '<2>noprefix\\n' | systemd-cat -t T --level-prefix=no"
check 'a priority by name' rec "printf 'x\\n<5>y\\n' | systemd-cat -t T -p warning"
check 'a priority by number' rec "printf 'x\\n' | systemd-cat -t T -p 0x2"
check 'not UTF-8' rec "printf 'a\\377b\\n' | systemd-cat -t T"
check 'no command: cat, by name' rec "printf 'from-cat\\n' | systemd-cat"
check 'no identifier: the command by name' rec "systemd-cat sh -c 'echo nocomm; sleep 0.3'"
check 'an empty identifier is none' rec "systemd-cat -t '' sh -c 'echo empty-t; sleep 0.3'"
check 'stdout and stderr at two priorities' recu \
  "systemd-cat -t T -p notice --stderr-priority=err sh -c 'echo so; echo se >&2'"
check 'stderr at the same priority: one stream' rec \
  "systemd-cat -t T -p notice --stderr-priority=notice sh -c 'echo so; echo se >&2'"
check 'stderr priority alone' recu \
  "systemd-cat -t T --stderr-priority=3 sh -c 'echo so; echo se >&2'"
check 'a process the command started' rec \
  "systemd-cat -t T sh -c 'echo sh-line; /bin/echo child-line; printf partial; /bin/echo -n \" more\"; echo; sleep 0.2'"
check 'the exit status' rec "systemd-cat -t T sh -c 'echo before; exit 7'"
check 'killed by a signal' rec "systemd-cat -t T sh -c 'echo dying; kill -TERM \$\$'"
check 'JOURNAL_STREAM names the stream' rec \
  "systemd-cat -t T sh -c 'test \"\$JOURNAL_STREAM\" = \"\$(stat -L -c %d:%i /proc/self/fd/2)\" && echo match'"
check 'options end at the command' rec "systemd-cat -t T sh -c 'echo \"args: \$*\"' x -p 3"
check 'after --' rec "systemd-cat -t T -- sh -c 'echo dashdash'"
check 'abbreviated long options' rec "systemd-cat --ident=T --pri=3 sh -c 'echo abbrev'"
check 'standard input closed is the stream' rec "systemd-cat -t T sh -c 'echo to-stdin >&0' <&-"
check 'a failed exec with standard error closed' rec 'systemd-cat -t T /nonexistent-xyz 2>&-'
# The identifier travels in the stream's header, which journald reads a line at
# a time: stripped, or empty and so none, or -- with a newline in it, or 255
# bytes long -- a header journald refuses, closing the stream, so the
# command's next write kills it with SIGPIPE.
check 'an identifier is stripped' rec "systemd-cat -t '  spaced  ' sh -c 'echo spaced; sleep 0.3'"
check 'a blank identifier is none' rec "systemd-cat -t '   ' sh -c 'echo blank; sleep 0.3'"
check 'a tab inside an identifier' rec "systemd-cat -t \$'tab\\there' sh -c 'echo tabbed'"
check 'a newline in the identifier' rec "systemd-cat -t \$'bad\\nid' sh -c 'sleep 0.5; echo late'"
check 'the longest identifier' rec "systemd-cat -t \"\$(printf 'i%.0s' \$(seq 1 254))\" sh -c 'echo longest'"
check 'one byte longer' rec "systemd-cat -t \"\$(printf 'j%.0s' \$(seq 1 255))\" sh -c 'sleep 0.5; echo late'"
check 'a header written into the identifier' rec \
  "systemd-cat -t \$'x\\n\\n27\\n0\\n0\\n0\\n0' sh -c 'echo \"<5>after\"; sleep 0.3'"
check 'its own message to the journal' rec 'SYSTEMD_LOG_TARGET=journal systemd-cat -p bogus' 'stdout journal'
check 'its own message when standard error is a journal stream' rec \
  "systemd-cat -t T sh -c 'systemd-cat -p bogus; echo after'" 'stdout journal'

# ------------------------------------------------------------ on purpose

# --version names this build.
run_ours 'systemd-cat --version'; o_rc=$?
run_gnu 'systemd-cat --version'; g_rc=$?
REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s' "$o_rc" "$(cat "$DIFF_TMP/o.out")" "$g_rc" "$(head -1 "$DIFF_TMP/g.out")")
if [ "$(cat "$DIFF_TMP/o.out")" = 'systemd-cat (SlateOS coreutils) 0.1.0' ] && [ "$o_rc" = 0 ] && [ "$g_rc" = 0 ] \
   && head -1 "$DIFF_TMP/g.out" | grep -q '^systemd [0-9]'; then
  AGREED=yes; xfail=$((xfail + 1))
else
  AGREED=no
fi
report 'version names this build (expected difference): systemd-cat --version'

echo "systemd-cat-diff: $pass passed ($xfail of them expected differences), $fail failed, $broken broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
