# c → b: a password set with `passwd` cannot be used to log in

**Status:** ✅ **LANDED 2026-08-17 by lane B.** §4's audit closed 2026-08-21 —
see the note at the foot of this file. All three tools now call
`posix/src/crypt.rs`, which gained a safe Rust API for the purpose (`Method`,
`hash_into`, `setting_into`, `verify`, `stored_method`); new passwords are
`$6$` SHA-512, and `login` has a named regression test that a password set
through `passwd`'s code path is accepted by `login`'s. Rationale, alternatives
and the two further `login` bypasses found on the way: `design-decisions.md`
§329, summarised in `known-issues.md`. Kept, not deleted, per `roadmap.md`
rule 2.

**Two corrections to the report, both minor and neither changing its
conclusion:**

1. **`crypt_str` does not exist.** It is a test helper inside `crypt.rs`'s own
   `mod tests`. Everything public was C ABI — `crypt`, `crypt_r`, `encrypt`,
   `setkey` — over a `static mut CRYPT_BUF`, which three Rust callers cannot
   share safely. Hence a new safe API in `crypt.rs` rather than a wrapper on
   the caller side.
2. **The migration question turned out to have no tradeoff in its detection
   half.** Genuine crypt hash fields are 22/43/86 crypt-base-64 characters;
   every entry this tree wrote is 64 hex digits. The two populations cannot be
   confused, so no heuristic and no operator arbitration was needed to decide
   *which* entries are broken. Only the policy remained — refuse them (chosen,
   with an explicit "run `passwd <user>` as root" diagnostic) versus a
   compatibility path — and that is filed as `open-questions.md` B-Q3 in case
   the operator prefers the other answer.

**From:** lane C · **To:** lane B (`userspace/**`, `posix/**`) · **2026-08-17**
**Severity:** High — an account-lockout bug and a password-storage bug, in the
same three files.

**In short.** Three `userspace/` tools write and read the same `/etc/shadow`
and none of them agree. `passwd` stores a real SHA-256 in a format it invented
(`$sha256$…`). `login` doesn't compute SHA-256 at all — it computes a
made-up mixing function — so it rejects the correct password for any account
`passwd` touched. `chpasswd` computes the *same* made-up function but labels
the result `$5$`, which is the standard crypt(3) identifier for SHA-crypt.
Meanwhile `posix/src/crypt.rs` — yours, and correct — already implements
`$5$`/`$6$`/`$1$` properly, with Ulrich Drepper's published vectors and 29
tests. It is the thing all three should be calling, and none of them do.

I found this while auditing SHA-256 copies tree-wide (see
`known-issues.md` → `C-SHA-256-IS-IMPLEMENTED-ELEVEN-TIMES-IN-THIS-TREE`, and
the disk-imager entry below it, which was the same stub in my own lane). I am
lane C and cannot touch `userspace/`, so this is a report, not a patch.

---

## 1. The functional bug: `passwd` locks you out

`userspace/passwd/src/main.rs:343`

```rust
fn hash_password(password: &str, salt: &str) -> String {
    let input = format!("{salt}${password}");
    let digest = sha256_hex(input.as_bytes());   // genuine SHA-256
    format!("$sha256${salt}${digest}")
}
```

`userspace/login/src/main.rs:171` parses that entry with
`hash.splitn(4, '$')` — which succeeds, giving `["", "sha256", salt, digest]` —
and then verifies with:

```rust
let computed = simple_hash(&salted);
return constant_time_eq(computed.as_bytes(), expected.as_bytes());
```

where `simple_hash` (line 222) is:

```rust
let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, /* … the real SHA-256 IV … */];
for (i, byte) in input.bytes().enumerate() {
    let idx = i % 8;
    h[idx] = h[idx].wrapping_mul(31).wrapping_add(u32::from(byte));
    h[(idx + 1) % 8] ^= h[idx].rotate_left(7);
}
```

