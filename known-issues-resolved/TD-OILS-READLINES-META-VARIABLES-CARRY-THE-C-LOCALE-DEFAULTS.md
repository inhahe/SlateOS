### TD-OILS-READLINES-META-VARIABLES-CARRY-THE-C-LOCALE-DEFAULTS. `"\M-\C-e": yank` binds the wrong key — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/bind_tables.rs` — the `VARIABLES` table, which
had `convert-meta on`, `input-meta off`, `meta-flag off`, `output-meta off`.

**What was wrong.** readline has *two* sets of defaults for those four and
picks between them at startup. The C declarations (readline.c:300 and
neighbours) are only the fallback; `rl_initialize` calls `_rl_init_eightbit`,
which asks `LC_CTYPE` and hands the answer to `_rl_set_localevars`
(nls.c:168–186). That goes eight-bit for **any** locale that is not exactly
`C` or `POSIX`:

```c
if (localestr && *localestr && (localestr[0] != 'C' || localestr[1]) && (STREQ (localestr, "POSIX") == 0))
  {
    _rl_meta_flag = 1;                    /* input-meta, meta-flag  on  */
    _rl_convert_meta_chars_to_ascii = 0;  /* convert-meta           off */
    _rl_output_meta_chars = 1;            /* output-meta            on  */
```

`C.UTF-8` takes that branch on the `localestr[1]` clause. osh had encoded the
C-locale set, so `convert-meta` was on and every meta binding was sent into the
escape sub-map instead of being stored as the eight-bit byte.

**Repro** (`INPUTRC` set to a file that binds nothing else):

```sh
printf '$if Bash\n"\\M-\\C-e": yank\n' > open
bind -f open
bind -q yank
#   bash: yank can be invoked via "\C-t", "\C-y", "\205".
#   osh:  yank can be invoked via "\C-t", "\C-y", "\M-\C-e".
```

`\205` is octal 0x85 = `META(CTRL('E'))`. `rl_translate_keyseq` builds that byte
either way (bind.c:619–638) and only splits it into `ESC` + `0x05` when
`convert-meta` is on; `_rl_get_keyname` then prints a byte ≥ 0x80 as a
three-digit octal escape and never as `\M-`, which is why the two spellings
differ (bind.c:2592–2660). osh's `bind_keys::encode` already had the octal rule
right — the binding was simply landing in the wrong map.

**Measured both ways, to be sure the locale is the whole story:**

| `LC_ALL` | `convert-meta` | `bind -q yank` shows |
|---|---|---|
| `C` | on | `"\M-\C-e"` |
| `C.UTF-8` | off | `"\205"` |
| `en_US.UTF-8` | off | `"\205"` |

A first attempt to attribute this to Git-for-Windows' `/etc/inputrc` (which
does carry `set convert-meta off`) was **wrong** and is recorded here so it is
not re-derived: that file is only reached when `INPUTRC` is unset *and*
`~/.inputrc` cannot be read (bind.c:1041–1062), and the divergence reproduces
with `INPUTRC` pointing at a file that sets nothing.

**Fixed** in three layers, because flipping the defaults alone was not enough.

1. **The defaults** (`bind_tables.rs`). `VARIABLES` now carries the eight-bit
   set — `convert-meta off`, `input-meta on`, `meta-flag on`, `output-meta on`
   — which is the branch osh is always in: design-decisions.md §104 settles
   that osh is UTF-8-only, so the C-locale set is unreachable. `byte-oriented`
   is not a second instance of this: it tracks `MB_CUR_MAX` and readline
   reports it `off` in both locales.

2. **`decode` did not model the variable at all** (`bind_keys.rs`). It treated
   `\M-` as an unconditional `ESC` push, so with the conversion off it still
   produced `ESC` `\C-e` rather than `\205`. It is now a transcription of
   `rl_translate_keyseq` (bind.c:523–648): `\C-`/`\M-` are *flags* gathered
   until they meet a character, applied control-first, and only afterwards is
   `META_CHAR (c) && _rl_convert_meta_chars_to_ascii` allowed to split the
   byte. `convert_meta` is a parameter rather than a read of the live table,
   because a `set convert-meta` in an earlier operand of the same `bind` call
   steers the operands after it. Two behaviours osh had never modelled came out
   of the rewrite and are now frozen in the corpus: `\M-x` and `\ex` are
   *different* bindings while the split is off, and a modifier that runs off
   the end of the string applies to the NUL that terminates it, so `"x\C-"`
   binds two keys (`x`, `\C-@`) and `"y\M-"` binds `y`, `\200`.

3. **`table_entry` reads a listing, not a key sequence** (`bind_keys.rs`).
   This one only surfaced once (2) was in: seeding the keymaps from the
   captured `bind -p` tables through `decode(text, true)` turned the capture's
   `\200` (self-insert at 0x80) into `ESC` `NUL`, which collided with the
   escape sub-map and left `bind -p` printing **11500** lines against bash's
   488, most of the keymap bound to `abort`. The dumper's dialect is not
   `rl_translate_keyseq`'s: `rl_invoking_keyseqs_in_map` (bind.c:2732–2741)
   writes `\M-` for exactly one thing, the sub-map at `ESC`, while
   `_rl_get_keyname` (bind.c:2592–2660) writes a high byte in octal and never
   writes `\M-`. **readline does not round-trip its own listing here** — feed
   `"\200"` back to `bind` with `convert-meta` on and it really does bind `ESC`
   `NUL`. So `table_entry` now rewrites the capture's `\M-` to the `\e` it
   names (with an escape-aware scan, so a literal `\\M-` is left alone) and
   decodes with `convert_meta = false`. The `\000` prefix marker comes off
   *after* decoding rather than before, since `\M-\000` — the escape sub-map's
   own binding — is not two halves that each stand on their own.

**Verified** with `bind -p`/`-P`/`-s`/`-v` over all five keymap names under
`C.UTF-8` and `en_US.UTF-8`, byte-identical to bash, plus the 14 existing
`bind` corpus cases and a new one,
`a-meta-key-is-a-byte-until-convert-meta-splits-it`. The `LC_ALL=C` runs still
differ, by exactly the four variables and the `\M-`-versus-`\e` spelling they
imply — that is §104 working as designed, not a defect.

**Found by** the first full corpus sweep under `LC_ALL=C.UTF-8`
(TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE, resolved by
the operator's Q38 answer) — this is that change's first real fallout, and it is
a genuine osh bug the `C` locale had been masking.
