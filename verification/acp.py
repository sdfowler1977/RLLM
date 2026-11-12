"""Actual standard-stream ACP sessions, owned workspace execution and cancellation."""
import hashlib,json,pathlib,queue,subprocess,tempfile,threading

PROJECT=pathlib.Path(__file__).resolve().parents[1]
BIN=PROJECT/'output/headless/RLLM'
passed=[]
def check(name,value):
    assert value,name
    passed.append(name)

class Client:
    def __init__(self,root):
        self.process=subprocess.Popen([str(BIN),'--acp','--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.queue=queue.Queue();self.frames=[]
        def read():
            for line in self.process.stdout:self.queue.put(json.loads(line))
            self.queue.put(None)
        self.reader=threading.Thread(target=read,daemon=True);self.reader.start()
    def raw(self,data):
        self.process.stdin.write(data);self.process.stdin.flush()
    def send(self,id,method,params=None):
        v=dict(jsonrpc='2.0',method=method,params=params or{})
        if id is not None:v['id']=id
        self.raw(json.dumps(v).encode()+b'\n')
    def receive(self):
        v=self.queue.get(timeout=15);assert v is not None,'ACP exited before replying';self.frames.append(v);return v
    def until(self,id):
        start=len(self.frames)
        while True:
            v=self.receive()
            if v.get('id')==id:return v,self.frames[start:]
    def call(self,id,method,params=None):
        self.send(id,method,params);return self.until(id)[0]
    def session(self,id,root):
        response=self.call(id,'session/new',dict(cwd=str(root),mcpServers=[]));return response['result']['sessionId']
    def prompt(self,id,session,text,links=None):
        self.send(id,'session/prompt',dict(sessionId=session,prompt=[dict(type='text',text=text)]+(links or[])));return self.until(id)
    def close(self):
        self.process.stdin.close();self.process.wait(timeout=15);self.reader.join(timeout=1)
        assert self.process.returncode==0,self.process.stderr.read().decode()

with tempfile.TemporaryDirectory(prefix='acp-',dir=PROJECT/'output')as directory:
    base=pathlib.Path(directory);root=base/'workspace';root.mkdir();outside=base/'outside';outside.mkdir()
    a=root/'sample.txt';a.write_text('actual ACP workspace content\n');b=root/'other.txt';b.write_text('different bytes\n');secret=outside/'private.txt';secret.write_text('outside selected root\n');(root/'escape').symlink_to(secret)
    with(root/'large.bin').open('wb')as f:f.truncate(32*1024*1024)
    c=Client(root)
    try:
        r=c.call(1,'session/new',dict(cwd=str(root),mcpServers=[]));check('session creation requires initialization',r['error']['code']==-32600)
        r=c.call(2,'initialize',dict(protocolVersion=2,clientCapabilities={},clientInfo=dict(name='Fixture',version='1')));cap=r['result']['agentCapabilities'];check('unknown requested version negotiates supported stable version',r['result']['protocolVersion']==1);check('unsupported media and peer launch capabilities are not advertised',not cap['promptCapabilities']and not cap['mcpCapabilities']and not cap['loadSession']);check('implemented session extensions are advertised',all(k in cap['sessionCapabilities']for k in ['list','resume','close']))
        r=c.call(3,'initialize',dict(protocolVersion=1));check('duplicate initialization cannot replace connection state','error'in r)
        r=c.call(4,'session/new',dict(cwd=str(outside),mcpServers=[]));check('client cannot widen startup workspace',r['error']['code']==-32602)
        r=c.call(5,'session/new',dict(cwd=str(root),mcpServers=[dict(name='Peer',command='/bin/false',args=[],env=[])]));check('unadvertised external server launch is refused','error'in r)
        r=c.call(6,'session/new',dict(cwd=str(root),mcpServers=[],additionalDirectories=[str(outside)]));check('additional roots cannot be silently accepted','error'in r)
        session=c.session(7,root)
        r,frames=c.prompt(8,session,'/inspect sample.txt');updates=[f['params']['update']for f in frames if f.get('method')=='session/update'];complete=next(u for u in updates if u['sessionUpdate']=='tool_call_update');raw=complete['rawOutput'];body=''.join(u['content']['text']for u in updates if u['sessionUpdate']=='agent_message_chunk');check('real engine output reaches ACP agent messages',complete['status']=='completed'and body==raw['output']and'sample.txt'in body);check('returned output digest describes actual returned bytes',raw['complete']and raw['returnedBytes']==len(body.encode())and raw['returnedSha256']==hashlib.sha256(body.encode()).hexdigest());check('tool start and completion precede prompt completion',next(u for u in updates if u['sessionUpdate']=='tool_call')['status']=='in_progress'and r['result']['stopReason']=='end_turn')
        r=c.call(9,'session/list');check('session list returns owned selected-root session',r['result']['sessions']==[dict(sessionId=session,cwd=str(root),title='Workspace observations')]);r=c.call(10,'session/resume',dict(sessionId=session,cwd=str(root),mcpServers=[]));check('existing in-process session resumes without fabricated history',r['result']=={})
        links=[dict(type='resource_link',name='sample.txt',uri=a.as_uri()),dict(type='resource_link',name='other.txt',uri=b.as_uri())];r,frames=c.prompt(11,session,'/compare',links);check('local resource links execute actual comparison',any(f.get('params',{}).get('update',{}).get('status')=='completed'for f in frames))
        r,frames=c.prompt(12,session,json.dumps(dict(tool='inspect',args=dict(path='sample.txt',content='complete_digest'))));check('JSON command uses actual whole-file digest',any(hashlib.sha256(a.read_bytes()).hexdigest()in json.dumps(f)for f in frames))
        for number,text in enumerate(['/inspect ../outside/private.txt','/inspect escape','/inspect missing.txt'],20):
            r,frames=c.prompt(number,session,text);check('scoped read refusal '+text,any(f.get('params',{}).get('update',{}).get('status')=='failed'for f in frames)or'error'in r);check('refusal exposes no outside content '+text,all('outside selected root'not in json.dumps(f)for f in frames))
        for number,text in enumerate([json.dumps(dict(tool='write',args=dict(path='created.txt',content='bad'))),json.dumps(dict(tool='inspect',args=dict(path='sample.txt'),root=str(outside))),'/compare not-json'],30):
            r,_=c.prompt(number,session,text);check('unsupported authority or malformed command refused '+str(number),'error'in r)
        check('refused write did not create a file',not(root/'created.txt').exists())
        r,_=c.prompt(34,session,'/inspect',[dict(type='resource_link',name='private',uri=secret.as_uri())]);check('outside resource links are refused','error'in r)
        r=c.call(35,'session/prompt',dict(sessionId=session,prompt=[dict(type='audio',mimeType='audio/wav',data='AAAA')]));check('unadvertised audio is not interpreted as text','error'in r)
        r,_=c.prompt(36,session,'/inspect sample.txt');check('unrelated valid operation still works after scoped failures',r['result']['stopReason']=='end_turn')
        c.raw(b'{not-json}\n');r=c.receive();check('malformed JSON receives a parse error',r['error']['code']==-32700)
        c.raw(b'x'*(64*1024+1)+b'\n');r=c.receive();check('oversized frame is drained within declared bound',r['error']['code']==-32700)
        r=c.call(37,'session/list');check('valid request after oversized frame remains usable',session in json.dumps(r))
        session2=c.session(38,root);command=dict(jsonrpc='2.0',id=40,method='session/prompt',params=dict(sessionId=session,prompt=[dict(type='text',text='/inspect {"path":"large.bin","content":"complete_digest"}')]))
        cancel=dict(jsonrpc='2.0',method='session/cancel',params=dict(sessionId=session));c.raw((json.dumps(command)+'\n'+json.dumps(cancel)+'\n').encode());r,frames=c.until(40);check('actual in-flight work cancels with stable prompt stop reason',r['result']['stopReason']=='cancelled');check('cancelled work publishes no agent result',not any(f.get('params',{}).get('update',{}).get('sessionUpdate')=='agent_message_chunk'for f in frames));r,_=c.prompt(41,session2,'/inspect sample.txt');check('one session cancellation leaves another session usable',r['result']['stopReason']=='end_turn')
        command['id']=42;cancel=dict(jsonrpc='2.0',method='$/cancel_request',params=dict(requestId=42));c.raw((json.dumps(command)+'\n'+json.dumps(cancel)+'\n').encode());r,_=c.until(42);check('request cancellation retains original request identity',r['result']['stopReason']=='cancelled')
        command['id']=43;closing=dict(jsonrpc='2.0',id=44,method='session/close',params=dict(sessionId=session));c.raw((json.dumps(command)+'\n'+json.dumps(closing)+'\n').encode());r,frames=c.until(44);check('session close waits for owned turn cancellation',r['result']=={}and any(f.get('id')==43 and f.get('result',{}).get('stopReason')=='cancelled'for f in frames));r=c.call(45,'session/resume',dict(sessionId=session,cwd=str(root),mcpServers=[]));check('closed session cannot revive','error'in r)
        r=c.call(46,'session/close',dict(sessionId=session2));check('idle close releases session capacity',r['result']=={})
        owned=[c.session(50+n,root)for n in range(8)]
        r=c.call(60,'session/new',dict(cwd=str(root),mcpServers=[]));check('bounded session admission reports actual capacity','error'in r)
        root.rename(base/'original');root.mkdir();(root/'sample.txt').write_text('replacement root\n');r,_=c.prompt(61,owned[-1],'/inspect sample.txt');check('replaced startup root cannot feed stale session','identity changed'in r.get('error',{}).get('message',''));check('replacement root contents are not adopted',all('replacement root'not in json.dumps(f)for f in c.frames))
    finally:c.close()

record=dict(passed=len(passed),checks=passed,scope='Owner checks through actual ACP standard streams and shared production workspace engine; no model, external server, developer report or journal in the selected workspace. Read-only ACP v1 capability scope, not full coding-agent or Android ACP client acceptance.')
out=PROJECT/'output/checks';out.mkdir(exist_ok=True,parents=True);(out/'acp.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
