### `TD-COREUTILS-GREP-DETECTS-BINARY-ONLY-IN-THE-FIRST-BUFFER`

Referenced by name from `BINARY_PROBE`'s doc comment in `grep.rs`.

Upstream re-runs the NUL scan on **every** read buffer until the first
detection; we scan only the first. For any file of 32 KiB or less -- which is
every fixture, every test, and most text -- the two are identical. Beyond that
we differ from GNU in one direction only: a file whose first 32 KiB are clean
but which turns binary later is printed where GNU would have started
suppressing part-way through.

The fix is not a bigger probe (that just moves the boundary) but moving the
scan into the read loop: check each refill until `binary` is set, which needs
`search_stream`'s line reader restructured to see buffer boundaries rather than
`read_until` hiding them. Left for when that reader is next touched. Low harm
in the meantime: the failure mode is *too much* output, never a wrong exit
status or a wrong count.
