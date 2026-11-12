"""Verify downloaded deliverables against the packaged byte identity."""
from pathlib import Path
import argparse,hashlib,json,zipfile
from signature import verify
root=Path(__file__).resolve().parents[1];p=argparse.ArgumentParser();p.add_argument('--apk',type=Path,required=True);p.add_argument('--source',type=Path,required=True);p.add_argument('--checksums',type=Path,required=True);args=p.parse_args();m=json.loads((root/'output/checks/packaged.json').read_text())
for key in ['apk','source','checksums']:
 p=getattr(args,key);meta=m['files'][Path(m['paths'][key]).name];assert p.stat().st_size==meta['bytes']and hashlib.sha256(p.read_bytes()).hexdigest()==meta['sha256'],key
signature=verify(args.apk)
with zipfile.ZipFile(args.source)as z:
 assert z.testzip()is None and len(z.namelist())==m['members'];manifest=json.loads(z.read('RLLM/PUBLICATION-MANIFEST.json'));assert manifest['version']==m['version']and len(manifest['build_inputs'])==m['build_inputs']
 for n,v in manifest['files'].items():data=z.read('RLLM/'+n);assert len(data)==v['bytes']and hashlib.sha256(data).hexdigest()==v['sha256'],n
 signed=manifest['signed_apk'];apk_bytes=args.apk.read_bytes()
 assert len(apk_bytes)==signed['bytes'] and hashlib.sha256(apk_bytes).hexdigest()==signed['sha256']
 apks=[n for n in z.namelist()if n.endswith('.apk')]
 if signed.get('location')=='companion':
  assert not apks and signed['path']==Path(m['paths']['apk']).name
 else:
  assert apks==['RLLM/release/RLLM.apk']and z.read(apks[0])==apk_bytes
record=dict(m,remote_verified=True,downloaded_signature=signature);(root/'output/checks/readback.json').write_text(json.dumps(record,indent=2)+'\n');print('Downloaded APK/source/checksums and every source member verified for build',m['version'])
