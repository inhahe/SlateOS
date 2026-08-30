#!/bin/bash
set -u
W=/tmp/tccx
cat > /tmp/p37.c <<'EOF'
extern int printf(const char *fmt, ...);
extern void *malloc(unsigned long n);
extern void free(void *p);
int main(void){
  char *p = (char*)malloc(8);
  if(!p) return 2;
  p[0]='S'; p[1]='L'; p[2]='A'; p[3]='T'; p[4]='E'; p[5]=0;
  printf("%s-%d\n", p, 1234);
  free(p);
  return 0;
}
EOF
echo "=== compile with image tcc ==="
"$W/tcc" -o /tmp/p37 /tmp/p37.c && echo "compile OK" || { echo "COMPILE FAILED"; exit 1; }
echo "=== readelf interp ==="
readelf -l /tmp/p37 2>/dev/null | grep -A1 INTERP
echo "=== run (redirected to file, like SlateOS) ==="
/tmp/p37 > /tmp/p37.out; echo "exit=$?"
echo "=== output bytes ==="
xxd /tmp/p37.out
echo "=== output text ==="
cat /tmp/p37.out
