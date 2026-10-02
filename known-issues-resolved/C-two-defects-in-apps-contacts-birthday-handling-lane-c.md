## Two defects in `apps/contacts`' birthday handling (lane C)

**Status: both FIXED 2026-08-16.** Each verified by reverting it alone and
watching the specific tests fail.

The one production warning was `days_before[m]` in `day_of_year`, guarded by
`.min(11)` against a twelve-element array — safe, but *the bound written
twice*. What made it worth pulling on is what the table sat next to:
`SimpleDate::new` validated the day against a flat `1..=31` with no reference
to the month at all. So the crate stated the calendar twice, in two
incompatible forms, and the two disagreed.

- **31 February was an acceptable birthday.** `SimpleDate::new(2026, 2, 31)`
  returned `Some`, and `parse` goes through `new`, so the date came in from a
  hand-typed field or an imported vCard, was stored, and was displayed straight
  back as `2026-02-31` — nothing anywhere said it was impossible. It also
  counted as a day of the year past the real end of February (59 + 31 = 90,
  where 1 March is 60), so the "upcoming birthdays" list placed it thirty days
  from where the person reading it would expect. Fixed by checking the day
  against *that month's* length, taken from the same table `day_of_year` uses,
  with the Gregorian leap rule applied so that 29 February 1988 is accepted and
  29 February 1989 and 1900 are not.
- **vCard's basic date form imported as no birthday at all.** `parse` split on
  `-` and required exactly three parts, so it accepted `1990-12-25` and
  rejected `19901225`. The second is not an edge case: **vCard 4.0 specifies
  the basic form**, and it is what phones and mail clients export, so importing
  a real address book dropped birthdays — silently, and indistinguishably from
  a card that never carried one. Now both spellings parse to the same date.
  Export still writes the extended form, which every reader accepts.

The structural fix behind both is one table, `MONTH_LENGTHS`, used for the
validation *and* for the day-of-year count, with `day_of_year` reaching it
through `.iter().take(month - 1)` rather than an index — the months before this
one are exactly the ones the iterator yields, so a month past December can only
mean "all of them" and the `.min(11)` second copy of the bound is gone. A new
property test asserts the first of each month is one past the last of the month
before, which is what a single table makes true by construction.

**Still not accepted, deliberately: vCard's year-less `--MMDD`** ("I know the
day but not the year"), which real cards do carry. That one is not a parse bug
but a missing representation — `SimpleDate` has a non-optional `year`, and a
sentinel year would leak into `format_display`, the edit field and the sort
order. Recording it here rather than fixing it in a lint sweep; the fix is an
`Option<u16>` year with a display form of `--MM-DD`.

Two further notes from the same read, neither a defect today:

- `is_upcoming_within` takes `today_month`/`today_day` as raw `u8`s rather than
  a `SimpleDate`, so the "today" side is unvalidated while the birthday side is
  validated by construction. Left alone because there is no clock in the system
  to feed it and its only non-test caller, `ContactStore::upcoming_birthdays`,
  is itself uncalled — changing the signature now would be churn against an API
  nobody has used yet.
- `day_of_year` stays leap-agnostic on purpose, and the reason is now in its doc
  comment: it compares a birthday against today, and those fall in *different*
  years, so applying each side's own leap rule would move one and not the other.
  A uniform 365-day year keeps the pair comparable, and one day of slack is
  inside the tolerance the question is asking about.

### Sweep progress: `pdfviewer` 94 → 0, all lint classes (2026-08-16)

Eleventh crate, sixth to reach **zero warnings of every class** across
`--all-targets`, and the last of the five crates listed as outstanding in the
`textfind` note near the top of this file. Tests 110 → 118.

The split is worth recording because it is unlike the other ten: of 94
warnings, **67 were in the test module** and only 27 in production code, and
all 27 were `arithmetic_side_effects` — not one `indexing_slicing` in shipping
code. The crate is a plain data model over `Vec<PdfPage>` and it already goes
through `get()` almost everywhere. The defects were therefore not where the
warnings were, which is the usual finding by now: four of the five below are in
a subsystem that produced five warnings between them.
