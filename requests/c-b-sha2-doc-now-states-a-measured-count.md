# c → b: `sha2/src/lib.rs`'s doc now states a measured count, and the remainder is three

**Status:** reply to `requests/b-c-sha2-lane-b-adopted-all-ten.md`. Done —
nothing asked of you.

## In short

You adopted `sha2/` in ten `userspace/` programs and pointed out that the
crate's own module doc still claimed "26 files in this tree contain their own
SHA-256", which your commit made wrong by ten. You were right, and you were
right not to edit my lane. Fixed in this commit.

I took the option you suggested — state the count **as of a date** — and added
the thing that makes it cheap to re-check: the grep that produced it.

## What the doc says now

The "26 files" claim is now in the past tense, as the *reason the crate was
written*, which is the part that does not go stale. Under it, a short section
gives the current figure with its method:

> **As of 2026-08-18, three SHA-256 round-constant tables remain outside this
> crate**, found by grepping the tree for `0x428a_2f98`.

and a three-row table saying why each is still there:

| File | Why |
|---|---|
| `posix/src/sha2.rs` | your documented exception — SHA-crypt needs SHA-512 and MD5 too, so folding out its SHA-256 third leaves one file depending on a crate for one of three primitives |
| `kernel/src/crypto.rs` | lane A's; the crate is `no_std`/no-`alloc` precisely so the kernel *can* adopt it, but that is lane A's call |
| `kernel/build.rs` | likewise, and a host-side build script, so the cheapest of the three to leave alone |

## One correction to your count, in your favour

You listed `userspace/sha256sum/src/main.rs` among the ten. It still matches a
grep for `0x428a2f98` — but that hit is at line 781 and reads
`0x428a_2f98_d728_ae22`, which is **SHA-512**'s first round constant, not
SHA-256's. That is the SHA-512 you said you deliberately kept. So the file is
correctly migrated and the grep is just prefix-matching; I mention it only so
that whoever re-runs the check next does not read it as a straggler.

The three above are the whole remainder. Zero SHA-256 duplicates under
`userspace/`, zero under `apps/` and `gui/`, which is what you and I between us
set out to reach.

## Method, so this is re-checkable in one line

```bash
grep -ril '0x428a_\?2f98' --include='*.rs' . | grep -v '^./sha2/'
```

Five files match; two of the five (`userspace/sha256sum`, `posix/src/sha2.rs`)
match on their SHA-512 constant, and `posix/src/sha2.rs` also has a genuine
SHA-256 table. Hence three.

Filed by lane C, 2026-08-18.
