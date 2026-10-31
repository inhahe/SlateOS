## 1596. The test PC's WiFi joins the operator's home network, its password in a file outside git

**Date:** 2026-10-30 · **Decided by:** Operator, the network (A-Q31, option A, answered in chat; `operator-answers/2026-10-30-chat-to-lane-a.txt`); the way its password travels is the question's own recommendation for that answer, option 1, which the answer left open and lane A applied · **Lane:** A

**In short:** The test PC's WiFi card (the RZ717, which Linux's mt7925e
drives) joins the operator's home network for its real-network tests,
rather than a separate test network. The network's name and password are
kept in a file on the operator's PC outside every repository; the boot
medium written to the test PC's E: drive carries a copy, and only the WiFi
driver process may read it there. Neither is ever committed or printed in a
log.

### The decision

1. **The network: the operator's home network** (A-Q31 option A). Its name
   and password are in `C:\Users\Computer\.slateos-wifi`, as
   `ssid=<name>` and `psk=<passphrase>` lines, readable only by the
   operator's account and the system's own.
2. **How the password reaches the test PC: a file on the boot medium**
   (option 1). `scripts/pikvm.py` stages the file as a boot module, never
   as kernel command-line text, which every startup log and boot-test
   record prints. The kernel places it on a filesystem of its own, sealed
   read-only like the boot medium's firmware, and gives a tree over that
   directory (design-decisions 1595) to the WiFi driver process alone.
3. **Nothing prints it.** The kernel and the driver say whether a
   credential was found and which network was joined, never the
   passphrase. A copy of the operator's answer in `operator-answers/`
   leaves both out.

### Alternatives

- **B, a separate test network** -- the question's recommendation: a
  password guarding nothing else. The operator chose the home network,
  which needs no hotspot or spare router switched on for a test.
- **2, the kernel command line** -- simplest, but the password would be in
  every startup log, every serial capture and the boot history.

### Consequences

The home network's password is on the test PC's E: drive while a boot
medium carrying it is there, readable by anyone with that drive -- the cost
option A states. The test PC's C: drive is never written
(`blk.writable` names E: alone), so it holds no copy.
