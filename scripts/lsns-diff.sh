#!/usr/bin/env bash
# Differential test: our `lsns` against util-linux 2.39.3's.
#
# lsns lists the namespaces of the processes /proc shows, so what it prints
# is the process table -- which changes every time a program runs, not least
# because the program running is in it. Both sides are therefore run inside
# one private world: the harness re-enters itself as PID 1 of a user, PID,
# network and mount namespace of its own (`unshare -rpfn --mount-proc`),
# starts a fixed population there, and runs each case once per side against
# it. Two things would still differ between the sides, and both are pinned:
#
#   * The PIDs of the lsns process itself and of the `timeout` above it --
#     both are listed by `lsns <namespace>` -- are made equal by writing
#     /proc/sys/kernel/ns_last_pid before each run, which a PID namespace's
#     owner may do.
#   * Nothing else: the population lives until the harness ends, so every
#     namespace keeps its inode number, and netlink is only asked, never
#     told, so a namespace's NETNSID stays what it was.
#
# The population has a process in a new namespace of each of the eight
# types; user namespaces nested two deep, one owning a network and a UTS
# namespace; PID namespaces nested two deep; a parent with two children; a
# zombie; command lines with a tab and a control byte, and one longer than
# lsns's 8 KiB buffer; a command name holding `) (`; network, UTS and IPC
# namespaces kept by nsfs bind mounts alone, one of them at three paths
# (one holding a blank); and two veth pairs, which make the kernel give
# two network namespaces an ID in ours.
#
# What is compared: stdout, stderr and the exit status of each case --
#
#   * options and their refusals, including the exclusive pairs (and the
#     one upstream never refuses, -l with -T);
#   * every format: the process tree, --list, --raw, --json, --noheadings,
#     --notruncate, --nowrap, and every column;
#   * the owner and parent trees, each type alone, -P, -p;
#   * each namespace's own process listing, as `lsns <namespace>`;
#   * a terminal of many widths, the C locale's ASCII tree, and closed and
#     full descriptors;
#   * last, two worlds lsns cannot read: a process whose command name holds
#     a newline (upstream gives up on the whole table), and /proc gone.
#
# Upstream dereferences a missing process for a persistent namespace whose
# owner or parent is filtered out by -t (`lsns -t net -Towner` here): those
# cases are counted apart, as upstream crashing, and ours printing a table.
set -u

self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")

if [ "${1:-}" != --inside ]; then
  DIFF_PROG='lsns'
  DIFF_PKG='lsns'
  DIFF_NEED='timeout script stty unshare ip readlink stat'
  DIFF_REF=/usr/bin/lsns
  # shellcheck source=diff-wsl.sh
  . "$(dirname "$0")/diff-wsl.sh"
  if ! unshare -rpfn --mount-proc true 2>/dev/null; then
    echo "lsns-diff.sh: this WSL will not make a user namespace; skipped"
    exit 0
  fi
  LSNS_BINDIR=$bindir LSNS_TMP=$DIFF_TMP \
    unshare -rpfn --mount-proc --kill-child bash "$self" --inside
  exit $?
fi

# --- inside: PID 1 of a world of our own ---------------------------------------------
bindir=$LSNS_BINDIR
tmp=$LSNS_TMP
cd "$tmp" || exit 1
diff_run() { { "$@" 2>&4; } 4>&2 2>/dev/null; }

pass=0; fail=0; broken=0; crashed=0

