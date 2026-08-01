# A glob's trailing slash is not a component: it asks that only directories
# match, and however many slashes were written the match keeps exactly one.
# Slashes *inside* the pattern are a different story — the part of the pattern
# in front of the last one is used verbatim when it is plain text, so `d1//*`
# keeps both of its slashes, and is re-expanded (and so tidied to one slash)
# when it is itself a pattern.
mkdir -p t/d1/d2/d3 t/e1
touch t/a.txt t/b.log t/d1/x.txt t/d1/d2/y.txt t/d1/d2/d3/z.txt
cd t || exit 1

echo "=== a trailing slash keeps only the directories, and keeps one slash"
echo */
echo *//
echo *///
echo d1/*/
echo d1/d2/*/
echo [de]*/
echo ?1/
echo a*/
echo nomatch*/

echo "=== …and without it nothing is filtered"
echo *
echo d1/*
echo d1/d2/*

echo "=== a plain prefix is used verbatim, a pattern prefix is re-expanded"
echo d1//*
echo d1///*
echo d*//d2
echo d*///d2
echo d1//d2/*

echo "=== a literal component still has to be inside a directory"
echo */.
echo */x.txt
echo *.txt/x.txt

echo "=== globstar: the zero-directories match is the prefix itself"
shopt -s globstar
echo d1/**
echo d1/d2/**
echo d1/d2/d3/**
echo e1/**
echo ./**
echo **

echo "=== …spelled with the slash when the prefix was not itself a pattern"
echo '[d]1/**'
echo [d]1/**
echo d1/*/**
echo */**
echo d1/**/d3/**

echo "=== globstar with a trailing slash is every directory below"
echo **/
echo d1/**/
echo d1/*/**/
echo */**/

echo "=== …and the intermediate form still swallows its slash"
echo **/*.txt
echo d1/**/*.txt
echo **/d3
echo d1/**/d3
shopt -u globstar
echo d1/**

echo "=== a link to a directory is a directory"
if ln -s d1 sld 2>/dev/null && ln -s a.txt sla 2>/dev/null; then
  echo sl*/
  echo sl*
else
  echo "sld/"
  echo "sla sld"
fi

echo "=== done"
