### TD-OILS-LOCALVAR-INHERIT-IS-INERT. `shopt -s localvar_inherit` was listed but did nothing — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_local`.

**What:** the option was in osh's `shopt` inventory and could be turned on and
off, but no code ever read it, so a `local` always started fresh. bash 5.2.37
with the option on starts the new local as a *copy* of the binding it shadows:

```sh
shopt -s localvar_inherit
f() { local v; declare -p v; }
g() { local -i v=7; f; }; g     # bash: declare -i v="7"   osh: declare -- v
```

**Fixed 2026-08-04.** `declare_local` returns early, before the clearing, when
the option is on — leaving value, array kind and every attribute in place and
letting the declaration's own flags be applied on top of them, which is exactly
what the clearing was there to prevent.

Measured while implementing it, all bash 5.2.37:

- **"Previous scope" is just the live binding.** An outer frame's local, a
  global and a temporary-environment prefix all serve equally, and a name that
  is unset there — including one an inner `unset` took away — is inherited as
  unset. That makes the implementation a *deletion* rather than a lookup:
  osh's live tables already hold the innermost binding.
- **The nameref marking is the one exception.** `-n` is dropped and the value
  — the target's *name* — is kept as a plain string, so a `local -n v=t`
  shadowed by a bare `local v` yields `declare -- v="t"`.
- **The declaration's flags are added, not substituted.** `local -x v` under an
  outer `local -i v=7` reports `declare -ix v="7"`.
- **Inheritance precedes the declaration's own assignment,** so an inherited
  attribute converts it: under `local -i v=7`, an inner `local v=own` reports
  `v="0"` and `local v=3+4` reports `v="7"`; under `local -l v=x`, an inner
  `local v=ABC` reports `v="abc"`.
- **An inherited array kind still cannot be converted** (`local -A v` over an
  inherited indexed array is the usual `cannot convert indexed to associative
  array`), an inherited readonly makes a readonly local, and a readonly
  *global* is refused before any of this — all of which fell out unchanged.
- `declare` inside a function inherits too, being the same declaration;
  `declare -g` names the global and never shadows, so it cannot.

**Pinned by** `tests/corpus/a-local-can-be-told-to-inherit-what-it-shadows.sh`
and the `localvar_inherit_copies_the_binding_being_shadowed` unit test in
`src/interp.rs`.