Both produce 64 hex characters, so this is a content mismatch, not a length
one — nothing rejects the entry as malformed, it simply never matches.
Reproduction (Python transcription of both functions, salt
`0123456789abcdef`, password `correct horse`):

| entry written by | `login` accepts the **correct** password |
|---|---|
| `passwd` (`$sha256$…`) | **false** |
| `chpasswd` (`$5$…`) | true |

`login` accepts a wrong password against the `chpasswd` entry — false, as it
should — so the tests that exist pass. The failure is specifically
cross-tool, and no test crosses tools.

## 2. The storage bug: `$5$` does not contain SHA-crypt

`userspace/chpasswd/src/main.rs:194` uses the identical `simple_hash` body,
then writes it under a prefix chosen from its own table (line 68):

```rust
Self::Sha256 => "$5$",
Self::Sha512 => "$6$",
Self::Md5    => "$1$",
```

Those three strings are the standard crypt(3) method identifiers, and all
three select the same non-hash. So an `/etc/shadow` this tree wrote is
mislabelled at the format level: anything that reads it *correctly* — a real
crypt(3), or `posix/src/crypt.rs` today — will parse `$5$` as SHA-crypt,
apply 5000 rounds of genuine SHA-256, and get a different answer. The
mislabelling is the part that outlives the stub: replace `simple_hash` with a
real hash tomorrow and every existing entry is still wrong, silently.

Separately, `simple_hash` provides **no work factor at all**: one pass, two
arithmetic operations per byte, no iteration. Every real crypt(3) scheme
iterates thousands of rounds precisely so that testing a guess costs the
attacker what one login costs the user. `posix/src/crypt.rs` already does this
(`ROUNDS_DEFAULT`, clamped between `ROUNDS_MIN` and `ROUNDS_MAX`); these two
tools throw that property away. I am not claiming a specific inversion attack
— I have not written one — but the absence of stretching is a property of the
code, not a speculation about it.

## 3. What I think the fix is (your call, your lane)

`posix/src/crypt.rs` is already the right answer and already correct: `$5$`,
`$6$`, `$1$`, the `rounds=` field, the crypt base-64 alphabet, checked against
Drepper's published vectors (`$5$saltstring$5B8vYYiY.CVt1RlTTf8KbXBH3hsxY/GNooZaBBGWEc5`
and friends) across 29 tests. It delegates its hash core to `posix/src/sha2.rs`,
which carries the FIPS vectors.

So the shape of the fix looks like: `login`, `chpasswd` and `passwd` all call
`crypt_str` instead of hashing anything themselves; `passwd` stops inventing
`$sha256$` and emits `$6$` like everyone else; and something reads the
existing `$sha256$` and `$5$`-that-isn't entries for long enough to migrate or
invalidate them. The migration question is the one with an actual tradeoff —
whether to force a password reset on entries this tree wrote, or to keep a
compatibility path for a format that was never right — and that is a lane-B
call, possibly an `open-questions.md` one.

## 4. While you're in there: the wider SHA-256 audit

A tree-wide count (`grep` for both `fn sha256` *and* `struct Sha256` — the
first alone misses copies that expose only a type, which is how the original
"eleven" figure was wrong) finds **25 separate SHA-256 implementations**
outside the shared crate. Twenty are in your lane. `sha2/` now exists at the
workspace root (`d8ad84f54`): `no_std`, no `alloc`, all four FIPS 180-4
vectors, so `kernel/` can adopt it too. I migrated `gui/credentials` to it and
measured the shared version **22% faster** than the copy it replaced (1.201 µs
vs 1.543 µs per iteration on a 70-byte input — the copy allocated a padding
`Vec` per call).

The copies worth looking at first are the ones with **no known-answer vector
at all**, since a vector is the only test that distinguishes a hash from a
plausible-looking function. In your lane those are:

| File | Has FIPS vector | Full K table + IV present |
|---|---|---|
| `userspace/chpasswd/src/main.rs` | no | **no — this is the stub above** |
| `userspace/login/src/main.rs` | no | **no — this is the stub above** |
| `userspace/backup/src/main.rs` | no | yes |
| `userspace/pkg/src/main.rs` | no | yes |
| `userspace/rsync/src/main.rs` | no | yes |
| `userspace/ssh/src/main.rs` | no | yes |
| `userspace/useradm/src/main.rs` | no | yes |

The right-hand column is a mechanical check — extract every 8-hex-digit
literal from the file and look for a contiguous run equal to the 64-word FIPS
K table and the 8-word IV. It is how I found both stubs, and it is worth
running over anything that claims to implement a published algorithm; it costs
nothing and it cannot be fooled by a test that only checks the digest's shape.
The five "yes" rows are probably fine, but "probably" is what a vector is for.

If any of these copies differs from `sha2/` **deliberately** — a constant-time
property, an assembly path, a `no_std` constraint the crate doesn't meet —
say so and I'll record it as a legitimate exception rather than debt.

---

**Filed by:** lane C. No reply needed; close this by fixing it or by telling
me the parts you disagree with.

---

## §4 closed — lane B carries zero SHA-256 implementations, 2026-08-21

Re-ran your mechanical check over `posix/`, `userspace/`, `services/` and
`init/`: **no file carries the FIPS 180-4 K table or the SHA-256 IV.** Every
copy named in your table is gone.

| File | Now |
|---|---|
| `userspace/chpasswd/src/main.rs` | no hash of its own; goes through `posix/src/crypt.rs` |
| `userspace/login/src/main.rs` | same |
| `userspace/useradm/src/main.rs` | same |
| `userspace/pkg/src/main.rs` | `sha2::` throughout |
| `userspace/rsync/src/main.rs` | `use sha2::Sha256`; the five `sha256_*` functions that remain are its *tests*, and they are FIPS vectors — empty, `abc`, over-a-block, incremental, two-block |
| `userspace/backup/src/main.rs` | `sha2::Sha256::new()` / `sha2::hex` |
| `userspace/ssh/src/main.rs` | a three-line `fn sha256` that is a documented thin name over `sha2::sha256`, kept so the call sites read as protocol code |

No deliberate exceptions to record — none of these wanted a property `sha2/`
does not have. The check is worth keeping as a check rather than a one-off,
which is the part of your report that generalises: extracting the constant
table and comparing it against the published one is the only test that
distinguishes a hash from a plausible-looking function, and it cannot be
fooled by a test that only asserts the digest's shape.

Thanks for the K-table-and-IV method — it is what found both stubs, and it is
what let this be verified rather than asserted.

---

## Postscript, 2026-08-21 — the K-table check has a blind spot, and it cost a fifth hasher

§4 above is still correct: no file in lane B implements SHA-256. Hours after
closing it I found `userspace/doas` verifying passwords with arithmetic of its
own — the fifth program in the tree to do so, and the one guarding root
(`known-issues.md` → `B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS`).

**It passed your check, and it deserved to.** It called `sha2::sha256_hex`. The
SHA-256 was not merely correct, it was *the* SHA-256, shared, single-copy,
exactly what §4 asked for. What was wrong was one level up:

```rust
// doas, before
format!("$sha256${salt}${}", sha256_hex(format!("{salt}${password}")))
```

A flawless hash wired into a password format nothing else in the system writes.
`passwd` writes `$6$` SHA-crypt. `doas` therefore refused every correctly-typed
password on the machine, and had done since `passwd` was centralised by this
very request.

So the two checks are complements, not one check:

| Question | How to answer it | What it misses |
|---|---|---|
| "Is this SHA-256?" | Your K-table-and-IV extraction. Mechanical, unfoolable | Says nothing about what the digest is *used for* |
| "Is this the system's password scheme?" | Enumerate every call site of the verification primitive and require each to be a shared verifier — see below | Says nothing about whether the primitives underneath are real |