# The namespace of type $2 that process $1 is in, as its inode number.
nsid() {
  local link
  link=$(readlink "/proc/$1/ns/$2") || return 1
  link=${link#*[}
  printf '%s' "${link%]}"
}

# --- the population ------------------------------------------------------------------
forever=100000
sleep "$forever" &
p_sleep=$!
bash -c "sleep $forever & sleep $forever & wait" &
p_parent=$!
unshare -u sleep "$forever" &
p_uts=$!
unshare -i sleep "$forever" &
p_ipc=$!
unshare -n sleep "$forever" &
p_net=$!
unshare -m sleep "$forever" &
p_mnt=$!
unshare -C sleep "$forever" &
p_cgroup=$!
unshare -T -f sleep "$forever" &
p_time=$!
unshare -U sleep "$forever" &
p_user=$!
unshare -U -n -u sleep "$forever" &
p_usernet=$!
unshare -p -f sleep "$forever" &
p_pid=$!
unshare -U -r -p -f --mount-proc unshare -U -p -f sleep "$forever" &
p_deep=$!
# A zombie: `sleep 0` exits, and the `sleep` its parent became never reaps it.
bash -c "sleep 0 & exec sleep $forever" &
p_zombie_parent=$!
cp "$(command -v sleep)" "$tmp/x) (y"
"$tmp/x) (y" "$forever" &
p_paren=$!
bash -c 'exec -a "$0" sleep '"$forever" "$(printf 'we\tird\001arg0')" &
p_weird=$!
bash -c 'exec -a "$0" sleep '"$forever" "$(printf '%09000d' 0)" &
p_long=$!

: > "$tmp/netA"; : > "$tmp/netA2"; : > "$tmp/net B"; : > "$tmp/utsA"; : > "$tmp/ipcA"
unshare --net="$tmp/netA" true
mount --bind "$tmp/netA" "$tmp/netA2"
mount --bind "$tmp/netA" "$tmp/net B"
unshare --uts="$tmp/utsA" true
unshare --ipc="$tmp/ipcA" true
ip link add v0 type veth peer name v1 netns "$p_net"
ip link add v2 type veth peer name v3 netns "$p_usernet"
# Every process above has exec'd by now.
sleep 1

# The world is checked before any case runs, through the reference: a
# namespace that failed to form (a kernel refusing veth, say) would leave
# both sides agreeing on a poorer table, and the run would pass while
# testing less than it says.
world_broken=
lsns_ref() { PATH="$bindir/gnu:$PATH" lsns "$@" 2>/dev/null; }
[ "$(lsns_ref -P -n -o NS | wc -l)" -eq 3 ] || world_broken="$world_broken persistent"
# The persistent one has no process, so no NETNSID: its line is blank.
netnsids=$(lsns_ref -t net -n -l -o NETNSID | tr -d ' ' | grep -v '^$' | sort | tr '\n' ' ')
[ "$netnsids" = "0 1 unassigned " ] || world_broken="$world_broken netnsid($netnsids)"
[ "$(lsns_ref -t user -n -l -o NS | wc -l)" -ge 5 ] || world_broken="$world_broken user-nesting"
[ "$(lsns_ref -t pid -n -l -o NS | wc -l)" -ge 4 ] || world_broken="$world_broken pid-nesting"
for t in mnt net pid uts ipc user cgroup time; do
  [ "$(lsns_ref -t "$t" -n -l -o NS | wc -l)" -ge 2 ] || world_broken="$world_broken $t"
done
grep -qs '^[0-9]* (sleep) Z ' /proc/[0-9]*/stat || world_broken="$world_broken zombie"
if [ -n "$world_broken" ]; then
  printf 'BROKEN the world did not form:%s\n' "$world_broken"
  printf '0 passed, 0 differed, 1 broken, 0 where only upstream crashed\n'
  exit 1
fi

# --- running a case --------------------------------------------------------------------
# Every run starts its processes from the same PID. (Once /proc is gone,
# at the end, there is nothing to pin, and nothing that shows a PID.)
pin_pids() { { echo 999 > /proc/sys/kernel/ns_last_pid; } 2>/dev/null; }

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  pin_pids
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" PATH="$bindir/$side:$PATH" \
    timeout -k 2 30 lsns "$@"
}
# $1 = side, $2 = columns, rest = argv: on a pty that wide.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60 && lsns"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  pin_pids
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" \
    timeout -k 1 20 script -qec "$cmd" /dev/null </dev/null
}

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$g_rc" = 139 ] && [ "$o_rc" != 139 ]; then
    AGREED=crashed
  elif [ "$o_rc" = 127 ] && [ ! -s "$tmp/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$tmp/o.out" "$tmp/g.out" \
     && cmp -s "$tmp/o.err" "$tmp/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err %q\n  gnu  (rc=%s): err %q\n%s' \
    "$o_rc" "$(head -c 600 "$tmp/o.err")" "$g_rc" "$(head -c 600 "$tmp/g.err")" \
    "$(diff "$tmp/g.out" "$tmp/o.out" | head -20)")
}

