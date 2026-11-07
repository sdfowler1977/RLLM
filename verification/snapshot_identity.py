"""Configured TLS identity through actual legacy and streamed payload operations."""
import base64,copy,json,os,pathlib,socketserver,ssl,subprocess,tempfile,threading
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
def call(value):p.stdin.write(json.dumps(value)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
body=b'\x00\xffopaque\r\nHost: 100.64.0.2:5006\r\n\x80bytes';connection=dict(url='https://100.64.0.2:5006/share/',tls_name='nas.example.test',user='retained-user',password='retained-password',plain=False)
def record(phase,streamed=False):
 r=dict(stage_ticket='owned-fixture',purpose='project_transfer:owned-'+phase,bytes=len(body),phone=False,tree=None,connection=copy.deepcopy(connection),target=connection['url']+'saved.bin',mime='application/octet-stream',source=None,phase=phase)
 if streamed:r['format']=2
 return r
def request(r,streamed=False,stored=None):return call(dict(op='project_chunks'if streamed else'project_payload',action='open',record=r,stored=copy.deepcopy(r)if stored is None else stored,archive=base64.b64encode(body).decode()))
try:
 for streamed in [False,True]:
  label='streamed'if streamed else'legacy'
  for phase in ['write','probe','verify']:
   r=record(phase,streamed);original=copy.deepcopy(r);reply=request(r,streamed);assert'value'in reply,reply;e=reply['value']
   try:
    check(label+' '+phase+' retains VPN socket route and HTTPS',e['host']=='100.64.0.2'and e['port']==5006 and e['tls']is True)
    check(label+' '+phase+' retains configured certificate DNS identity',e.get('tls_host')=='nas.example.test')
    wire=base64.b64decode(e['wire_base64'])if'wire_base64'in e else e['wire'].encode();header,payload=wire.split(b'\r\n\r\n',1)
    check(label+' '+phase+' applies certificate alias only to request authority',b'\r\nHost: nas.example.test:5006\r\n'in header+b'\r\n')
    check(label+' '+phase+' retains original connection credentials',b'Authorization: Basic '+base64.b64encode(b'retained-user:retained-password')in header)
    check(label+' '+phase+' preserves conditional write or read semantics',wire.startswith((b'PUT'if phase=='write'else b'GET')+b' /share/saved.bin HTTP/1.1\r\n')and(b'If-None-Match: *'in header)==(phase=='write'))
    if phase=='write':check(label+' write preserves exact body framing',b'Content-Length: '+str(len(body)).encode()in header and payload==(b''if streamed else body))
    else:check(label+' '+phase+' carries no write payload',payload==b'')
    check(label+' '+phase+' leaves saved connection and source bytes unchanged',r==original)
   finally:call(dict(op='project_chunks'if streamed else'project_close',action='close',handle=e['snapshot_handle']))
  for invalid in ['https://nas.example.test','*.example.test','100.64.0.2','bad\r\nInjected: value']:
   r=record('write',streamed);r['connection']['tls_name']=invalid;reply=request(r,streamed);check(label+' rejects invalid certificate identity '+repr(invalid),'error'in reply and'value'not in reply)
  r=record('write',streamed);r['connection']['url']='http://100.64.0.2:5006/share/';r['connection']['plain']=True;r['target']=r['connection']['url']+'saved.bin';reply=request(r,streamed);check(label+' refuses certificate identity on unencrypted transport','error'in reply and'value'not in reply)
  r=record('probe',streamed);stored=copy.deepcopy(r);stored['connection']['tls_name']='changed.example.test';reply=request(r,streamed,stored);check(label+' refuses changed saved connection before transport','error'in reply and'value'not in reply)
  r=record('write',streamed);r['connection']['tls_name']='';reply=request(r,streamed);assert'value'in reply,reply;e=reply['value'];check(label+' retains ordinary HTTPS when no alias is configured',e['tls']is True and not e.get('tls_host'))
  call(dict(op='project_chunks'if streamed else'project_close',action='close',handle=e['snapshot_handle']))
finally:p.stdin.close();p.wait(timeout=10)

# Execute the unchanged production TLS socket block with a real local certificate,
# trusted only inside this test JVM. Hostname verification remains enabled.
activity=(PROJECT/'platform/com/local/client/Activity.java').read_text();start=activity.index('if(command.optBoolean("tls")){SSLSocket');end=activity.index('\n',start);tls_block=activity[start:end].strip()
JAVA=r'''
import java.net.*;import java.io.*;import java.util.*;import javax.net.ssl.*;
public class SnapshotTlsChecks{
 static int networkActive(int token,long deadline){return 10000;}
 static class Command{String name;Command(String n){name=n;}boolean optBoolean(String key){return true;}String optString(String key,String fallback){return name.isEmpty()?fallback:name;}boolean has(String key){return !name.isEmpty();}String getString(String key){return name;}}
 public static void main(String[] args)throws Exception{String host=args[0];int port=Integer.parseInt(args[1]),token=1;long transportDeadline=System.currentTimeMillis()+10000;Command command=new Command(args[2]);Socket raw=new Socket();raw.connect(new InetSocketAddress(host,port),5000);raw.setSoTimeout(5000);Socket connection=raw;
 TLS_BLOCK
 try(Socket managed=connection){managed.getOutputStream().write(Base64.getDecoder().decode(args[3]));managed.getOutputStream().flush();ByteArrayOutputStream result=new ByteArrayOutputStream();byte[] block=new byte[4096];int count;while((count=managed.getInputStream().read(block))!=-1)result.write(block,0,count);System.out.println(Base64.getEncoder().encodeToString(result.toByteArray()));}
 }
}
'''.replace(' TLS_BLOCK',tls_block)
with tempfile.TemporaryDirectory(prefix='snapshot-tls-',dir=PROJECT/'output')as folder:
 root=pathlib.Path(folder);jdk=pathlib.Path(os.environ['CLIENT_JDK']);certificate=root/'fixture.pem';private=root/'fixture.key';trust=root/'trust.p12';password='controlled-fixture-password'
 subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=nas.example.test','-addext','subjectAltName=DNS:nas.example.test','-keyout',str(private),'-out',str(certificate)],check=True,capture_output=True);private.chmod(0o600)
 subprocess.run([str(jdk/'bin/keytool'),'-importcert','-noprompt','-alias','fixture','-file',str(certificate),'-keystore',str(trust),'-storetype','PKCS12','-storepass',password],check=True,capture_output=True)
 source=root/'SnapshotTlsChecks.java';source.write_text(JAVA);compiled=subprocess.run([str(jdk/'bin/javac'),'--release','8','-d',str(root),str(source)],capture_output=True,text=True)
 if compiled.returncode:raise RuntimeError(compiled.stderr)
 captured=[];names=[];refused=[];context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(certificate,private);context.set_servername_callback(lambda connection,name,ctx:names.append(name))
 class Handler(socketserver.BaseRequestHandler):
  def handle(self):
   try:
    with context.wrap_socket(self.request,server_side=True)as connection:
     data=b''
     while b'\r\n\r\n'not in data:
      part=connection.recv(4096)
      if not part:return
      data+=part;assert len(data)<=65536
     header,payload=data.split(b'\r\n\r\n',1);length=next(int(line.split(b':',1)[1])for line in header.split(b'\r\n')if line.lower().startswith(b'content-length:'))
     while len(payload)<length:
      part=connection.recv(4096)
      if not part:return
      payload+=part
     captured.append((header,payload));connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK')
   except ssl.SSLError as error:refused.append(type(error).__name__)
 class Server(socketserver.ThreadingTCPServer):allow_reuse_address=True;daemon_threads=True
 server=Server(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 try:
  r=record('write');r['connection']['url']='https://127.0.0.1:'+str(server.server_address[1])+'/share/';r['target']=r['connection']['url']+'saved.bin';reply=request(r);assert'value'in reply,reply;e=reply['value'];call(dict(op='project_close',handle=e['snapshot_handle']))
  prefix=[str(jdk/'bin/java'),'-Djavax.net.ssl.trustStore='+str(trust),'-Djavax.net.ssl.trustStorePassword='+password,'-cp',str(root),'SnapshotTlsChecks','127.0.0.1',str(server.server_address[1])]
  accepted=subprocess.run(prefix+[e['tls_host'],e['wire_base64']],capture_output=True,text=True,timeout=20);check('actual production TLS binding completes a trusted DNS-alias handshake over an IP socket',accepted.returncode==0 and base64.b64decode(accepted.stdout).endswith(b'\r\n\r\nOK'))
  check('actual TLS server receives unchanged opaque payload and aliased HTTP authority',len(captured)==1 and captured[0][1]==body and b'Host: nas.example.test:'in captured[0][0])
  for name in ['', 'wrong.example.test']:
   rejected=subprocess.run(prefix+[name,e['wire_base64']],capture_output=True,text=True,timeout=20);check('production hostname verification rejects '+('the socket IP'if not name else'a mismatched DNS identity'),rejected.returncode!=0 and'SSLHandshakeException'in rejected.stderr)
  check('production TLS sends configured SNI while keeping failed peers from request bytes','nas.example.test'in names and'wrong.example.test'in names and len(captured)==1)
 finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close();thread.join(timeout=5)
report=dict(passed=len(passed),checks=passed,scope='Owner execution of public legacy project_payload/current project_chunks operations, opaque bytes and pinned connections; unchanged production Android TLS socket block executed on the host JVM against a real local certificate, per-process fixture trust, successful alias/SNI and refused hostname mismatches. No archive extraction, excluded historical fixtures or physical Android/live-user NAS acceptance is claimed.')
(PROJECT/'output/checks/snapshot_identity.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
