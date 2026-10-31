## A-THE-DRIVER-LAYERS-C-PARSERS-ARE-NOT-FUZZED (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** the project's rule is that every parser of untrusted input
gets a fuzz harness (`CLAUDE.md`, design-decisions 1393). The Linux driver
layer's own C parsers have none, because the fuzzing tool only builds Rust
crates. One of them reads what any radio in range can send: the RSN element
a WiFi access point advertises. Today it is checked by a fixed table of
elements and by every cut of each at every length, in every boot test.

**Where.** `linuxdrv/emul/lx_wifi.c`: `lx_wifi_rsn_why` (an access point's
RSN element, from its beacons and probe responses) and
`lx_wifi_network_parse` (the boot medium's network file, the operator's);
also `lx_wpa.c`'s `lx_wpa_key_parse` and `lx_wpa_find_gtk` (EAPOL-Key frames
and their key data, from the access point once associated). Their checks:
`lx_wifi_join_check` and `lx_wpa_check`, run in the hwsim run.

**The proper fix.** A libFuzzer harness for C: the parsers moved into a
header of their own that needs nothing of Linux's (`u8`, the byte-order
reads and the suite numbers defined in it), included by `lx_wifi.c` and by a
harness target compiled with `-fsanitize=fuzzer,address`; and
`scripts/fuzz.py` (lane F's) taught to build and run a C target as it runs a
Rust one -- asked of lane F in
`requests/a-f-fuzz-py-c-targets-for-the-driver-layers-parsers.md`.
