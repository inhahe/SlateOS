# Here-documents: expanding, quoted, and tab-stripping variants.
v=world
cat <<EOF
hello $v
EOF
cat <<'EOF'
literal $v
EOF
cat <<-EOF
	indented $v
EOF
