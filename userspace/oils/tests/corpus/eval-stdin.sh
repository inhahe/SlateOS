# STDIN: fed-to-eval
# `eval` runs in the current shell, so it reads the shell's stdin — including
# a pipe feeding the enclosing compound command.
eval 'read v; echo "[$v]"'
