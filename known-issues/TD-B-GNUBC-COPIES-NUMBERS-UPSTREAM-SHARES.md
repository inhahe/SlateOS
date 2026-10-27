## TD-B-GNUBC-COPIES-NUMBERS-UPSTREAM-SHARES (lane B, 2026-10-16)

**Status:** OPEN

**In short:** bc gives the same answers as Ubuntu's, but on very long
numbers it does more copying than it needs to. Each time it reads a
variable, uses a value on its stack or stores one, it copies every digit,
where upstream's bc shares the number and only counts how many places hold
it. On a loop over a 5000-digit number that is several copies of 5000
digits per instruction that upstream never makes.

**Found by:** the Opus review of 9454a6359..44c61f513 (finding 10).

**Where:** `userspace/gnubc/src/exec.rs`: `top` and `second` clone the
values they return, `binary` takes both operands by value before popping
them, and `load_var`, `D`, the store into `last` in `W` and `P`, array loads
and by-value array parameters each clone a `bcnumber::Num`.

**The proper fix:** keep the machine's numbers as `Rc<Num>` -- the execution
stack, the variables and the array elements -- as dc's port already does,
so that a load or a push shares the number and only an operation's result is
new; and pop an operation's operands rather than copy them. `bc-diff.sh` and
the machine's tests hold the output; a benchmark of the loop above shows the
difference.
