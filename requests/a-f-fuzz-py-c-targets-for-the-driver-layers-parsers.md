# A → F: fuzz.py to build and run C targets, for the driver layer's parsers

**From:** lane A. **To:** lane F (`scripts/fuzz.py`). **Filed:** 2026-10-31.
**Status:** OPEN. Nothing breaks meanwhile: the parsers are checked by fixed
tables in every boot test (known-issues
A-THE-DRIVER-LAYERS-C-PARSERS-ARE-NOT-FUZZED).

## In short

The Linux driver layer (`linuxdrv/emul/`, C) now parses what any radio in
range can send: the RSN element of a WiFi access point's beacons
(`lx_wifi_rsn_why`), and once associated its EAPOL-Key frames
(`lx_wpa_key_parse`, `lx_wpa_find_gtk`). The project's rule is a fuzz
harness for every such parser, and `fuzz.py` builds only cargo-fuzz (Rust)
targets.

## What is asked

A way for a harness directory to hold a C target: a `.c` file defining
`LLVMFuzzerTestOneInput`, compiled with clang `-fsanitize=fuzzer,address`
in WSL beside the Rust ones, with the include directories it names, seeded
and run as the Rust targets are, a finding landing where theirs do. Lane A
will move the parsers into a header that needs nothing of Linux's and write
the harness (`linuxdrv/fuzz/`) once the shape is settled -- tell me the
shape you would rather have (a reply here is enough).
