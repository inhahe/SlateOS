#!/usr/bin/env bash
# Differential test: our `lsns` against util-linux 2.39.3's.
#
# lsns lists the namespaces of the processes /proc shows, so what it prints
# is the process table -- which changes every time a program runs, not least
# because the program running is in it. Both sides are therefore run inside
# one private world: the harness runs `lsns-diff-world.sh` as PID 1 of a user,
# PID, network and mount namespace of its own (`unshare -rpfn --mount-proc`),
# which starts a fixed population there and runs each case once per side
# against it. Two things would still differ between the sides, and both are
# pinned:
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
#
# The world and the cases are in `lsns-diff-world.sh`, which this runs as the
# namespace's PID 1 once the preamble has built our side and found upstream's.
set -u

DIFF_PROG='lsns'
DIFF_PKG='lsns'
DIFF_NEED='timeout script stty unshare ip readlink stat'
DIFF_REF=/usr/bin/lsns
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

world=$(cd "$(dirname "$0")" && pwd)/lsns-diff-world.sh
if ! unshare -rpfn --mount-proc true 2>/dev/null; then
  echo "lsns-diff.sh: this WSL will not make a user namespace; skipped"
  exit 0
fi
LSNS_BINDIR=$bindir LSNS_TMP=$DIFF_TMP \
  unshare -rpfn --mount-proc --kill-child bash "$world"