report() {
  if [ "$AGREED" = crashed ]; then
    crashed=$((crashed + 1))
    [ -n "${VERBOSE:-}" ] && printf 'CRASH %s (upstream crashed; ours did not)\n' "$1"
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# both ARGV...: both sides, with these arguments.
both() {
  local o_rc g_rc
  run_side ours "$@" >"$tmp/o.out" 2>"$tmp/o.err"; o_rc=$?
  run_side gnu "$@" >"$tmp/g.out" 2>"$tmp/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[${CASE_LOCALE:-C.UTF-8}] lsns $(printf '%q ' "$@" | sed "s|$tmp/||g")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$tmp/o.out" 2>"$tmp/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$tmp/g.out" 2>"$tmp/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] lsns $(printf '%q ' "$@")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs lsns, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  pin_pids
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 30 \
    bash -c "exec lsns \"\$@\" $how" sh "$@" >"$tmp/o.out" 2>"$tmp/o.err"; o_rc=$?
  pin_pids
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 30 \
    bash -c "exec lsns \"\$@\" $how" sh "$@" >"$tmp/g.out" 2>"$tmp/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "lsns $(printf '%q ' "$@")$how"
}

many_ns=NS
for _ in $(seq 25); do many_ns=$many_ns,NS; done

# --- options and their refusals --------------------------------------------------------
for args in '-h' '--help' '-V' '--version' '--vers' '--bogus' '-z' '--no' '--he' \
            '-J -r' '-r -J' '-P -p 1' '-p 1 -P' '-l -T' '-T -l' '-T=owner' '-Tparent' \
            '-T=process' '--tree=process' '--tree=bogus' '--tree bogus' '--tree=' \
            '-T owner' '--tree owner' '-T parent 4026531835' \
            '-t bogus' '-t ""' '-t NET' '-t net,user' '-t' '-p' '-p x' '-p -1' '-p 99999999999' \
            '-p 0' '-p " 1"' '-p 1x' '-p ""' '-o ""' '-o +' '-o NS,' '-o ,NS' '-o NS,,TYPE' \
            '-o ns,type' '-o +PATH' '-o NOSUCH' '-o +NOSUCH,NS' "-o $many_ns" "-o $many_ns,NS" \
            "-o +$many_ns" 'abc' '0' '18446744073709551616' '99999' '-- -1' '1 2' \
            '-p 1 4026531835' '4026531835 -p 1' '" 4026531835"' '4026531835x'; do
  eval "set -- $args"
  both "$@"
done

# --- every format and filter ------------------------------------------------------------
for args in '' '-l' '-r' '-J' '-J -l' '-n' '-n -l' '-u' '-W' '-u -W' '-r -n' \
            '--output-all' '--output-all -l' '--output-all -r' '--output-all -J' '--output-all -T' \
            '-o +PATH,UID,PPID' '-o NS,TYPE,NETNSID,NSFS' '-o NS,TYPE,NETNSID,NSFS -W' \
            '-o NS,NSFS -r' '-o NS,NSFS -J' '-o NS,NSFS -l' '-o NETNSID,NS -J' \
            '-o PNS,ONS,NS,TYPE' '-o COMMAND,PATH' '-o USER,UID -J' \
            '-T' '-Towner' '-Tparent' '-Tprocess' '-T -o +PNS,ONS' '--tree=parent -J' \
            '--tree=owner -r' '-T -l' '-l -Tparent' '-Tparent -o NS,TYPE,PNS' \
            '-Towner --output-all' '-Tparent -n' \
            '-t net' '-t net -l' '-t net -J' '-t net -r' '-t user' '-t user -Tparent' \
            '-t user -Towner' '-t user -T' '-t pid -Tparent' '-t pid -T' '-t pid -l' \
            '-t mnt -t uts' '-t time' '-t cgroup' '-t ipc' '-t uts -o +NSFS' '-t mnt -Towner' \
            '-t mnt -Tparent' '-t user -t net -Towner' '-t net -t user -Tparent' \
            '-P' '-P -o +NSFS' '-P -T' '-P -Tparent' '-P -l' '-P -J' '-P -t net' '-P -t pid' \
            '-p 1' '-p 1 -Towner' '-p 1 -l' '-p 1 -J' '-p 2' \
            '-t net -Towner' '-t net -Tparent' '-t uts -Towner' '-t ipc -Tparent'; do
  eval "set -- $args"
  both "$@"
done
for p in "$p_sleep" "$p_parent" "$p_uts" "$p_ipc" "$p_net" "$p_mnt" "$p_cgroup" "$p_time" \
         "$p_user" "$p_usernet" \
         "$p_pid" "$p_deep" "$p_zombie_parent" "$p_paren" "$p_weird" "$p_long"; do
  both -p "$p"
  both -p "$p" -o +PATH,PPID -l
done

# --- each namespace's processes ----------------------------------------------------------
ids=
for t in mnt net pid uts ipc user cgroup time; do ids="$ids $(nsid 1 "$t")"; done
ids="$ids $(nsid "$p_uts" uts) $(nsid "$p_ipc" ipc) $(nsid "$p_net" net) $(nsid "$p_mnt" mnt)"
ids="$ids $(nsid "$p_cgroup" cgroup) $(nsid "$p_user" user)"
ids="$ids $(nsid "$p_usernet" net) $(nsid "$p_pid" pid_for_children) $(nsid "$p_deep" user)"
ids="$ids $(nsid "$p_time" time_for_children) $(stat -L -c %i "$tmp/netA") $(stat -L -c %i "$tmp/utsA")"
for id in $ids; do
  for args in '' '-l' '-J' '-r' '-Towner' '-Tparent' '-o PID,COMMAND' '-o +NS,TYPE,NSFS' \
              '-u' '--output-all -l' '-p 1'; do
    eval "set -- $args"
    both "$@" "$id"
  done
done

# --- the C locale draws the tree in ASCII ------------------------------------------------
CASE_LOCALE=C
for args in '' '-Towner' '-Tparent' '-J' '-o NS,NSFS' "$(nsid 1 mnt)"; do
  eval "set -- $args"
  both "$@"
done
unset CASE_LOCALE

# --- terminals ------------------------------------------------------------------------------
for cols in 1 20 40 60 80 100 132 200; do
  pty_case "$cols"
  pty_case "$cols" --output-all
  pty_case "$cols" -o NS,NSFS,COMMAND
  pty_case "$cols" -t net -o +NSFS
  pty_case "$cols" -Tparent
  pty_case "$cols" -u
  pty_case "$cols" "$(nsid 1 mnt)"
done

# --- closed and full descriptors -----------------------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how"
  redir_case "$how" --output-all -J
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
  redir_case "$how" -t bogus
  redir_case "$how" 99999
done

# --- a process whose command name holds a newline --------------------------------------------
# Its /proc/PID/stat line ends before the `)` after the name, and upstream
# gives up on the whole table.
nl_name=$(printf 'a\nb')
cp "$(command -v sleep)" "$tmp/$nl_name"
"$tmp/$nl_name" "$forever" &
p_nl=$!
sleep 1
for args in '' '-l' '-J' "$(nsid 1 mnt)" '-p 1' '-t net' '-P'; do
  eval "set -- $args"
  both "$@"
done
kill "$p_nl"
wait "$p_nl" 2>/dev/null

# --- /proc gone ------------------------------------------------------------------------------
mount -t tmpfs none /proc
both
both -h
both -t net

printf '%d passed, %d differed, %d broken, %d where only upstream crashed\n' \
  "$pass" "$fail" "$broken" "$crashed"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