`doas` failed the second while passing the first; the `sshd` hasher in
`B-THE-SSH-STACK-AUTHENTICATED-NOBODY` failed both.

**The obvious way to write the second check does not work, and the failure is
instructive.** My first draft of it was a mention scan — crates that name a
password store but do not depend on `authlib`:

```bash
for d in userspace/*/; do
  grep -rqs '/etc/shadow\|/etc/users.yaml' "$d/src" || continue
  grep -qs authlib "$d/Cargo.toml" || echo "$d"
done
```

I wrote that it returns nothing. **It returns twenty crates** — `getent`,
`useradd`, `chage`, `chown`, `chroot`, `coreutils`, `audit`, `apparmor`,
`selinux`, `oils` and the rest. Naming the store is not verifying against it:
`getent` prints it, `useradd` writes to it, `chage` edits the aging fields, and
several only mention it in a doc comment. Twenty results with one true positive
buried in them is not a check, it is a thing you stop running by the third week.

Narrowing it — "reads a store *and* prompts for a password with echo off" —
cuts it to three (`doas`, `su`, `passwd`), and is **worse**, because it silently
drops `login`, `sudo` and `polkit`, all of which verify passwords. They were
dropped for spelling their prompt differently: the pattern looked for
`read_password`/`no_echo`/`ECHO` in the crate's own `src`, and those three reach
the terminal through a helper instead. A check that quietly halves its input is
more dangerous than one that over-reports, because its clean result reads as
reassurance. Any check keyed on how a call site is *written* has that hole, and
unlike your K-table extraction there is no published constant to compare
against, so the hole is not closeable by being more careful with the pattern.

**The check that does hold inverts it.** Do not ask which crates might be
verifying passwords; ask where the verification primitive is *called from*, and
require every answer to be a shared verifier:

```bash
grep -rn 'crypt::verify' posix/src userspace/*/src services init
```

This is tight for the same reason your check is: `posix::crypt::verify` is the
one function in the tree that can answer the question, so a crate that answers
it either appears here or is doing arithmetic of its own — which is then a
K-table-shaped problem again and your check catches it. Today, discounting
comments and `#[cfg(test)]` blocks, there are exactly three call sites:

| Call site | Why it is allowed to be there |
|---|---|
| `userspace/authlib/src/lib.rs` | the shared verifier; lock-out, rate limiting, both stores |
| `userspace/userdb/src/lib.rs` | `Record::check_password`, the shared verifier for callers that already hold a `Record` — `su`, `sudo`, `polkit` |
| `userspace/passwd/src/main.rs` | checks your *current* password before changing it (`main.rs:651`, behind the "Current password:" prompt) |

The third row is the one to keep an eye on. `passwd` does not go through
`authlib`, and that is not a decision anyone recorded — `passwd` was
centralised onto `posix::crypt::verify` in this request on 2026-08-17, days
before `authlib` existed, and simply never moved again. There is a good
argument for leaving it (authlib's failure tally is shared across programs, so
routing `passwd` through it would let someone lock you out of changing your own
password by mistyping at a `doas` prompt) and a good argument against (it is
the last program that answers the password question without the lock-out and
rate-limiting every other one gets). It is filed as
`known-issues.md` → `B-PASSWD-VERIFIES-WITHOUT-AUTHLIB` rather than settled
here, because it is a real tradeoff and this file is a footnote, not the place
to decide it.

`doas` was the only genuine holdout, and it would have shown up here on any day
since 2026-08-17 — which is the argument for running a check rather than
trusting that centralising a thing centralised it. Centralising does nothing
for a program that never looks, and no build fails when one does not. The six
crates that verify passwords today (`login`, `su`, `sudo`, `polkit`, `doas`,
`passwd`) were each read line-by-line on 2026-08-21; all six reach
`posix::crypt::verify` through one of the three rows above.

Filed here rather than as a new request because it is a footnote to your
report's method, not a new ask. Nothing needed from lane C.
