# Startup files, and the invocation options that redirect or suppress them.
#
# Which files a shell reads is decided entirely by *how it was started*, and by
# two facts that are independent of each other: whether it is a login shell, and
# whether it is interactive.
#
#   login             /etc/profile, then the FIRST of ~/.bash_profile,
#                     ~/.bash_login, ~/.profile that exists — never ~/.bashrc.
#   interactive, not login   ~/.bashrc (or --rcfile's argument instead).
#   non-interactive (either) the file $BASH_ENV names, after any profile.
#
# So `bash -l -c cmd` reads a profile and $BASH_ENV but no rc file, `bash -i -c
# cmd` reads the rc file but not $BASH_ENV, and a plain script reads only
# $BASH_ENV. A missing file is silent — every shell has startup files it does not
# have.
#
# `~/.bash_logout` is not part of that table: bash reads it from inside the
# `exit`/`logout` builtin (`exit_or_logout` calls `bash_logout`), so it runs only
# when a *login* shell exits through that builtin — never on falling off the end
# of a script, and never for a subshell's exit. Being inside the builtin also
# fixes its two odd details: it goes *before* the EXIT trap, and `$?` in it is
# the status from before the `exit`, because the operand has not been recorded
# yet.
#
# Most of the cases below re-invoke the shell and grep for markers. That is not
# decoration: a real /etc/profile exists on the reference system and not under
# the shell being compared, so only the case's own output can be compared.
#
# The long options are parsed in a separate, earlier pass than the single-letter
# ones (bash's `parse_long_options`), which is why they must come first and why
# their diagnostics differ from the letter parser's.

export HOME="$PWD"
unset BASH_ENV
mk() { printf '%s\n' "$2" > "$1"; }

echo "=== login: /etc/profile, then the first profile that exists"
mk .bash_profile 'echo "  MARK bash_profile"'
mk .bash_login 'echo "  MARK bash_login"'
mk .profile 'echo "  MARK profile"'
mk .bashrc 'echo "  MARK bashrc"'
"$BASH" -l -c 'echo "  MARK cmd"' | grep '^  MARK'

echo "=== ~/.bash_profile missing: ~/.bash_login is next"
rm -f .bash_profile
"$BASH" -l -c 'echo "  MARK cmd"' | grep '^  MARK'

echo "=== then ~/.profile"
rm -f .bash_login
"$BASH" -l -c 'echo "  MARK cmd"' | grep '^  MARK'

echo "=== none of them: silence, not an error"
# ~/.bashrc goes too, only because a real /etc/profile has been seen to notice a
# login shell with an rc file and no profile and comment on it. A login shell
# ignoring ~/.bashrc is checked below, where --noprofile keeps /etc/profile out.
rm -f .profile .bashrc
"$BASH" -l -c 'echo "  MARK cmd"; echo "  MARK rc=$?"' | grep '^  MARK'
mk .bashrc 'echo "  MARK bashrc"'

echo "=== --noprofile skips all of them"
mk .bash_profile 'echo "  MARK bash_profile"'
"$BASH" --noprofile -l -c 'echo "  MARK cmd"' | grep '^  MARK'

echo "=== a login shell never reads ~/.bashrc, interactive or not"
"$BASH" --noprofile -l -c 'echo "  MARK cmd"' | grep '^  MARK'
"$BASH" --noprofile -l -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== interactive non-login reads ~/.bashrc, and only it"
"$BASH" --noprofile -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== --norc skips it"
"$BASH" --noprofile --norc -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== --rcfile replaces it; --init-file is the same option"
mk my.rc 'echo "  MARK my.rc"'
"$BASH" --noprofile --rcfile my.rc -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'
"$BASH" --noprofile --init-file my.rc -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== a non-interactive shell reads neither"
"$BASH" --noprofile --rcfile my.rc -c 'echo "  MARK cmd"' | grep '^  MARK'

echo "=== an rc file that is a directory is reported, and the shell carries on"
mkdir dir.rc
"$BASH" --noprofile --rcfile dir.rc -i -c 'echo "  MARK icmd"' 2>&1 >/dev/null | grep -o 'dir.rc: is a directory'
"$BASH" --noprofile --rcfile dir.rc -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== \$BASH_ENV: non-interactive only, and after any profile"
mk env.sh 'echo "  MARK env"'
BASH_ENV=./env.sh "$BASH" --noprofile -c 'echo "  MARK cmd"' | grep '^  MARK'
BASH_ENV=./env.sh "$BASH" -l -c 'echo "  MARK cmd"' | grep '^  MARK'
BASH_ENV=./env.sh "$BASH" --noprofile --norc -i -c 'echo "  MARK icmd"' 2>/dev/null | grep '^  MARK'

echo "=== its value is expanded as if double-quoted: substitutions yes,"
echo "=== word splitting and globbing no, and a tilde afterwards"
V=nv
export V
BASH_ENV='./e${V}.sh' "$BASH" --noprofile -c 'true' | grep '^  MARK'
mk 'e nv.sh' 'echo "  MARK spaced"'
BASH_ENV='./e nv.sh' "$BASH" --noprofile -c 'true' | grep '^  MARK'
BASH_ENV='~/env.sh' "$BASH" --noprofile -c 'true' | grep '^  MARK'
BASH_ENV='' "$BASH" --noprofile -c 'echo "  MARK empty ok"' | grep '^  MARK'

