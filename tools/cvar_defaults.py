# Dump client.dll/engine.dll convar defaults: python3 tools/cvar_defaults.py <dll>  (prints "name default")
# Dump convar (name, default, help) triples: a lea of a help string, then default, then name, within a short window.
import struct,sys,re
d=open(sys.argv[1],'rb').read()
pe=struct.unpack_from('<I',d,0x3c)[0]
ns=struct.unpack_from('<H',d,pe+6)[0]; osz=struct.unpack_from('<H',d,pe+20)[0]
secs=[]
for i in range(ns):
    o=pe+24+osz+i*40
    name=d[o:o+8].rstrip(b'\0'); vs,va,rs,ra=struct.unpack_from('<IIII',d,o+8)
    secs.append((name,va,vs,ra,rs))
def rva2off(r):
    for n,va,vs,ra,rs in secs:
        if va<=r<va+max(vs,rs): return r-va+ra
def cstr(r):
    o=rva2off(r)
    if o is None or o>=len(d): return None
    e=d.find(b'\0',o)
    s=d[o:e]
    if 0<len(s)<200 and all(32<=c<127 for c in s): return s.decode()
_,tva,tvs,tra,trs=[s for s in secs if s[0]==b'.text'][0]
code=d[tra:tra+trs]
leas=[]
for m in re.finditer(rb'[\x48\x4c]\x8d[\x05\x0d\x15\x1d\x25\x2d\x35\x3d]',code):
    i=m.start(); disp=struct.unpack_from('<i',code,i+3)[0]
    s=cstr(tva+i+7+disp)
    if s is not None: leas.append((i,s))
num=re.compile(r'^-?[0-9]*\.?[0-9]+$')
ident=re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')
seen=set()
for k in range(1,len(leas)):
    (i1,a),(i2,b)=leas[k-1],leas[k]
    if i2-i1<40 and num.match(a) and ident.match(b) and b not in seen:
        seen.add(b); print(b,a)
