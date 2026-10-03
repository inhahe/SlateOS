## 1206. The address book is kept like the notes library, and the store counts its own changes

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The contacts app now keeps its address book -- every contact
with all its numbers, addresses, accounts and groups, the groups, and who was
looked at last -- in one file in the settings folder
(`contacts/address-book.txt`), written after every change, the way the notes
library is kept (§1205). Until now nothing was kept, and a vCard export was
the only way to keep anyone. vCard stays what it was: the way in from and out
to other programs (Ctrl+O, and Ctrl+E where it was Ctrl+S).

### Not vCard as the store

vCard is the address book's interchange format and the obvious candidate. It
cannot hold what this app keeps: a contact's groups (vCard's `CATEGORIES` are
names, and two groups can share one), a group's colour and description, the
star, when a contact was added and last reached, which of several numbers is
primary in a way every reader agrees on, and the recently viewed. Each of
those would be an extension (`X-SLATEOS-...`), and a store made of extensions
is a private format wearing a public one's name -- other programs would read
it and drop half of it without a word. The file is the notes library's kind:
tab-separated lines, `textfmt::tsv`'s escapes, the format named on the first
line, read whole or not at all.

### The store counts its changes

The notes app marks the library changed in each method that changes it; a
method that forgets is a change that is never written. The contacts store's
fields are private, so every change goes through one of its methods, and each
of those counts one in `ContactStore::revision` -- the two that hand out
`&mut` count whether or not the caller then changes anything. The window
writes the book after any event that moved the revision. A method added later
that forgets to count is still possible, but it is one place, next to the
field, not every call site in the app.

**Where it lives:** `apps/contacts/src/main.rs`: `book_path`, `book_text`,
`parse_book`, `ContactStore::{revision, changed}`, `ContactsApp::{from_settings,
load_book, take_store, keep, unkept, keeping_line, stamp, request_close}`.

**How to reverse:** the format is `book_text`/`parse_book`; the counting is
independent of it.