echo "=== startup files see \$0 and the positional parameters already set"
mk .bash_profile 'echo "  MARK prof dollar0=$0 argc=$# args=[$*]"'
mk s.sh 'echo "  MARK script"'
"$BASH" -l s.sh a b | grep '^  MARK'

echo "=== and they run before the script is even opened"
"$BASH" -l nosuch.sh 2>/dev/null | grep '^  MARK'
echo "  rc=$?"

echo "=== \`return\` stops a startup file but its operand is thrown away"
mk .bash_profile 'true
return 5
echo "  MARK not reached"'
"$BASH" -l -c 'echo "  MARK rc=$?"' | grep '^  MARK'

echo "=== \`exit\` in one pre-empts the command entirely"
mk .bash_profile 'echo "  MARK prof"
exit 7'
"$BASH" -l -c 'echo "  MARK not reached"' | grep '^  MARK'
echo "  rc=$?"

echo "=== ~/.bash_logout: only a login shell, only via exit/logout,"
echo "=== before the EXIT trap, and seeing the status from before the exit"
mk .bash_profile ':'
mk .bash_logout 'echo "  MARK logout rc=$?"'
"$BASH" --noprofile -l -c 'trap "echo \"  MARK trap rc=\$?\"" EXIT; false; exit 5' | grep '^  MARK'
echo "  rc=$?"

echo "=== the logout builtin is the same path"
"$BASH" --noprofile -l -c 'true; logout 6' | grep '^  MARK'
echo "  rc=$?"

echo "=== falling off the end is not an exit, so nothing is read"
"$BASH" --noprofile -l -c 'echo "  MARK end"' | grep '^  MARK'

echo "=== nor is a subshell's exit"
"$BASH" --noprofile -l -c '(exit 3); echo "  MARK after=$?"' | grep '^  MARK'

echo "=== and a non-login shell never reads it"
"$BASH" --noprofile -c 'exit 4' | grep '^  MARK'
echo "  rc=$?"

echo "=== a failing command in it does not change the status; an exit does"
mk .bash_logout 'echo "  MARK lo"
false'
"$BASH" --noprofile -l -c 'exit 4' | grep '^  MARK'
echo "  rc=$?"
mk .bash_logout 'echo "  MARK lo"
exit 9'
"$BASH" --noprofile -l -c 'exit 4' | grep '^  MARK'
echo "  rc=$?"
rm -f .bash_logout

echo "=== long options must precede every single-letter one"
"$BASH" --noprofile -x -c 'true' 2>/dev/null; echo "  rc=$?"
"$BASH" -x --noprofile -c 'true' 2>&1 >/dev/null | grep -o -e '--: invalid option'
"$BASH" -x --noprofile -c 'true' >/dev/null 2>/dev/null; echo "  rc=$?"

echo "=== one dash is as good as two"
"$BASH" -noprofile -norc -i -c 'echo "  MARK single-dash"' 2>/dev/null | grep '^  MARK'
"$BASH" -login --noprofile -c 'echo "  MARK $(shopt -p login_shell)"' | grep '^  MARK'

echo "=== a missing argument names the option without its dashes, and no usage"
"$BASH" --rcfile >/dev/null 2>err; echo "  rc=$?"
grep -o 'rcfile: option requires an argument' err
echo "  stderr lines: $(wc -l < err)"
"$BASH" --init-file >/dev/null 2>err; echo "  rc=$?"
grep -o 'init-file: option requires an argument' err

echo "=== an unmatched two-dash option is fatal, with a usage summary"
"$BASH" --bogus >/dev/null 2>err; echo "  rc=$?"
grep -o -e '--bogus: invalid option' err
if test -s err; then echo "  usage printed"; fi

echo "=== but an unmatched one-dash word just falls through to the letters"
# `-xz`, not `-norc`: the letter parser would read the `o` in `norc` as `-o` and
# eat the next word as an option name, which is a different case entirely.
"$BASH" --noprofile -xz >/dev/null 2>err; echo "  rc=$?"
grep -o -e '-z: invalid option' err

echo "=== --version and --help win over anything after them, but not before"
"$BASH" --version -q >/dev/null 2>&1; echo "  rc=$?"
"$BASH" --help -q >/dev/null 2>&1; echo "  rc=$?"
"$BASH" --version --bogus >/dev/null 2>&1; echo "  rc=$?"

echo "=== --verbose is set -v"
"$BASH" --noprofile --verbose -c 'true' 2>&1 >/dev/null

echo "=== --noediting is accepted"
"$BASH" --noprofile --noediting -c 'echo "  MARK ok"' | grep '^  MARK'

echo "=== after -- a long option is a filename"
"$BASH" --noprofile -- --norc 2>/dev/null; echo "  rc=$?"
