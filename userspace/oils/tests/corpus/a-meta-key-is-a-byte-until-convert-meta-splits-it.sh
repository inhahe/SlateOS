# `\M-` is a *bit*, not a prefix — and `convert-meta` decides whether readline
# then splits the byte carrying it back into `ESC` plus the rest.
#
# `rl_translate_keyseq` (bind.c:523-648) gathers `\C-` and `\M-` as two flags
# and applies them to the character they run into, control first:
#
#     if (has_control) c = (c == '?') ? RUBOUT : CTRL (_rl_to_upper (c));
#     if (has_meta)    c = META (c);                      /* c |= 0x80 */
#     if (META_CHAR (c) && _rl_convert_meta_chars_to_ascii)
#       { array[l++] = ESC; array[l++] = UNMETA (c); }
#     else
#       array[l++] = c;
#
# Three things fall out of that, all of them visible here:
#
#   * The two modifiers commute. `\M-\C-g` and `\C-\M-g` are the same key,
#     because neither is a prefix — both are flags waiting for a character.
#   * `\M-x` and `\ex` are the same binding only while the split is on. With it
#     off they are two: one byte `0xf8`, and two bytes `ESC x`.
#   * A modifier that runs off the end has nothing to modify but the NUL the
#     string ends in — the loop condition is `(c = seq[i]) || has_control ||
#     has_meta`, so it runs one past the end — and the binding is *two* keys.
#     `"x\C-"` is `x` then `\C-@`, and `"y\M-"` is `y` then `\200`.
#
# `convert-meta` is not a thing a script normally sets: readline decides it
# from the locale. `_rl_set_localevars` (nls.c:168-186) goes eight-bit for any
# locale whose name is not exactly `C` or `POSIX` —
#
#     if (localestr[0] == 'C' && localestr[1] == '\0') ... /* seven-bit */
#     else { _rl_meta_flag = 1; _rl_convert_meta_chars_to_ascii = 0;
#            _rl_output_meta_chars = 1; }
#
# — so every UTF-8 locale, `C.UTF-8` included, gets `convert-meta off` and the
# other three on. That is a *startup* decision, and `bind` reaches it even in a
# non-interactive shell: `bind` calls bash's `initialize_readline`
# (bind.def:134-135), which calls `rl_initialize`, which calls
# `_rl_init_eightbit`. So the defaults below are the locale's, not a table's.
#
# The listing side has its own dialect and does not invert the above.
# `rl_invoking_keyseqs_in_map` (bind.c:2732-2741) writes `\M-` for exactly one
# thing — the sub-map hanging off `ESC`, and only while `convert-meta` is on —
# while `_rl_get_keyname` (bind.c:2592-2660) writes a byte with the eighth bit
# set in octal and never writes `\M-` at all. So with the conversion off, a
# meta key lists as `\205`, and `ESC`-prefixed sequences list as `\e...`.
export INPUTRC=/dev/null

echo "=== the locale picked the eight-bit set"
bind -v | grep -aE 'convert-meta|input-meta|meta-flag|output-meta'

echo "=== the two modifiers commute, and neither is a prefix"
bind '"\M-\C-g": yank' ; bind -q yank
bind '"\C-\M-g": kill-line' ; bind -q kill-line

echo "=== with the split off, \\M-x and \\ex are two different bindings"
bind '"\M-x": upcase-word'
bind '"\ex": downcase-word'
bind -q upcase-word
bind -q downcase-word

echo "=== a modifier with nothing to modify takes the NUL that ends the string"
bind '"x\C-": beginning-of-line'
bind '"y\M-": end-of-line'
bind -q beginning-of-line
bind -q end-of-line

echo "=== turning the split on makes them one binding again"
bind 'set convert-meta on'
bind -v | grep -a 'convert-meta'
bind '"\M-w": transpose-words'
bind -q transpose-words
# ... and the same sub-map now lists as \M- rather than \e.
bind -q upcase-word
bind -q downcase-word
