"""Rename a tree extracted with 8.3 names to the Joliet names of the same raw image."""
import struct, sys, os
RAW,HDR,DATA=2352,16,2048
img,root=sys.argv[1:3]
f=open(img,'rb')
def read(lba,size):
    out=bytearray()
    for i in range((size+DATA-1)//DATA):
        f.seek((lba+i)*RAW+HDR); out+=f.read(DATA)
    return bytes(out[:size])
def tree(lba,size,dec):
    d=read(lba,size); i=0; files=[]; dirs=[]
    while i<len(d):
        n=d[i]
        if n==0: i=(i//DATA+1)*DATA; continue
        r=d[i:i+n]; i+=n
        e,s,fl,nl=struct.unpack_from('<I',r,2)[0],struct.unpack_from('<I',r,10)[0],r[25],r[32]
        nm=r[33:33+nl]
        if nm in (b'\0',b'\1'): continue
        nm=dec(nm).split(';')[0]
        if fl&2: dirs.append((nm,tree(e,s,dec)))
        else: files.append((nm,e,s))
    return files,dirs
def exts(t):
    s={(e,z) for _,e,z in t[0]}
    for _,c in t[1]: s|=exts(c)
    return s
def root_of(v,dec):
    r=v[156:190]; return tree(struct.unpack_from('<I',r,2)[0],struct.unpack_from('<I',r,10)[0],dec)
pvd=read(16,DATA); j=None
for s in range(17,30):
    v=read(s,DATA)
    if v[0]==2 and v[1:6]==b'CD001' and v[88:90]==b'%/': j=v;break
    if v[0]==255: break
A=root_of(pvd,lambda b:b.decode('latin-1')); B=root_of(j,lambda b:b.decode('utf-16-be'))
ren=[0]
def match(a,b,path):
    used=set()
    for an,ac in a[1]:
        ea=exts(ac)
        best=max((x for x in b[1] if x[0] not in used),key=lambda x:(len(ea&exts(x[1])),x[0].upper()==an.upper()))
        used.add(best[0]); match(ac,best[1],os.path.join(path,an))
        if an!=best[0] and os.path.lexists(os.path.join(path,an)):
            os.rename(os.path.join(path,an),os.path.join(path,best[0])); ren[0]+=1
    pool=list(b[0])
    for an,e,z in a[0]:
        c=[x for x in pool if x[1]==e and x[2]==z]
        c.sort(key=lambda x:x[0].upper()!=an.upper()); x=c[0]; pool.remove(x)
        if an!=x[0] and os.path.lexists(os.path.join(path,an)):
            os.rename(os.path.join(path,an),os.path.join(path,x[0])); ren[0]+=1
match(A,B,root); print('renamed',ren[0])
