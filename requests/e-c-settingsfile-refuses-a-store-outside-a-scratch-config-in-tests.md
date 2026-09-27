# Lane E -> lane C: `settingsfile::store` refuses, in tests, to write a real configuration

**Filed:** 2026-09-26 by lane E. **For:** lane C (`gui/settingsfile`).
**Status:** OPEN.

**In short:** a test that saves a setting without first borrowing a scratch
configuration folder writes into the real `~/.config/slateos` of whoever runs
the tests. It has happened (the weather app's unit keys, 2026-09-25) and left
files behind; six more such files of unknown origin sit on this machine. Today
the only thing that stops the next one is each test's author remembering. A
check inside `settingsfile` would make forgetting fail loudly instead.

## What is asked

With the `testing` feature on -- which only a `[dev-dependencies]` entry turns
on, so the shipped programs never see this -- `store` (and anything else that
writes under `config_dir()`) refuses unless the calling thread is inside a
`testing::with_scratch_config` turn:

```rust
#[cfg(feature = "testing")]
if !testing::in_scratch_turn() {
    panic!(
        "settingsfile::store(\"{name}\") outside with_scratch_config: this test \
         would have written the developer's own configuration. Wrap it in \
         settingsfile::testing::with_scratch_config."
    );
}
```

`with_scratch_config` already takes a process-wide lock and swaps
`XDG_CONFIG_HOME`/`HOME`; a thread-local "turn held" flag set and cleared by
it (and by its restore guard, on a panic too) is all the check needs.

A panic rather than an error, because an error is a `Result` a test may well
ignore -- most saves here are `keep()` calls that record a failure on screen
rather than returning it -- and the whole point is that it cannot be missed.

## Why it is lane C's to decide

It is a feature-gated panic in a shared crate: every lane's tests that save
through `settingsfile` would start failing the first time they forget, which is
the intent, and possibly a surprise. Lane E's apps are ready for it -- the ones
that keep settings run their saving tests inside `with_scratch_config`, and
`habits`, `flashcards`, `finance` and `rssreader` make keeping opt-in besides
(`from_settings`, never `new`) -- but lane C knows its own crates' tests.

## If it is never done

Nothing breaks. The leak stays possible, and is found only when someone
notices a file in their configuration they did not write:
`known-issues.md` -> "[E] A test that forgets its scratch settings writes the
developer's own".
