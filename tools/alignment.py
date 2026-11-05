"""Validate alignment output against every original APK payload before signing."""
from pathlib import Path
import argparse,hashlib,subprocess,zipfile,zlib

def payloads(path):
    with zipfile.ZipFile(path)as archive:
        members=archive.infolist();names=[m.filename for m in members]
        if not members or len(names)!=len(set(names)):
            raise ValueError('APK payload inventory is empty or duplicated')
        result={}
        for member in members:
            digest=hashlib.sha256();total=0
            with archive.open(member)as stream:
                while chunk:=stream.read(1024*1024):digest.update(chunk);total+=len(chunk)
            if total!=member.file_size:raise ValueError('APK payload length differs')
            result[member.filename]=(total,digest.hexdigest())
        return result

def verify(original,aligned):
    expected=payloads(original);actual=payloads(aligned)
    if actual!=expected:raise ValueError('Aligned APK payload identity differs from original')
    return len(expected)

def align(tool,original,aligned):
    # A successful process exit alone did not catch an earlier truncated output.
    # Retain the rejected file and make one bounded retry; never sign bad bytes.
    expected=payloads(original)
    for attempt in range(2):
        try:
            subprocess.run([str(tool),'-P','16','-f','4',str(original),str(aligned)],check=True)
            if payloads(aligned)!=expected:raise ValueError('Aligned APK payload identity differs from original')
            subprocess.run([str(tool),'-c','-P','16','4',str(aligned)],check=True)
            print(f'Alignment payload validation: {len(expected)} exact members; attempt {attempt+1}',flush=True)
            return
        except (OSError,ValueError,zipfile.BadZipFile,zlib.error,RuntimeError,EOFError,subprocess.CalledProcessError)as error:
            if aligned.exists():aligned.rename(aligned.with_name(f'aligned-rejected-{attempt+1}.apk'))
            if attempt:raise RuntimeError('APK alignment failed twice; no APK signed')from error
            print('Alignment output rejected; retaining it and retrying once: '+str(error),flush=True)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--tool',type=Path,required=True);p.add_argument('original',type=Path);p.add_argument('aligned',type=Path);a=p.parse_args();align(a.tool,a.original,a.aligned)
