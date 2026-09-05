# B → C: a pending question would turn eighteen of your tests red

**Filed:** 2026-09-05 by lane B
**Status:** heads-up — **nothing has landed**, no action needed yet
**Touches:** `randrange/` (repository root; in no lane's owned *or* forbidden
globs, last changed by lane B on 2026-09-04 in `81033b2db`), and — if the
question is answered one way — about eighteen tests in `apps/**` and `gui/**`,
which are yours.

## In short

`randrange::fill_from_kernel` is hard-coded to refuse on any non-unix host, so
on the Windows development machine `seed_from_system` has always fallen back to
the caller's named constant and `SystemRandom::open` has always failed. I have a
finished, tested change that removes that: the host reads the Windows system
CSPRNG through `BCryptGenRandom`, and the "no entropy" case becomes a stand-in
filler that a test hands over — which means it is exercised **on the SlateOS
target too**, where it never was.

I have not landed it, and `lane-b` does not contain it. It is parked on branch
`lane-b-randrange-entropy`, and the decision is queued for the operator in
`open-questions.md` → *"The test machine cannot produce random numbers, on
purpose, and about eighteen tests in the apps now depend on that. Should it
start?"*

**The SlateOS build is unaffected either way.** `toolchain/x86_64-slateos.json`
sets `"target-family": ["unix"]`, so the target keeps the `getrandom` arm it has
always used; the new code is behind `#[cfg(windows)]` and is compiled only for
the test host.

## Why it is your business

Twenty-six `apps/` crates call `seed_from_system` or `seeded_from_system`, and
"this host cannot produce entropy" turns out to be a deliberate, repeated
testing convention in your tree rather than an accident. If the change lands,
these go red:

**Assert equality with the fallback constant:**

- `apps/dots` — `a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal`
- `apps/flashcards` — `a_fresh_app_is_seeded_by_the_system_and_not_by_a_literal`

**`#[cfg(not(unix))]` tests — they exist only on the Windows host, and their
subject is that the host declines:**

- `apps/wordle`, `apps/videoplayer`, `apps/radio`, `apps/memory`,
  `apps/lightsout`, `apps/match3`, `apps/pipes`, `apps/tetris`,
  `apps/musicplayer`, `apps/hangman`, `apps/speedtest`
- `gui/desktop/src/wallpaper.rs` (two)
- `gui/credentials/src/main.rs` — `the_shipped_generator_refuses_when_the_kernel_is_out_of_reach`
  and `the_request_path_refuses_to_set_a_password_without_entropy`

**Still pass, but their comments and mutation harnesses go stale:**
`apps/battleship` and `apps/freecell` assert *inequality*, so they survive — but
`apps/battleship/mutate.py`, `apps/freecell/mutate.py` and `apps/dots/mutate.py`
score mutants against the old assumption and will report newly-surviving
mutants.

## The replacement, if it comes to that

Every one of those tests has a strictly better form that works on **both**
platforms — assert the fresh value differs from a *named* seed rather than that
it equals the fallback:

```rust
// instead of: assert_eq!(fresh_game().seed, FALLBACK_SEED);
assert_ne!(fresh_game().seed, Game::with_seed(42).seed);
```

And where a test genuinely wants a fixed board, name the seed:
`SeededRng::new(SEED)`, not `seed_from_system(SEED)`. The second has never
promised to give you `SEED` — it promises a *fallback* — and under the change it
visibly does not.

`apps/battleship` is the reason this file exists rather than a one-line note.
`roadmap.md` records fault (15) of its window-wiring pass as "`randrange`'s
`seed_from_system` fallback was the very constant fault (3) used, so on any
machine with no kernel randomness to open the ships still stood exactly where
the bug put them — the fault moved rather than fixed, and invisible to every
test run off Slate OS." That is the dependency this change removes.

## What I would like from you

Nothing yet. If you have a view on whether the convention should stand, add it
to the `open-questions.md` entry — you own more of the affected code than I do,
and a note there from lane C would carry weight. If the operator picks the
option where lane C does the rewrite, I will file a follow-up request with the
exact file-and-line list from a full workspace run.

## Incidental

The change also clears four permanently-failing tests in `userspace/ssh` (the
Diffie-Hellman exponent and the KEXINIT cookie cannot be drawn on the host).
That is *not* why I need it, though — I am unblocking that in lane B by making
the SSH code take its byte source as a parameter, which it should have done
anyway, so item 4 of `known-issues.md`
→ `TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`
does not depend on this question being answered.
