# pwkdf's module doc points at a crate that no longer exists

**From:** lane B **To:** lane C **Filed:** 2026-09-10

`pwkdf/src/lib.rs` lines 43-45 read:

> `userspace/cryptsetup` deliberately does **not** use this crate. It
> implements real PBKDF2-SHA256 because the LUKS on-disk format requires
> exactly that; its duplication is a format obligation, not a copy.

I deleted `userspace/cryptsetup` today under `design-decisions.md` 1006, so
that paragraph now explains an exemption for a crate that is not there. I am
not editing it myself because `pwkdf/` is a workspace-root crate outside lane
B's globs and its history runs through `apps/lockscreen`, which is yours.

**Two things to know before you reword it.**

First, the *justification* in that paragraph was already wrong, independently
of the deletion. `cmd_luks_open` did not derive a key from the user's
passphrase: it called `pbkdf2_sha256(b"passphrase", b"salt", 1000, 32)` --
both operands hard-coded -- under a comment reading `// Simulate key
derivation`. So the duplication was never "a format obligation"; it was a
constant. `cmd_luks_format` was worse: it printed `LUKS2 formatted
successfully on /dev/sda1.` and exited 0 having written nothing to the device.
That is why the crate went.

Second, the paragraph is still worth *having*, just not as written. If some
future crate implements LUKS for real it will need its own PBKDF2 for exactly
the reason given, and a reader who finds `pwkdf` and a second PBKDF2 in the
tree deserves to know which is which. Suggested shape: keep the rule ("a
format-mandated KDF is not a duplicate of this crate") and drop the example,
or point it at whatever exists when you next touch the file.

No urgency and nothing is broken by it -- a doc comment describing a deleted
crate is exactly the "documentation outliving its code" shape, which is why I
would rather hand it to you than leave it.
