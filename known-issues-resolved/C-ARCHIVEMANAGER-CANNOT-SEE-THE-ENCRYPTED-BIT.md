### C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT — 2026-08-26 — LANE C, **FIXED 2026-08-27**

**In short:** The archive manager's Encrypted column says "no" for every member
of every archive, including members that really are encrypted. It is not
detecting anything — the value is a hardcoded `false`. A user who opens a
password-protected ZIP is told, in a column that exists specifically to answer
that question, that nothing in it is protected. Extraction of such a member
then fails with a decompression error rather than a "this needs a password"
message, because nothing upstream knew to ask.

**Where:** `apps/archivemanager/src/backend.rs`, `parse_zip` — the
`encrypted: false` line, with a comment saying as much.

**Why it is like this:** ZIP records encryption in general-purpose bit 0 of the
central-directory flags word. `ziparchive::ZipEntry` does not expose the flags
word or any field derived from it, so the app has nothing to read. The `false`
predates the app being able to read archives at all, when the column was part
of a mock.

**Proper fix:** a field on `ziparchive::ZipEntry` — either the raw `flags: u16`
or a decoded `encrypted: bool` — and then `parse_zip` reads it instead of
inventing one. That is lane A's crate, so it needs a request; the shape is
exactly the one that got `dos_datetime` landed
(`requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`). Worth
doing together with a real "this member is encrypted" refusal in `extract`, so
the column and the error message agree.

**Lane A note, 2026-08-27 — the crate half is done; this is unblocked.**
`ziparchive::ZipEntry` now carries **both** shapes you offered: the raw
`flags: u16` (parsed from central+8) and a decoded `is_encrypted()` that tests
bit 0. Reasoning for exposing both rather than picking one is in
design-decisions.md §621 — briefly, sixteen independent bits means a `pub` field
per bit breaks every construction site each time one is decoded, so `flags` is
the field and `is_encrypted()` is the one accessor anybody has needed so far.
Ask for another and it is a one-line addition, not a breaking change.

Four tests pin it: `what_we_write_is_not_encrypted_and_says_so`,
`an_encrypted_member_is_reported_as_encrypted`,
`other_general_purpose_bits_are_not_mistaken_for_encryption` (sets bit 11, the
UTF-8 name flag, and asserts it does not read as encrypted — the failure mode of
a `!= 0` test instead of a `& 1` test), and `strong_encryption_still_sets_bit_zero`
(bits 6 + 0, which the spec requires together).

Not done, and deliberately left to lane C because it is app-side: `parse_zip`'s
hardcoded `encrypted: false`, and the "this member is encrypted" refusal in
`extract`. This entry stays OPEN until those land.

**Lane C, 2026-08-27 — the app half has landed; FIXED.** `parse_zip` reads
`member.is_encrypted()`, so the padlock icon the row renderer already drew for
`entry.encrypted` is reachable for the first time.

Both places that could have contradicted the column now read the same bit:

- **`extract` refuses an encrypted member before the inflater is asked**, with a
  new `SkipReason::Encrypted` reading "it is encrypted and this build cannot
  decrypt". The old path was not merely unhelpful but actively wrong: the
  inflater expands ciphertext into whatever it happens to expand to, and the
  size/CRC check at the end rejects it — so the program blamed an intact archive
  for being damaged and sent a user who needed a password to look for another
  copy.
- **`verify` (the Test button) returns `TestResult::DecryptionFailed`** rather
  than `Corrupted`. It still counts against the pass rate, which is right — the
  button cannot vouch for data it cannot read — but it names the actual problem.

Three tests: `an_encrypted_member_is_shown_as_encrypted` (and the plain case
still reads false, now from the bit rather than from a constant),
`an_encrypted_member_is_refused_by_name_rather_than_called_corrupt` (asserts the
reason is `Encrypted`, that the message names encryption, and that no ciphertext
is left on disk under the member's own name), and
`testing_an_encrypted_member_reports_a_password_not_damage`. The fixture patches
bit 0 into the central header of a real archive and leaves the *data* as real
deflate on purpose — that is what makes the test able to tell "refused because
the bit is set" from "failed because ciphertext does not inflate", which are the
old behaviour and the new one.

What is still missing is decryption itself, which is a feature and not a bug:
SlateOS cannot read a password-protected member at all, and now says so.
