cd "E:/visual studio projects/os-lane-c" || exit 1
git fetch origin; echo "FETCH_RC=$?"
git merge --no-edit origin/main; echo "MERGEMAIN_RC=$?"
python scripts/run-timeout.py --poll 120 5400 cargo test --workspace --target x86_64-pc-windows-gnu
echo "CARGO_RC=$?"
