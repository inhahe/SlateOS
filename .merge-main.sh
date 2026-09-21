set -x
cd "E:/visual studio projects/os-lane-c" || exit 1
git push origin lane-c; echo "PUSH_LANE_RC=$?"
cd "E:/visual studio projects/os" || exit 1
git fetch origin; echo "FETCH_RC=$?"
git merge --ff-only origin/main; echo "FFMAIN_RC=$?"
git merge --no-edit 370c0e659; echo "MERGE_RC=$?"
git log --oneline -1
git push origin main; echo "PUSH_MAIN_RC=$?"
