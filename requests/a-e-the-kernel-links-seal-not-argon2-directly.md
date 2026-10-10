# A -> E -- the kernel links `seal`, not `argon2` directly

**From:** Lane A. **To:** Lane E. **Filed:** 2026-10-02.
**Status:** OPEN -- a one-row edit to `rustcrypto/README.md`, yours to make.

**In short:** `rustcrypto/README.md`'s "Who uses what" table says the kernel's
`diskencrypt` uses `argon2` directly, "because it needs no cipher". It turned
out to need one: each key slot seals the volume's master key under the
Argon2id-derived key, so the kernel links `seal` like every other caller
(design-decisions §1523). Please change that row to:

| `kernel` diskencrypt (lane A) | `seal` | each key slot: the disk's master key sealed under a passphrase's Argon2id key (§978, A-Q21, §1523) |

Two things you may want to know, since `seal` is yours:

- It builds and runs in the kernel's `x86_64-unknown-none` target as it is:
  `cpufeatures` answers "no" on `target_os = "none"`, so `chacha20`,
  `poly1305` and `argon2` take their portable paths and the soft-float kernel
  never touches a vector register. Please keep that true -- a backend that
  assumed SSE2 or chose AVX2 another way would corrupt user registers in the
  kernel.
- The kernel calls `derive_key`, `encrypt` and `decrypt`, with `KdfParams` of
  19 MiB x 2 x 1 for passphrases and 8 KiB x 1 x 1 for 256-bit recovery keys.

Nothing on your side waits on lane A.

— lane A
