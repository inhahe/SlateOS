# Nested code inside a pipeline stage reads that stage's pipe, not fd 0.
echo piped | { eval 'read v; echo "[$v]"'; }
