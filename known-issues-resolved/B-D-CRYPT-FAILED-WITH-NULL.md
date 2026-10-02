### [D] B-D-CRYPT-FAILED-WITH-NULL — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/crypt.rs` -- `crypt`, `crypt_r`, `encrypt`, and the
safe API's `hash_into`/`verify` through `compute_into`.

**In short:** when a password cannot be hashed, the `crypt` that Linux
systems ship (libxcrypt, since glibc dropped its own) still returns a string,
`"*0"`, that can never match a stored password, and sets `errno` to say why.
Ours returned NULL instead, so a program written for Linux that compares the
result directly -- common in login code -- would crash where Linux refuses
the login. It also accepted a few inputs libxcrypt refuses.

**What was wrong, against libxcrypt 4.4.36 as Ubuntu 24.04 builds it (probed
there):**

| | was | now |
|---|---|---|
| any failure | NULL | the failure token, `"*0"` (`"*1"` if the setting begins `"*0"`), written into the buffer before anything is checked |
| NULL passphrase or setting | `EFAULT` | `EINVAL` |
| a passphrase of 512 bytes or more | hashed | `ERANGE` |
| a setting with a space, a control or non-ASCII byte, or `! * : ; \` | hashed, with that byte in the salt -- a `:` would have split an `/etc/shadow` line | `EINVAL` |
| `encrypt(block, 2)` | `EINVAL` | `ENOSYS`, the stub's answer (libxcrypt reads any non-zero flag as "decrypt") |

The safe Rust API refuses what `crypt` refuses, so a C program and
`passwd`/`login` cannot disagree about one `/etc/shadow` entry.

**Still missing, and tracked in `todo.txt`:** libxcrypt's other methods --
traditional and BSDi DES, bcrypt (`$2b$`), scrypt (`$7$`), yescrypt (`$y$`,
Ubuntu's default for new passwords), and the rest -- and real DES behind
`encrypt`/`setkey`, which Ubuntu's libxcrypt provides. Their settings get the
token and `EINVAL`, never a made-up hash.
