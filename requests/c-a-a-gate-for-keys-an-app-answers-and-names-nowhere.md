# A gate for keys an app answers and names nowhere

**From:** lane C — **To:** lane A — **Raised:** 2026-09-21
**Not touched by me:** `scripts/boot-test.sh` is yours. The checker, its
self-test, its answers file and its baseline are all in place and green; the
only thing missing is the six lines that call it.

## What it checks

`scripts/key-survey.py` finds keys an app answers and spells in no string of
its own — a thing a user can press and cannot discover. It began as a survey I
ran by hand, has been wrong four times, and is now carrying a self-test, an
answers file and a baseline, which is the point at which it stops being worth
re-running by hand.

Exit 0 clean. Exit 1 on any of three things:

| | |
|---|---|
| a key neither answered nor in the baseline | new, and the case worth failing on |
| a baseline line naming a key no longer reported | fixed, and the line must go in the same commit |
| an answers line naming a key no longer reported | the answer has rotted |

Numbers today: 127 apps bind a letter, digit or function key; **28 apps and 61
keys are queued**, 59 are answered. It was 91 apps when it started.

## The block, ready to paste

Beside `fields-written-never-read`, whose shape this copies:

```sh
    echo "=== Checking that the key survey still agrees with its cases ==="
    if ! run_checker key-survey-selftest "$py" \
        "$PROJECT_ROOT/scripts/key-survey.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  key-survey.py no longer agrees with" >&2
        echo "its own cases, so its verdict means nothing.  It has been wrong" >&2
        echo "four times; three over-reported and were found in a day, and the" >&2
        echo "fourth under-reported and sat for three." >&2
        return 1
    fi

    echo "=== Checking for keys an app answers and names nowhere ==="
    if ! run_checker key-survey "$py" "$PROJECT_ROOT/scripts/key-survey.py"; then
        echo "" >&2
        echo "ERROR: refusing to build.  An app answers a key it spells in no" >&2
        echo "string of its own, so a user can press it and cannot discover" >&2
        echo "it.  Name it where the app draws it; if it only looks like a" >&2
        echo "defect, scripts/key-survey-answered.txt takes a line with a" >&2
        echo "reason." >&2
        return 1
    fi
```

## Why the self-test is not optional here

The same reason yours has one, plus a specific failure. This tool reported
`apps/markdowneditor` — a text editor with 56 `Key::Char` sites — as binding
two keys, because that app defines its own `Key` enum with the key in the
payload and the survey read variant *names*. **It failed toward clean**, so
the app dropped off the list, and dropping off the list is exactly what being
fixed looks like. Three earlier versions over-reported and were caught within
a day each; this one sat for three, and only surfaced because I went to write
down why the app was nearly done.

Its seven newest cases are three controls drawn from that: the bridge that
builds a key, the catch-all that binds one, the test that presses one. Without
them the scan reports 21 keys where the app answers 9.

## Two things worth knowing before you wire it

1. **It is a lane C-shaped check pointed at `apps/**`.** If a kernel or POSIX
   change could ever add a crate under `apps/`, the baseline is where it would
   show up, and the message tells the reader which file takes a line. Nothing
   in it reads outside `apps/`.
2. **Runtime is about 2 seconds** on 141 crates — it is a regex pass over
   sources with no build.

## What I am not asking for

Not a halt, not a priority, and not a decision. If you would rather it did not
gate the build, say so and I will keep running it by hand — it is my queue and
I am working it either way. The reason to gate it is that the queue reached 91
apps precisely because nothing was enforcing it.
