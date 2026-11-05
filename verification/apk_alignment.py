"""Real zipalign output and damaged APK payload checks; no signing or delivery."""
import hashlib,importlib.util,json,os,pathlib,tempfile,zipfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('alignment',PROJECT/'tools/alignment.py');alignment=importlib.util.module_from_spec(spec);spec.loader.exec_module(alignment)
passed=[]
def check(name,value):assert value,name;passed.append(name)
def rejected(name,original,candidate):
    try:alignment.verify(original,candidate)
    except (ValueError,zipfile.BadZipFile,RuntimeError,EOFError):passed.append(name)
    else:raise AssertionError(name)
original=PROJECT/'release/RLLM.apk';identity=hashlib.sha256(original.read_bytes()).hexdigest()
with tempfile.TemporaryDirectory(prefix='apk-alignment-',dir=PROJECT/'output')as tmp:
    root=pathlib.Path(tmp);aligned=root/'aligned.apk';alignment.align(pathlib.Path(os.environ['CLIENT_TOOLS'])/'zipalign',original,aligned);check('real official zipalign retains every current APK payload',alignment.verify(original,aligned)==len(zipfile.ZipFile(original).infolist()))
    truncated=root/'truncated.apk';truncated.write_bytes(aligned.read_bytes()[:2_048_000]);rejected('truncated alignment output is rejected before signing',original,truncated)
    with zipfile.ZipFile(original)as z:
        entries=[(m.filename,z.read(m))for m in z.infolist()]
    for kind in ['changed','missing','extra','duplicate']:
        path=root/(kind+'.apk')
        with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED)as z:
            for i,(name,data)in enumerate(entries):
                if kind=='missing'and i==0:continue
                z.writestr(name,data+b'changed'if kind=='changed'and i==0 else data)
            if kind=='extra':z.writestr('unexpected.txt',b'extra')
            if kind=='duplicate':z.writestr(entries[0][0],entries[0][1])
        rejected('valid ZIP with '+kind+' payload inventory cannot be signed',original,path)
    damaged=root/'bad-crc.apk'
    with zipfile.ZipFile(damaged,'w',zipfile.ZIP_STORED)as z:z.writestr('classes.dex',b'original payload')
    raw=bytearray(damaged.read_bytes());at=raw.index(b'original payload');raw[at]^=1;damaged.write_bytes(raw);rejected('payload CRC corruption cannot pass identity validation',original,damaged)
check('packaging validation leaves the saved signed application unchanged',hashlib.sha256(original.read_bytes()).hexdigest()==identity)
report=dict(passed=len(passed),checks=passed,scope='Actual official zipalign and current saved APK payloads. Deliberately damaged temporary copies exercise pre-sign rejection. No new application or signature acceptance is claimed; transient retry is not forced.')
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True);(PROJECT/'output/checks/apk_alignment.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
