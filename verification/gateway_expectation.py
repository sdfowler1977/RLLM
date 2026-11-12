"""Current gateway header admission: real sockets and authenticated file tasks."""
import hashlib,json,pathlib,select,socket,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1]
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
passed=[]
def check(name,value):
 assert value,name
 passed.append(name)
with tempfile.TemporaryDirectory(dir=PROJECT/'output',prefix='gateway-expectation-')as temp:
 root=pathlib.Path(temp);payload='Actual selected file for expectation handling.\n';(root/'文.txt').write_text(payload)
 token='gateway-expectation-'+'z'*40;key=root/'token';key.write_text(token);key.chmod(0o600)
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--a2a','--root',str(root),'--token-file',str(key),'--port','0'],stderr=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 try:
  check('listener starts',bool(select.select([p.stderr],[],[],10)[0]));port=int(p.stderr.readline().rstrip().rsplit(':',1)[1].split('/')[0])
  def header(method,path,length,expect=None,auth=token,extra=''):
   return(f'{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n'+(f'Expect: {expect}\r\n'if expect is not None else'')+extra+'\r\n').encode()
  def response(f):
   raw=b''
   while not raw.endswith(b'\r\n\r\n'):
    b=f.read(1);assert b,'Peer closed before a complete response';raw+=b;assert len(raw)<16384
   code=int(raw.split(b' ',2)[1]);fields={k.lower():v.strip()for k,v in(line.split(b':',1)for line in raw.split(b'\r\n')[1:-2])}
   size=int(fields.get(b'content-length',b'0'));body=f.read(size);assert len(body)==size
   return code,raw,body
  def connect():return socket.create_connection(('127.0.0.1',port),timeout=2)
  for version,expect,early_body in [('1.0','100-continue',False),('0.3','100-CoNtInUe',False),('1.0','100-continue, 100-CONTINUE',False),('1.0','100-continue',True)]:
   current=version=='1.0';mid='observed-'+str(len(passed));part={'data':{'tool':'inspect','args':{'path':'文.txt','content':'complete_digest'}}};message={'messageId':mid,'role':'ROLE_USER'if current else'user','parts':[part]}
   if not current:message['kind']='message';part['kind']='data'
   body=json.dumps({'jsonrpc':'2.0','id':mid,'method':'SendMessage'if current else'message/send','params':{'message':message}},ensure_ascii=False).encode()
   with connect()as c,c.makefile('rb')as f:
    h=header('POST','/a2a',len(body),expect,extra=f'A2A-Version: {version}\r\n');c.sendall(h+(body if early_body else b''));code,raw,_=response(f)
    check(version+' receives interim before body wait '+mid,code==100 and raw==b'HTTP/1.1 100 Continue\r\n\r\n')
    if not early_body:c.sendall(body)
    code,_,reply=response(f);v=json.loads(reply);task=v['result']['task']if current else v['result'];output=task['artifacts'][0]['parts'][0]['data']
    check(version+' final response returns actual selected file '+mid,code==200 and hashlib.sha256(payload.encode()).hexdigest()in output['output']and output['returnedSha256']==hashlib.sha256(output['output'].encode()).hexdigest())
  for name,expect,auth,length,extra,wanted in [
   ('wrong bearer with expectation','100-continue','wrong',2,'',401),
   ('wrong bearer without expectation',None,'wrong',2,'',401),
   ('unknown expectation','fancy-upload',token,2,'',417),
   ('mixed expectation','100-continue, fancy-upload',token,2,'',417),
   ('authentication before expectation disclosure','fancy-upload','wrong',2,'',401),
   ('duplicate expectation','100-continue',token,2,'Expect: 100-continue\r\n',400),
   ('oversized framed body','100-continue',token,1048577,'',400),
   ('empty body has no interim','100-continue',token,0,'',200)]:
   with connect()as c,c.makefile('rb')as f:
    c.sendall(header('GET','/health',length,expect,auth,extra));code,raw,_=response(f)
    check(name+' returns final before any body upload',code==wanted and b'Connection: close\r\n'in raw)
    check(name+' closes only this connection',f.read(1)==b'')
  with connect()as c,c.makefile('rb')as f:
   c.sendall(header('GET','/health',0));check('ordinary authenticated local health remains usable',response(f)[0]==200)
 finally:
  p.terminate();p.wait(timeout=10)
result=dict(passed=passed,count=len(passed),scope='Owner execution: real loopback HTTP, no body sent before continue/final decisions, both A2A versions and actual file/output digests. No installed Android or independent acceptance.')
(PROJECT/'output/checks/gateway-expectation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
