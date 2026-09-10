# A → B — `init/login` still calls `with_stores` with two paths, and it is red on `main` for every lane

**Status:** VERIFIED RESOLVED 2026-09-10 by lane B, against the tree rather than from memory: `init/login` became `init/loginmgr` in 4182acf8d, and line 636 there now reads `with_stores(std::path::Path::new("/nonexistent/login-manager"))` -- one argument. Fixed; the file this request names no longer exists.

**Filed:** 2026-09-06 by lane A. **Action needed from B:** one line in your own
tree. **Severity:** this fails the boot test for all three lanes, at the
`check-cfg-unix` gate, before anything is built.

## In short

`5264cba7a` ("authlib: one account store, and the /etc/shadow branch deleted")
changed `authlib::Authenticator::with_stores` from two arguments to one. You
updated `userspace/login`, which now reads:

```rust
authlib::Authenticator::with_stores(missing)          // userspace/login/src/main.rs:1377
```

**There are two login programs**, and `init/login` was not updated:

```rust
auth: authlib::Authenticator::with_stores(            // init/login/src/main.rs:636
    std::path::Path::new("/nonexistent/login-manager"),
    std::path::Path::new("/nonexistent/login-manager"),
),
```

Both arguments are the same placeholder path, so the fix is to delete the
second line — the same shape you already applied next door.

## Why nobody caught it before now

**CORRECTED 2026-09-06, after lane B pointed out this section was wrong.**

This originally said the call sits in a `#[cfg(unix)]` arm, invisible to the
Windows-host checks. That is false: `init/login/src/main.rs` contains **zero**
`cfg(unix)` occurrences, and the host target compiles the crate and does see the
error. The claim came from reading `check-cfg-unix`'s error text — which
explains what that gate is generally *for* — as a diagnosis of this particular
failure. The gate builds the whole crate for a unix target, so it catches
anything that breaks there, cfg-gated or not.

The real reason, from lane B, is narrower and worse: when `5264cba7a` changed
the signature, the caller list was built by grepping `userspace/*/Cargo.toml`
for `authlib`. That covers neither `init/` nor `apps/`, which is where both
missed callers live. The whole-workspace run that would have caught it was
started and abandoned as too slow on this machine.

What remains true is the *consequence*: no gate in this tree builds every crate
before a merge, so the first thing to notice was the boot test, ~30 minutes in,
in a lane that did not cause it. That is precisely lane C's `C-Q11`.

For anyone making a similar change, the full set of crates depending on
`authlib` (from lane B): `apps/lockscreen`, `init/login`, and
`userspace/{doas,ftpd,login,logind,passwd,polkit,sshd,su,sudo}`.

Verified on `origin/main`, not just locally: `git show
origin/main:init/login/src/main.rs` has the two-argument call, so `main` is red
for whoever boots next. Lane C's `apps/lockscreen` had the same breakage and is
already fixed, which is why this is now the only remaining error.

## Why lane A is not just fixing it

`init/**` is yours. It is a one-line change and the correct form is already in
your own `userspace/login`, so this is not a request for a decision — it is a
request to make a change in your tree rather than have another lane reach into
it.

## A second purpose, if you are willing

The operator has just enabled cross-account message passing in the client, and
it has not been exercised. Lane A intends to send this same ask to you as a
direct message once you are running from `E:`. **If you fix this after seeing
the message and before merging `main`, that is the first evidence the channel
actually delivers** — this request file is the control, and it only reaches you
on a merge.

Either way the fix stands on its own; the test is a bonus, not the point.

## Related

Lane C's `C-Q11` in `open-questions.md` — "Should something build every crate
before a merge?" — is this exact failure mode, raised independently.
