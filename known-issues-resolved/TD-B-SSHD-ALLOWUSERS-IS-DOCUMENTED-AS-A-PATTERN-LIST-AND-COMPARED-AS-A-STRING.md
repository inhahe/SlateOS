## TD-B-SSHD-ALLOWUSERS-IS-DOCUMENTED-AS-A-PATTERN-LIST-AND-COMPARED-AS-A-STRING (lane B) -- FIXED 2026-09-05

**Status:** FIXED — 2026-09-05 (filed and fixed within the hour; see "How it
was fixed" at the end)

**In short:** `sshd_config` lets an administrator write `AllowUsers admin*` to
mean "any account whose name starts with `admin`". Ours compares the whole line
literally, so that setting matches an account *named* `admin*` — which cannot
exist — and therefore locks everybody out. The same is true of `DenyUsers`,
`AllowGroups` and `DenyGroups`. The failure is silent: the daemon starts, the
configuration parses, and logins are refused with the ordinary "not permitted"
message.

**Where it lives.** `userspace/sshd/src/main.rs`, the access check:

```rust
if !config.allow_users.is_empty() && !config.allow_users.iter().any(|u| u == username) {
    return false;
}
```

and the three sibling comparisons beside it. All four use `==`.

**Which direction it fails in.** Both, depending on the directive:

| Directive | A pattern that should… | …actually |
|---|---|---|
| `AllowUsers admin*` | let `admin1` in | locks *everyone* out (fails closed) |
| `DenyUsers guest*` | keep `guest1` out | lets `guest1` **in** (fails open) |

The `Deny` half is the serious one: an administrator who wrote a pattern
believes an account is blocked and it is not. There is no warning, because a
pattern is a perfectly valid literal name as far as the parser is concerned.

**What the proper fix is.** The matcher already exists — `pattern_list_matches`
was written for `AcceptEnv` in the same file, with `*`, `?` and `!` negation
and OpenSSH's rule that a negated match wins outright. Replace the four `==`
comparisons with it, and add tests for both failure directions above.

Note the one behaviour change this makes: a configuration that today contains
`AllowUsers admin*` currently denies everybody, and afterwards will admit the
`admin*` accounts. That is the directive doing what its name and every
`sshd_config(5)` say — the present behaviour is not a stricter policy, it is a
broken one.

**Cost while unfixed:** any `sshd_config` written by someone who knows OpenSSH
does something other than what it says. Nothing in this tree ships such a
config yet, which is the only reason this is not urgent.

**If never fixed:** the first real deployment that writes a `Deny` pattern gets
an access-control rule that quietly does nothing.

### How it was fixed — 2026-09-05 (lane B)

All four comparisons now go through `pattern_list_matches`, the matcher written
an hour earlier for `AcceptEnv`. `AllowUsers admin*` admits the admins;
`DenyUsers guest*` blocks the guests; `AllowGroups dev-*` and `DenyGroups no-*`
likewise; and `!` negation carries over, so `AllowUsers dev* !dev-intern` says
the thing it looks like it says.

The `Deny` side lost its `is_empty()` guard rather than gaining a pattern one:
an empty pattern list matches nothing by construction, so the guard was
restating the matcher's own base case in a second place where it could later
disagree with it.

**Tests, one per failure direction** — `an_allowusers_pattern_admits_the_accounts_it_names`
(was fail-closed: locked everyone out) and
`a_denyusers_pattern_actually_blocks_the_accounts_it_names` (was fail-open: let
the named accounts in), plus `group_patterns_match_on_both_sides` and
`a_negated_pattern_carves_an_exception_out_of_an_allow_list`. 184 tests pass on
the host and under WSL's linux half; clippy clean on both.

The behaviour change flagged above is real and intended: a configuration
containing `AllowUsers admin*` used to deny everybody and now admits the
`admin*` accounts. That is the directive doing what it says — the previous
behaviour was not a stricter policy but a broken one.
