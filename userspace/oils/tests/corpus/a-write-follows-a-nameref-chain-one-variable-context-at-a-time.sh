# A read resolves a nameref by looking each link up the ordinary way — every
# scope at once, innermost binding first (`find_variable_nameref`). A **write**
# does not. `bind_variable` (variables.c:3275) descends the stack of variable
# contexts one hash table at a time:
#
#	  for (vc = shell_variables; vc; vc = vc->down)
#	    if (vc_isfuncenv (vc) || vc_isbltnenv (vc))
#	      {
#		v = hash_lookup (name, vc->table);
#		nvc = vc;
#		if (v && nameref_p (v))
#		  { nv = find_variable_nameref_context (v, vc, &nvc); … }
#		if (v)
#		  return (bind_variable_internal (v->name, value, nvc->table, 0, flags));
#	      }
#	  return (bind_variable_internal (name, value, global_variables->table, 0, flags));
#
# and `find_nameref_at_context` (variables.c:2172) follows only as much of the
# chain as *one* table holds:
#
#	  nv = v;
#	  level = 1;
#	  while (nv && nameref_p (nv))
#	    {
#	      level++;
#	      if (level > NAMEREF_MAX)
#		return (&nameref_maxloop_value);
#	      newname = nameref_cell (nv);
#	      if (newname == 0 || *newname == '\0')
#		return ((SHELL_VAR *)NULL);
#	      nv2 = hash_lookup (newname, vc->table);
#	      if (nv2 == 0)
#		break;
#	      nv = nv2;
#	    }
#
# The global table is neither a function nor a builtin environment, so the
# outer loop skips it: this is the machinery for a reference some `local`
# bound, and a purely global one falls through to `bind_variable_internal` and
# the ordinary walk. Three things follow, and none of them is what a read does.
exec 2>&1
echo "--- the write lands in a context the read never names"
q=GLOBQ
outer() { local -n p=q; local q=OUTERQ; inner; echo "outer q=$q"; }
inner() { local q=INNERQ; echo "read  p=$p"; p=WRITTEN; echo "inner q=$q"; }
outer; echo "global q=$q"
echo "--- and the walk never climbs back up"
declare -n G1=zz
i1() { local -n a=G1; local zz=INNERZZ; a=X; echo "inner zz=$zz"; }
i1; echo "global zz=$(declare -p zz 2>&1)"
echo "--- a link the context does not hold drops the walk one context down"
declare -a arr=(A B C)
j1() { local -n b='arr[1]'; b=Z; }
j1; declare -p arr
k1() { local -n c=nosuch; c=CREATED; }
k1; declare -p nosuch
# `find_nameref_at_context` sets `level = 1` where `find_variable_nameref`
# sets it to 0, so one context follows **seven** links where a read follows
# eight. Seven resolve; the eighth is the cap.
echo "--- one context follows seven links where a read follows eight"
sev() { local -n s1=s2 s2=s3 s3=s4 s4=s5 s5=s6 s6=s7 s7=s8; local s8=EIGHT; s1=W; echo "st=$?"; declare -p s8; }
sev; echo "global s1=$(declare -p s1 2>&1)"
eig() { local -n e1=e2 e2=e3 e3=e4 e4=e5 e5=e6 e6=e7 e7=e8 e8=e9; local e9=NINE
        echo "read  [${e1}]"; e1=W; echo "st=$?"; declare -p e9; }
eig; echo "global e1=$(declare -p e1 2>&1)"
echo "--- …so a chain spread over two contexts is followed further than a read would"
declare -n t7=t8; declare -n t8=t9; declare -n t9=t10; declare -n t10=t11
declare -n t11=t12; declare -n t12=t13; t13=THIRTEEN
deep() { local -n t1=t2 t2=t3 t3=t4 t4=t5 t5=t6 t6=t7; echo "read  [${t1}]"; t1=W; echo "st=$?"; }
deep; declare -p t13; echo "global t1=$(declare -p t1 2>&1)"
# `find_variable_nameref_context` hands `&nameref_maxloop_value` straight back
# to `bind_variable`, which is the one place a write says anything at all about
# giving up (the read walk gives up silently):
#
#	  else if (nv == &nameref_maxloop_value)
#	    {
#	      internal_warning (_("%s: circular name reference"), v->name);
#	      return (bind_global_variable (v->name, value, flags));
#	    }
#
# — so the value goes to the **global** of the name the walk started from, and
# the local reference survives, exactly as a cycle that escaped to global scope
# leaves it.
echo "--- past the cap it warns and binds the global of the name it started from"
cap() { local -n u1=u2 u2=u3 u3=u4 u4=u5 u5=u6 u6=u7 u7=u8 u8=u9 u9=u10; local u10=TEN
        echo "read  [${u1}]"; u1=W; echo "st=$?"; declare -p u10 u1; }
cap; echo "global u1=$(declare -p u1 2>&1)"
echo "--- a cycle among locals reaches the same cap by spinning"
cyc() { local -n v1=v2 v2=v3 v3=v2; v1=W; echo "st=$?"; declare -p v1 v2 v3; }
cyc; echo "global v1=$(declare -p v1 2>&1)"
echo "--- every write that goes through bind_variable takes this walk"
run() { echo "== $1"; ( eval "t() { local -n w1=w2 w2=w3 w3=w4 w4=w5 w5=w6 w6=w7 w7=w8 w8=w9; local w9=NINE; $2; declare -p w9; }"; t; echo "st=$?"; echo "  global: $(declare -p w1 2>&1 | head -1)" ); }
run scalar    'w1=X'
run append    'w1+=X'
run read      'read w1 <<< P'
run 'printf -v' 'printf -v w1 "%s" P'
run arith     '(( w1 = 5 ))'
run 'assign-op' 'w1=${w1:=V}'
echo "--- and the ones that do not are unchanged by it"
run element   'w1[2]=X'
run 'read -a' 'read -a w1 <<< "P Q"'
run for       'for w1 in P; do :; done'
run compound  'w1=(P Q)'
run unset     'unset w1'
run declare   'declare w1=Z'
echo TAIL
