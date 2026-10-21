# Fuzzing `sshwire`

Coverage-guided fuzzing (libFuzzer, through `cargo fuzz`) of what an SSH
peer sends before anything is authenticated, and of what a private key file
holds (design-decisions §1393). Any input `sshwire` cannot use must come back
as an error: a panic, a hang or a runaway allocation is a bug, and in `sshd`
it is one any stranger on the network can set off.

Two targets:

- `wire`: the identification line, a run of the wire encoding's reads
  (`uint32`, `boolean`, `string`, `mpint`, in an order the input chooses),
  base64 and an OpenSSH private key file, and the big-number arithmetic the
  key exchange does on the peer's values -- bounded as the exchange bounds
  them.
- `packet`: the packet layer in the clear, and under aes128-ctr and
  hmac-sha2-256 with keys fixed in the target, so the length, decryption and
  padding checks that run before the MAC is compared see what a hostile peer
  can send. A decoded packet must use at least one byte of the input and no
  more than it was given.

Neither runs under the address sanitizer: `sshwire` has no `unsafe` code.

## Running it

    python scripts/fuzz.py run userspace/sshwire/fuzz 1200   # seconds per target

`scripts/fuzz.py`'s docstring says what WSL needs. Run a round whenever the
parsing changes, beside no QEMU guest. A finding lands in
`~/fuzz/sshwire-fuzz/tree/fuzz/artifacts/` in WSL: fix the crate, and copy
the input into its tests as a regression test.

The harness came from lane F (`requests/f-b-fuzz-sshwire-a-harness-ready-to-drop-in.md`),
which ran each target for two minutes: `wire` 1.1 million inputs, `packet`
52 million, no finding.
