#!/bin/bash
# glibc 2.39's getifaddrs, as the oracle for posix/src/socket.rs's
# `GLIBC_UP`, `GLIBC_DOWN`, `GLIBC_NO_ADDRESS` and `GLIBC_SLASH_31`, which
# carry what this prints for `up`, `down`, `noaddr` and `slash31`, pasted by
# hand: the `entry` lines, less the `entry ` prefix, the veth's peer `p0`
# (whose MAC the kernel picks at random on every run) and the IPv6 lines
# (this system has no IPv6).
#
#   wsl -d Ubuntu -- bash posix/tools/oracle/ifaddrs_run.sh
#
# Runs ifaddrs_oracle.c in network sandboxes shaped like this system's: lo,
# and one Ethernet NIC called eth0 (a veth, whose peer is brought up so the
# link has carrier) with QEMU's MAC and the address QEMU's DHCP hands out.
#   up:      eth0 up, 10.0.2.15/24 with a broadcast address
#   down:    the same, then eth0 set down (the address stays)
#   noaddr:  eth0 up, no address (before DHCP answers)
#   slash31: eth0 up, 10.0.2.15/31
#   nobrd:   eth0 up, 10.0.2.15/24 with no broadcast address
# The sandbox is `unshare -r -n`: a user and network namespace, no root
# needed. The binary is built in a temporary directory.
set -u
here="$(cd "$(dirname "$0")" && pwd)" || exit 1
work="$(mktemp -d)" || exit 1
trap 'rm -rf "$work"' EXIT
gcc -O1 -Wall -o "$work/ifaddrs_oracle" "$here/ifaddrs_oracle.c" || exit 1
run() {
    local name="$1" setup="$2"
    echo "=== $name"
    unshare -r -n sh -c "
        sysctl -qw net.ipv6.conf.all.disable_ipv6=1 2>/dev/null
        sysctl -qw net.ipv6.conf.default.disable_ipv6=1 2>/dev/null
        ip link set lo up &&
        ip link add p0 type veth peer name eth0 && ip link set eth0 address 52:54:00:12:34:56 &&
        ip link set p0 up && $setup && '$work/ifaddrs_oracle'"
}
run up "ip addr add 10.0.2.15/24 brd + dev eth0 && ip link set eth0 up && sleep 0.3"
run down "ip addr add 10.0.2.15/24 brd + dev eth0 && ip link set eth0 up && sleep 0.3 && ip link set eth0 down"
run noaddr "ip link set eth0 up && sleep 0.3"
run slash31 "ip addr add 10.0.2.15/31 brd + dev eth0 && ip link set eth0 up && sleep 0.3"
run nobrd "ip addr add 10.0.2.15/24 dev eth0 && ip link set eth0 up && sleep 0.3"
