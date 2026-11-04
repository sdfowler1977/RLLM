"""Back-to-back authenticated workspace tasks release their actual execution slot."""
from pathlib import Path
import hashlib,json,subprocess,tempfile
PROJECT=Path(__file__).resolve().parents[1];passed=[];(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
with tempfile.TemporaryDirectory(dir=PROJECT/'output',prefix='server-admission-')as t:
 root=Path(t);source=root/'actual.txt';source.write_text('Actual sequential observation.');p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):
  p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,a,i):return op(dict(op='event',root=str(root),state=st,action=a,inputs=i))
 token='fixture-'+'z'*40;st=event(None,'launch',{})['state'];config=event(st,'gateway_start',dict(listen='127.0.0.1',port='8080',token=token,a2a=True))['effect']['config']
 def request(version,i):
  current=version=='1.0';message={'messageId':version+'-'+str(i),'role':'ROLE_USER'if current else'user','parts':[{'data':{'tool':'inspect','args':{'path':'actual.txt','content':'complete_digest'}}}]}
  if not current:message['kind']='message';message['parts'][0]['kind']='data'
  return{'jsonrpc':'2.0','id':i,'method':'SendMessage'if current else'message/send','params':{'message':message}}
 def send(version,request):
  body=json.dumps(request);header=f'POST /a2a HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(body.encode())}\r\nA2A-Version: {version}\r\nAuthorization: Bearer {token}\r\n\r\n';wire=op(dict(op='gateway_incoming',config=config,header=header,body=body))['reply'];r=json.loads(wire.split('\r\n\r\n',1)[1]);assert'result'in r,dict(version=version,response=r);return r['result']['task']if version=='1.0'else r['result']
 try:
  for version in ['1.0','0.3']:
   ids=set();completed=[];fresh=[]
   for i in range(128):
    payload=f'Actual source revision {version}/{i}.\n';source.write_text(payload);task=send(version,request(version,i));ids.add(task['id']);completed.append(task['status']['state']==('TASK_STATE_COMPLETED'if version=='1.0'else'completed'));output=task['artifacts'][0]['parts'][0]['data'];raw=output['output'];fresh.append(hashlib.sha256(payload.encode()).hexdigest()in raw and output['complete']and output['returnedSha256']==hashlib.sha256(raw.encode()).hexdigest())
   check(version+' sequential task acknowledgements leave capacity ready for the next task',all(completed)and len(completed)==128)
   check(version+' every admitted task returns its actual fresh source digest',all(fresh))
   check(version+' each fresh message owns a distinct actual task',len(ids)==128)
   source.write_text('Changed after the last completed task.');cached=send(version,request(version,127));check(version+' identical message retry preserves its historical result without replay',cached['id']==task['id']and cached['artifacts']==task['artifacts'])
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-interface checks: authenticated current/legacy local-agent protocol,256immediate sequential real file-inspection tasks with changed source bytes, complete source/output hashes, distinct identities and cached-message idempotency. No retries or sleeps between fresh requests.'};(PROJECT/'output/checks/server-admission.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
