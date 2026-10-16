## Resolved — lane D

*(Lane D was created on 2026-09-22; its first question was **D-Q3**, because
D-Q1 and D-Q2 are the old names of `deferred-questions.md`'s DQ1 and DQ2 and
are never reissued.)*

- D-Q4 [where do background services keep the passwords they need before anyone signs in?] -- resolved 2026-10-09, **B: a system section of the password manager, unlocked at startup with a key the TPM keeps** (§1187).
- D-Q5 [Chinese, Japanese and Korean conversion tables: built into every program, or files?] -- resolved 2026-10-09, **B: table files in the image, mapped on first use** (§1188).
- D-Q6 [the C library's translations of glibc: keep under the LGPL, or rewrite?] -- resolved 2026-10-09, **B: rewrite them clean-room; glibc is the oracle, never the source** (§1189).
- D-Q7 [the C library says ASCII and converts UTF-8: which?] -- resolved 2026-10-09, **Claude's recommendation: one byte a character in "C", `C.UTF-8` as the user's setting** (§1190).
- D-Q8 [should `mlock` say it locked while the kernel could not?] -- resolved 2026-10-09, **B: say it could not**; the kernel locks since 2026-10-07, so the library asks it, and only a kernel without the call is told "could not"; the swap question back is answered in §1191 (a swap file by default, a partition at install, lane A building both).
