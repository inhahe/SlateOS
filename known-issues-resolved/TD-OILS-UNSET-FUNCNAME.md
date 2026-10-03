### TD-OILS-UNSET-FUNCNAME. `unset FUNCNAME` does not stop osh re-materialising it — 2026-08-01 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::refresh_funcname`, which
rebuilds `FUNCNAME` (and `BASH_SOURCE`/`BASH_LINENO`) into `Shell::arrays` from
the live call stack on every function entry and exit, with nothing recording
that the script asked for the name to go away.

**What.** bash lets `FUNCNAME` be unset, and unsetting it is permanent: the
dynamic behaviour is destroyed for the rest of the shell, leaving an ordinary
name that no longer tracks the call stack. osh keeps rebuilding it.

```
$ bash -c 'unset FUNCNAME; f(){ echo "[${FUNCNAME[0]-U}]"; }; f; FUNCNAME=x; f'
[U]
[x]
$ osh -c '...same...'
[f]
[f]
```

Two ends of it agree already: at the top level `${FUNCNAME+s}` is empty in both,
and `declare -p FUNCNAME` says `not found` in both. The divergence shows only
*inside* a function after the `unset`. `PIPESTATUS` is not affected (bash
re-materialises that one too, and osh matches); `BASH_SOURCE`/`BASH_LINENO`
refuse to be unset at all in both.

**Fixed in `58f03723d`.** The set of retired dynamic names the fix above called
for already existed under another name — `Shell::dyn_unset`, which the
`DYNAMIC_SPECIALS` machinery keeps for exactly this purpose and which `unset`
already writes to. `refresh_funcname` simply was not consulting it, so the fix
was one condition:

```rust
// A `FUNCNAME` whose binding `unset` dropped stays dropped, frames or
// no frames.
let in_function = !self.fn_stack.is_empty() && !self.dyn_unset.contains("FUNCNAME");
```

Nothing else had to change: an explicit assignment afterwards is a plain write to
the now-ordinary name, which is bash's behaviour and was already osh's, and
`BASH_SOURCE`/`BASH_LINENO` refuse the `unset` in the first place so they never
enter the set. Covered by the corpus case
`a-a-funcname-is-present-and-empty-outside-a-function.sh` (the `unset lets
FUNCNAME go and refuses the other two` section, and the ordinary-variable section
after it) and by the lib test
`funcname_is_present_and_empty_outside_a_function`. The report's own reproducer
is now byte-identical between the two shells.

**Impact.** A script that unsets `FUNCNAME` and then reads it inside a function.
Rare — `unset FUNCNAME` is nearly always a mistake — and independent of
`set -u`, which reports the name identically either way.
