## TD-B-SWEPT-FOR-MORE-SIMULATED-DUPLICATES (lane B, 2026-09-10) — closed, avahi was the only one

**In short:** after deleting `userspace/avahi` for simulating a service
`kernel/src/net/mdns.rs` implements for real, I swept for others. There are
none in lane B. Recorded so the sweep is not repeated, and because the
discriminator took two tries to get right.

**What does NOT work: the roadmap-text signal.** avahi was found because
`roadmap.md` marks mDNS/DNS-SD `[x]` twice. Generalised — index every `[x]`
entry on rare words, report pairs sharing five or more — that produces 118
pairs and they are almost all legitimate siblings written from one template:
Sokoban/Klotski/Rush Hour, SysV shm/msg/sem, epoll/inotify, Hearts/Spades. The
duplicate mark was a real clue about avahi and is not a rule.

**What does work: the same name in both trees, then ask who does the I/O.**
22 names exist as both a `kernel/src/**` module and a `userspace/*` crate. Most
are correct layering — the kernel provides a mechanism, userspace provides the
command, and `tar`, `cpio`, `zip`, `acl` are all that shape. The avahi
discriminator is narrower: **the userspace side did no I/O of its own domain
while the kernel side did the real work.**

Applied to the five where it could bite — `arp`, `ftp`, `netstat`, `telnet`,
`traceroute` — all five do real network I/O (23 to 41 call sites each). And
`userspace/ssh`, 15 lines against a 2,940-line kernel module, is a deliberate
shim over its own `lib.rs`, the same shape as `sshd`, with the reason in its
module doc.

**A measurement trap worth knowing.** The first run of that check piped Rust
sources through the console and two crates came back with **zero** network I/O
— `arp` and `telnet` — because their sources contain a U+2192 arrow and the
Windows console encoder raised `UnicodeEncodeError` mid-pipe. The traceback
scrolled past above the table and the table read as a finding. Re-run entirely
in Python they are 25 and 36. **A crate that fails to print looks exactly like
a crate with nothing in it**, which is this file's recurring theme arriving in
the measuring instrument rather than the subject.
