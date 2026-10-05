## TD-PRINTF-BUILDS-THE-WHOLE-FIELD-IN-MEMORY (lane B, 2026-08-17) — **open, low priority**

**In short:** `printf '%2147483647d' 5` asks for a number padded out to two
billion characters. GNU prints it, slowly, a chunk at a time, using almost no
memory. Ours would try to build the whole two-gigabyte line in memory first,
and on a machine without two spare gigabytes it fails instead of printing.
Nobody types this on purpose; a script computing a width from data could.

**Where:** `coreutils::cfmt::pad` and `cfmt::integer` return a `Vec<u8>`
holding the finished field, and `extfloat::render` returns a `String`. The
width has already been bounded to `INT_MAX` by `MAX_FIELD` in
`userspace/coreutils/src/bin/printf.rs` — anything wider is the `write error`
case described above — so the exposure is exactly the range 1 byte to
`INT_MAX`.

**Reproduce:** `printf '%2147483647d' 5`. GNU streams it; ours allocates. Do
not add it to `printf-cases.py`: it is a *legal* input, so the reference would
faithfully write two gigabytes into the record file.

**The proper fix:** give `cfmt` a `render_to(&mut impl Write, …)` that emits
padding in fixed-size chunks rather than materialising it, and let `render`
keep its `Vec` signature by calling it. `printf` and `seq` both write to a
`BufWriter` already, so neither call site changes shape. This also removes the
one place where a caller-chosen number decides an allocation size, which is
worth doing on its own account.

**Why it is not urgent:** the answer is never *wrong*, only expensive, and
`MAX_FIELD` already rules out the case that used to hang outright — a `*`
width of `INT_MIN`, which allocated two gigabytes and stopped responding
(fixed 2026-08-17, and now a unit test plus a differential case).
