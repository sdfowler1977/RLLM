"""Preserve exact work-in-progress source; never substitutes for a signed release."""
import argparse,datetime,hashlib,json,pathlib,re,zipfile
p=argparse.ArgumentParser();p.add_argument('description');args=p.parse_args()
assert re.fullmatch(r'[A-Za-z]+(?:-[A-Za-z]+)*',args.description)
root=pathlib.Path(__file__).resolve().parents[1]
stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d__%H-%M-%SZ')
name=f'RLLM__{args.description}__{stamp}.zip';assert not re.search(r'a[0-9]',name,re.I)
output=root.parents[1]/'recovery'/name;output.parent.mkdir(parents=True,exist_ok=True)
files=sorted(f for f in root.rglob('*')if f.is_file()and not set(f.relative_to(root).parts)&{'output','release','__pycache__','.git'})
assert all(not f.is_symlink()for f in files)
manifest={f.relative_to(root).as_posix():hashlib.sha256(f.read_bytes()).hexdigest()for f in files}
with zipfile.ZipFile(output,'w',zipfile.ZIP_DEFLATED)as z:
 for file in files:z.write(file,'RLLM/'+file.relative_to(root).as_posix())
 z.writestr('CHECKPOINT.json',json.dumps({'status':'in-progress; not a signed release','files':manifest},indent=2))
with zipfile.ZipFile(output)as z:
 assert z.testzip()is None
 for file,digest in manifest.items():assert hashlib.sha256(z.read('RLLM/'+file)).hexdigest()==digest
print(json.dumps(dict(path=str(output),name=name,sha256=hashlib.sha256(output.read_bytes()).hexdigest(),files=len(files))))
