git add -A
git commit -m "Obligatory commit message"
echo Files changed since last push:
git diff --name-only @{push}..HEAD 2>nul || git diff --name-only @{upstream}..HEAD
git push

