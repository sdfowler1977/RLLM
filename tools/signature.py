"""Verify finished delivery APK bytes with the retained signing identity."""
from pathlib import Path
import os,re,subprocess,zipfile
CERTIFICATE='ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5'
def verify(path):
 path=Path(path);env=dict(os.environ,PATH=os.environ['CLIENT_JDK']+'/bin:'+os.environ['PATH']);result=subprocess.run([str(Path(os.environ['CLIENT_TOOLS'])/'apksigner'),'verify','--verbose','--print-certs',str(path)],text=True,capture_output=True,env=env)
 if result.returncode:raise ValueError('APK signature verification failed: '+path.name)
 cert=result.stdout
 if not all(s in cert for s in ['Verified using v2 scheme (APK Signature Scheme v2): true','Verified using v3 scheme (APK Signature Scheme v3): true']):raise ValueError('Delivery APK requires verified v2 and v3 signatures: '+path.name)
 certificates=re.findall(r'^Signer #[0-9]+ certificate SHA-256 digest: ([0-9a-f]{64})$',cert,re.MULTILINE)
 if certificates!=[CERTIFICATE] or 'Number of signers: 1'not in cert.splitlines():raise ValueError('Delivery APK signing identity differs from the retained certificate: '+path.name)
 with zipfile.ZipFile(path)as z:
  if z.testzip()is not None:raise ValueError('APK archive integrity verification failed: '+path.name)
 return dict(v2=True,v3=True,certificate_sha256=CERTIFICATE)
