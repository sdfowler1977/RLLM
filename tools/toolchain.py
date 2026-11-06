from pathlib import Path,PurePosixPath
from concurrent.futures import ThreadPoolExecutor
import hashlib,json,os,posixpath,shutil,stat,subprocess,tarfile,zipfile
base=Path.cwd();source=Path(__file__).resolve().parents[1];downloads=base/'tooling/downloads';downloads.mkdir(parents=True,exist_ok=True);tools=json.loads((source/'docs/TOOLCHAIN.json').read_text())['tools']
def digest(path):
 h=hashlib.sha256()
 with path.open('rb')as f:
  while data:=f.read(8*1024*1024):h.update(data)
 return h.hexdigest()
def verified(path,t):
 return path.is_file()and path.stat().st_size==t.get('bytes',t.get('size'))and digest(path)==t['sha256']
def preserve(path):
 if not path.exists():return
 number=1
 while path.with_name(path.name+f'.rejected-{number}').exists():number+=1
 path.rename(path.with_name(path.name+f'.rejected-{number}'))
def durable_download(t,path):
 # The parent owns and flushes the complete sequential write. A downloader's
 # process exit alone previously preceded truncated persisted cache files.
 partial=path.with_name(path.name+'.partial');preserve(partial)
 process=subprocess.Popen(['curl','--fail','--silent','--show-error','--location','--connect-timeout','15','--max-time','600',t['url']],stdout=subprocess.PIPE)
 size=0;expected=t.get('bytes',t.get('size'))
 try:
  with partial.open('xb')as out:
   while chunk:=process.stdout.read(1024*1024):
    size+=len(chunk)
    if size>expected:raise ValueError('Official tool download exceeds its pinned size: '+t['component'])
    out.write(chunk)
   out.flush();os.fsync(out.fileno())
  if process.wait()!=0:raise ValueError('Official tool download failed: '+t['component'])
  if size!=expected or not verified(partial,t):raise ValueError('Persisted official tool bytes differ: '+t['component'])
  preserve(path);partial.rename(path)
  descriptor=os.open(path.parent,os.O_RDONLY)
  try:os.fsync(descriptor)
  finally:os.close(descriptor)
  if not verified(path,t):raise ValueError('Renamed official tool bytes differ: '+t['component'])
 finally:
  process.stdout.close()
  if process.poll()is None:process.kill();process.wait()
def download(t):
 suffix='.zip'if t['url'].endswith('.zip')else'.tar.gz'if t['url'].endswith('.tar.gz')else'.tar.xz';p=downloads/(t['component']+'-'+t.get('target','host')+suffix)
 for attempt in range(2):
  if verified(p,t):print('Verified persisted official',t['component'],t.get('target',''),flush=True);return t,p
  preserve(p)
  try:durable_download(t,p)
  except (OSError,ValueError)as failure:
   if attempt:raise
   print('Retrying rejected official download once:',t['component'],str(failure),flush=True)
 if not verified(p,t):raise ValueError('Official tool download could not be persisted: '+t['component'])
 print('Verified persisted official',t['component'],t.get('target',''),flush=True);return t,p
with ThreadPoolExecutor(max_workers=4)as pool:items=list(pool.map(download,tools))
def name(n):
 p=PurePosixPath(n);assert n and not p.is_absolute()and'\\'not in n and all(x not in ['..','']for x in p.parts),n;return p
def extract(t,p):
 # Recheck the persisted cache immediately before consuming it, including cache
 # entries downloaded concurrently with other components.
 if not verified(p,t):_,p=download(t)
 dest=base/'tooling'/('components'if t['component'].startswith('rust')else t['component']);dest.mkdir(parents=True,exist_ok=True);seen=set();links=[];total=0
 if zipfile.is_zipfile(p):
  with zipfile.ZipFile(p)as z:
   entries=z.infolist();assert len(entries)<30000
   for i in entries:
    pp=name(i.filename);assert i.filename not in seen;seen.add(i.filename);total+=i.file_size;assert total<5*1024**3 and i.file_size<1024**3
    mode=(i.external_attr>>16)&0o170000;assert mode in [0,stat.S_IFREG,stat.S_IFDIR,stat.S_IFLNK]
    if mode==stat.S_IFLNK:
     target=z.read(i).decode();resolved=posixpath.normpath(posixpath.join(str(pp.parent),target));name(resolved);links.append((pp,target,False));continue
    target=dest.joinpath(*pp.parts)
    if i.is_dir():target.mkdir(parents=True,exist_ok=True)
    else:
     target.parent.mkdir(parents=True,exist_ok=True)
     with z.open(i)as src,target.open('wb')as dst:shutil.copyfileobj(src,dst,1024*1024)
     assert target.stat().st_size==i.file_size,(target,'incomplete extraction')
     target.chmod(0o755 if i.external_attr>>16&0o111 else 0o644)
 else:
  with tarfile.open(p)as z:
   entries=z.getmembers();assert len(entries)<30000
   for i in entries:
    pp=name(i.name);assert i.name not in seen;seen.add(i.name);total+=i.size;assert total<5*1024**3 and i.size<1024**3;assert i.isfile()or i.isdir()or i.issym()or i.islnk(),i.name
    if i.issym()or i.islnk():resolved=posixpath.normpath(posixpath.join(str(pp.parent),i.linkname))if i.issym()else i.linkname;name(resolved);links.append((pp,i.linkname,i.islnk()));continue
    target=dest.joinpath(*pp.parts)
    if i.isdir():target.mkdir(parents=True,exist_ok=True)
    else:
     target.parent.mkdir(parents=True,exist_ok=True)
     with z.extractfile(i)as src,target.open('wb')as dst:shutil.copyfileobj(src,dst,1024*1024)
     assert target.stat().st_size==i.size,(target,'incomplete extraction')
     target.chmod(0o755 if i.mode&0o111 else 0o644)
 for pp,target,hard in links:
  path=dest.joinpath(*pp.parts);path.parent.mkdir(parents=True,exist_ok=True)
  if path.is_symlink()or path.exists():assert path.is_symlink()and os.readlink(path)==target;continue
  if hard:os.link(dest/target,path)
  else:os.symlink(target,path)
 print('Extracted contained',p.name,len(seen),'members',flush=True)
for t,p in items:extract(t,p)
rust=base/'tooling/rust';rust.mkdir(exist_ok=True)
for t,_ in items:
 if t['component'].startswith('rust'):
  outer=base/'tooling/components'/(t['component']+'-1.99.1-'+t['target']);inner=outer/(t['component']if t['component']=='rustc'else'rust-std-'+t['target']);shutil.copytree(inner,rust,dirs_exist_ok=True,symlinks=True)
(base/'tooling/env.sh').write_text('''project_tooling=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export CLIENT_RUST="$project_tooling/rust"
export CLIENT_JDK="$project_tooling/jdk/jdk-17.0.21+9"
export CLIENT_ANDROID="$project_tooling/platform/android-35"
export CLIENT_TOOLS="$project_tooling/buildtools/android-15"
export CLIENT_NDK="$project_tooling/ndk/android-ndk-r28c"
export LD_LIBRARY_PATH="$CLIENT_RUST/lib:$CLIENT_TOOLS/lib64:$CLIENT_JDK/lib:$CLIENT_JDK/lib/server:${LD_LIBRARY_PATH:-}"
''')
print('Verified toolchain restored.',flush=True)
