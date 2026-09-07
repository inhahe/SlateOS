# B → C — `apps/lockscreen` calls two `authlib` items I deleted; `main` is red until it is fixed

**Filed:** 2026-09-07 by Lane B.
**Status:** OPEN — needs ~15 lines in `apps/lockscreen/src/main.rs`, which is yours.
**Severity:** `main` does not build. The boot test builds the whole workspace, so
this blocks all three lanes, and it is my fault, not yours.

## What I broke

`design-decisions.md` §353 item 3 says `authlib`'s `/etc/shadow` store is to be
**deleted**, not kept as a fallback — because a fallback that fires means the
generation broke, and admitting someone on the strength of a stale account file
is worse than refusing. I did that in `5264cba7a`. Two things went with it:

| Removed | Replacement |
|---|---|
| `authlib::shadow` (the whole module: `Entry`, `parse_line`, `lookup`, `lookup_in`) | Nothing. `/etc/users.yaml` is the only store. |
| `Authenticator::with_stores(users_yaml, shadow)` — second parameter | `Authenticator::with_stores(users_yaml)` |

I converted every caller I could find and missed two, because I built my list by
grepping `userspace/*/Cargo.toml` for `authlib` — which does not cover `apps/`
or `init/`. `init/login` is mine and is fixed (`e23e87865`). This one is yours.

## The two errors, verbatim

```
error[E0433]: cannot find `shadow` in `authlib`
    --> apps\lockscreen\src\main.rs:2110:20
2110 |     match authlib::shadow::lookup(shadow, username) {

error[E0061]: this function takes 1 argument but 2 arguments were supplied
    --> apps\lockscreen\src\main.rs:2019:20
2019 |             inner: authlib::Authenticator::with_stores(users_yaml, shadow),
```

## The fix

Three edits, and a fourth if you want the comments to stop describing a store
that no longer exists.

**1. `SystemAuthority::with_stores` (line ~2017) loses its second parameter.**

```rust
    pub fn with_stores(users_yaml: &Path) -> Self {
        Self {
            inner: authlib::Authenticator::with_stores(users_yaml),
        }
    }
```

**2. `account_has_password` (line ~2104) loses its `shadow` parameter and its
fallback branch.** The function becomes the native lookup plus the final
"unknown user → assume a password" case, which is the one deliberate divergence
from `authlib`'s `resolve` and should stay exactly as it is:

```rust
fn account_has_password(users_yaml: &Path, username: &str) -> bool {
    if let Ok(db) = userdb::UserDb::load(users_yaml)
        && let Some(record) = db.find(username)
    {
        return record_has_password(record);
    }
    // Unknown user: assume a password, so the prompt is shown and the
    // authority — not this screen — decides. See the doc comment above.
    true
}
```

**3. The test fixtures.** `store_with` returns a third element that is a path to
a shadow file "that deliberately does not exist"; it has no meaning now. The
call sites that need touching are, by line: `3605-3606` (the fixture itself),
`3635`, `3649`, `3664`, `3679`, `3689`, `3708`, `3717`, `3735-3738`, `3746`,
`3749`, `3759-3760`, `3765`.

Two of those tests are worth keeping *as tests*, with the shadow half dropped
rather than the test deleted:

- the one at ~3727 whose comment says "This is the one that matters" — it
  asserts that a lock screen which cannot read the store still prompts rather
  than concluding "no password". That property is unchanged and more important
  now, since there is only one store to fail to read.
- the one at ~3759, unknown user → still prompts.

**4. Comments that now describe a store that is gone** (optional, but they will
mislead the next reader): lines ~2084-2099 explain the two-store precedence and
say it "mirrors `authlib`'s private `resolve` on purpose — native database
first, `shadow(5)` second". `resolve` now reads one store, so the mirroring
claim is still true but the description of it is not. Same for ~2156, ~2168-2170
and ~2277.

## Why I did not just fix it myself

The lane rule, and one specific risk it exists for: `apps/lockscreen/src/main.rs`
is 3600+ lines and actively worked, so if you have uncommitted edits in it right
now my write would silently clobber them. That is the failure the ownership
boundary is for, and it is worse than a red `main` for another hour.

If you would rather I made the change, say so and I will — it is my breakage and
I am not trying to hand you work I caused. I have deliberately not touched the
file.

## What I have done on my side

- `init/login` fixed (`e23e87865`).
- Swept my whole lane: no two-argument `with_stores` and no `authlib::shadow`
  reference remains in `posix/`, `userspace/`, `services/` or `init/`.
- The list of every crate in the tree that depends on `authlib` is: `apps/lockscreen`,
  `init/login`, and `userspace/{doas,ftpd,login,logind,passwd,polkit,sshd,su,sudo}`.
  All but the first are green.
