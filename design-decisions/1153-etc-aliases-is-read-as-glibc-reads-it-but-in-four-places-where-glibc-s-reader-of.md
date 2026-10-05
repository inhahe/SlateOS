## 1153. `/etc/aliases` is read as glibc reads it but in four places, where glibc's reader of it does what its readers of every other database do not

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** the mail aliases file says what an address such as
`postmaster` stands for. glibc's reader of it has four faults its readers of
the other databases do not share: a stray comma (`a: x,,y`) makes it loop
for good, so the program hangs; an entry too big for the caller's buffer is
lost instead of coming again with a bigger one; an indented entry after an
empty line can be listed but not looked up; and an `:include:` file that is
missing leaves an error code behind after a call that succeeded. This
library reads the file as glibc does in every other respect, and in these
four does what glibc does everywhere else.

| | glibc 2.39 | here |
|---|---|---|
| an empty member: `a: x,,y`, `a: ,x`, a carried-on line that starts with `,` | loops for good | passed over (`a` has `x` and `y`), as glibc's own `:include:` reading passes over one |
| `getaliasent_r` given a buffer too small for the next entry | `ERANGE`, and the entry is lost: the next call starts partway into it | `ERANGE`, and the same entry comes next time -- what glibc does for every other database |
| `getaliasent`, which retries with a bigger buffer, on an entry over 1 KiB | passes over it | gives it |
| `getaliasbyname` for an entry that begins with white space after an empty line | NULL, though `getaliasent` lists it | the entry |
| `errno` after enumerating past an `:include:` that cannot be opened | `open`'s error, the call having succeeded | as it was, as after any other enumeration |

**The alternatives:** reproduce all four, so that a typo in `/etc/aliases`
hangs whatever reads it and a mail system enumerating its aliases loses the
long ones without a word; or leave `<aliases.h>` out. POSIX does not
specify these functions and glibc's manual describes none of the four;
glibc's other readers show what its interface means. Everywhere else the
tests replay glibc's answers (`aliases_oracle.txt`), and in these four they
state the difference -- glibc's loops are in the oracle as `loops`, from a
five-second timeout.

**Not a difference:** with no `/etc/aliases`, a lookup fails with `ENOENT`,
as glibc's -- there is no built-in copy, as there is for `/etc/services`
(section 1113's reasoning is about registries, and aliases are a site's own).

**Where:** `posix/src/aliases.rs`.
