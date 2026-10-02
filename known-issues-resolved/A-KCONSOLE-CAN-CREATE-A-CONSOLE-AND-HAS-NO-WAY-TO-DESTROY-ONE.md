## `A-KCONSOLE-CAN-CREATE-A-CONSOLE-AND-HAS-NO-WAY-TO-DESTROY-ONE` (lane A, 2026-08-26) — **fixed 2026-08-26**

**In short:** The module that manages virtual consoles — the text screens you
switch between with a hotkey — could make new ones and had no operation for
getting rid of one. The list of them is capped at sixteen, so a session that
opened consoles for temporary work eventually could not open any more, and
there was nothing the user could do about it: the only way back was a reboot.

**Where.** `kernel/src/fs/kconsole.rs`. `create` (~line 188) was the only writer
of the `consoles` vector, and the cap it checks is the whole of the pressure:

```rust
const MAX_CONSOLES: usize = 16;

pub fn create(name: &str, console_type: ConsoleType, cols: u32, rows: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.consoles.len() >= MAX_CONSOLES {
            return Err(KernelError::ResourceExhausted);
        }
        ...
        state.consoles.push(Console { ... });
```

`init_defaults` seeds three, so a user got thirteen creations per boot, ever.
There was no `destroy`, no `remove`, no `close` — the module's entire public
surface was `init_defaults`, `switch`, `write`, `create`, `resize`, `active`,
`list`, `get`, `stats`, `self_test`. The shell command matched it exactly.

**Why it stayed invisible.** Nothing in the kernel calls `kconsole::create`.
The only caller is `cmd_kconsole`, i.e. a human typing at the shell, and the
gap does not bite until the fourteenth creation in one boot — which nothing had
ever done, because until this batch the boot battery did not create consoles
either. It surfaced only when `kshell::self_test` rung 95 needed to create one
to prove that omitting a bracketed operand still yields the documented default,
and then needed to put the table back the way it found it. This is the second
consecutive batch in which writing a rung that owns its fixture exposed a
missing teardown path in the module under test (see
`A-FDTABLE-HAS-NO-PROCESS-EXIT-PATH-AND-THE-TABLE-VECTOR-ONLY-GROWS`), which is
worth generalising: **a module whose only caller is an interactive command
tends to be missing precisely the operations no test ever needed.**

**Fixed.** `kconsole::destroy(id)` and a `kconsole destroy <id>` subcommand.
Two decisions inside it were not obvious and are recorded in the function's own
doc comment:

*The active console is refused, not silently vacated.* `destroy` returns
`DeviceBusy` when `id` is the active console, matching what Linux's
`VT_DISALLOCATE` has done with `EBUSY` for as long as virtual terminals have
existed. Destroying it instead would leave `active_id` pointing at nothing — a
state `init_defaults` can never produce and nothing can leave, since `active()`
would find no console and `switch` is the only writer of `active_id` and needs a
target that exists. Auto-selecting a survivor was rejected for the reason that
governs this whole burn-down: which console the user wants to be looking at is
not something the function can know, so picking one is a guess wearing the
clothes of a recovery. Switch first, then destroy.

*`next_id` is not rewound.* Ids are never re-issued, because `kconsole list` and
`/proc/kconsole` name consoles by id and a re-used number would silently rebind
an identifier the user had already read. That is deliberately the *opposite* of
the rule in `fdtable`, where descriptor numbers do restart at 0 after a process
exits — and correctly so, because there the whole table goes away with its
allocator and the numbers were scoped to a process that no longer exists. Here
the id space outlives every individual console.

`remove`, not `swap_remove`, so destroying one console does not silently
reorder the ones that survive in `kconsole list` and `/proc/kconsole`.

`kconsole::self_test` grew from 8 to 9 tests; test 9 asserts the active console
is refused, that the refused destroy freed nothing, that a destroyed console is
gone from both the count and `get`, that destroying it twice is an error rather
than a silent no-op, that the survivors are untouched, and that re-creating a
console under the freed *name* gets a strictly greater id. It is deliberately
placed after test 8 so the exact `count == 4` that test asserts stays meaningful.

**Not a regression.** True since the module was written.
