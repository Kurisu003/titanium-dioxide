# Player-settings field defaults from client.dll tables {name, default, help, ...}:
#   python3 tools/settings_defaults.py <client.dll> <field>...        find the table entry of a field
#   python3 tools/settings_defaults.py <client.dll> --dump <hex offset>  dump the table containing it
import struct,sys
d=open(sys.argv[1],'rb').read()
pe=struct.unpack_from('<I',d,0x3c)[0]
ns=struct.unpack_from('<H',d,pe+6)[0]; osz=struct.unpack_from('<H',d,pe+20)[0]
base=struct.unpack_from('<Q',d,pe+24+24)[0]
secs=[]
for i in range(ns):
    o=pe+24+osz+i*40
    vs,va,rs,ra=struct.unpack_from('<IIII',d,o+8); secs.append((d[o:o+8].rstrip(b'\0'),va,vs,ra,rs))
def off2rva(off):
    for n,va,vs,ra,rs in secs:
        if ra<=off<ra+rs: return off-ra+va
def rva2off(r):
    for n,va,vs,ra,rs in secs:
        if va<=r<va+rs: return r-va+ra
def cstr(p):
    r=p-base; o=rva2off(r) if 0<r<1<<31 else None
    if o is None: return None
    e=d.find(b'\0',o); s=d[o:e]
    if 0<len(s)<150 and all(32<=c<127 for c in s): return s.decode()
for name in ([] if len(sys.argv)>2 and sys.argv[2]=="--dump" else sys.argv[2:]):
    off=d.find(b'\0'+name.encode()+b'\0')+1
    ptr=struct.pack('<Q',base+off2rva(off))
    i=d.find(ptr)
    while i>=0:
        out=[]
        for k in range(-6,8):
            q=struct.unpack_from('<Q',d,i+k*8)[0]
            s=cstr(q)
            out.append(repr(s[:50]) if s else hex(q))
        print(name,'@',hex(i),' | '.join(out)); i=d.find(ptr,i+1)

def dump(at):
    # walk back to the start, then forward, 48-byte entries {name, default, desc, ...}
    def ent(i):
        n=cstr(struct.unpack_from('<Q',d,i)[0]); df=struct.unpack_from('<Q',d,i+8)[0]; ds=cstr(struct.unpack_from('<Q',d,i+16)[0])
        return n,(cstr(df) if df else ''),ds
    i=at
    while ent(i-48)[0] and ent(i-48)[2]: i-=48
    while True:
        n,df,ds=ent(i)
        if not n: break
        print(n, repr(df)); i+=48
if len(sys.argv)==2 or sys.argv[2]=='--dump':
    dump(int(sys.argv[3],16))
