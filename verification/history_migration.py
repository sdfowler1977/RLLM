"""Import a genuine prior-controller question with an owned remote-tool receipt."""
import argparse,base64,copy,json,pathlib,subprocess,tempfile
args=argparse.ArgumentParser();args.add_argument('--previous-controller',required=True);args=args.parse_args();PROJECT=pathlib.Path(__file__).resolve().parents[1];old=bytes([114,101,97]).decode();passed=[]
def check(name,value):assert value,name;passed.append(name)
with tempfile.TemporaryDirectory(prefix='migration-',dir=PROJECT/'output') as temp:
 root=pathlib.Path(temp);(root/'input.txt').write_text('Ordinary local input remains available.')
 class Controller:
  def __init__(self,path):self.p=subprocess.Popen([str(path),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  def op(self,v):
   self.p.stdin.write(json.dumps(v)+'\n');self.p.stdin.flush();r=json.loads(self.p.stdout.readline());assert 'value'in r,r;return r['value']
  def event(self,st,action,inputs=None):return self.op(dict(op='event',root=str(root),state=st,action=action,inputs=inputs or {}))
  def close(self):self.p.stdin.close();self.p.wait(timeout=10)
 def wire(value,status='200 OK'):
  b=json.dumps(value).encode()if value is not None else b'';return base64.b64encode(b'HTTP/1.1 '+status.encode()+b'\r\nContent-Type: application/json\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b).decode()
 prior=Controller(args.previous_controller)
 try:
  st=prior.event(None,'resume')['state'];st['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',protocol='chat',model='controlled',plain=True,stream=False)];st['agents']=[dict(name='General',tools='read,'+old)];st[old]={'endpoint':'http://127.0.0.1:9127/mcp','key':'fixture-key'}
  d=prior.event(st,'send',{'draft':'Obtain an actual peer observation.'});proposal=json.dumps({'tool':old,'args':{'name':'inspect_product','arguments':{}}});d=prior.event(d['state'],'network_result',{'purpose':'model','model_ticket':d['state']['model_expected'],'data':wire({'choices':[{'message':{'content':proposal},'finish_reason':'stop'}]})});assert d['state']['dialog']=='approval';d=prior.event(d['state'],'approve')
  for _ in range(8):
   e=d['effect'];assert e and e['kind']=='network',(d['state'].get('notice'),d['state'].get('dialog'))
   if e['purpose']=='model':break
   request=json.loads(e['wire'].split('\r\n\r\n',1)[1]);method=request['method']
   if method=='initialize':response={'jsonrpc':'2.0','id':request['id'],'result':{'protocolVersion':request['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'Fixture','version':'one'}}}
   elif method=='notifications/initialized':response=None
   elif method=='tools/call':response={'jsonrpc':'2.0','id':request['id'],'result':{'content':[{'type':'text','text':'Original controlled peer observation.'}]}}
   else:raise AssertionError(method)
   d=prior.event(d['state'],'network_result',{'purpose':e['purpose'],'data':wire(response,'202 Accepted' if response is None else '200 OK')})
  else:raise AssertionError('Peer operation did not settle')
  assert d['effect']['purpose']=='model';d=prior.event(d['state'],'network_result',{'purpose':'model','model_ticket':d['state']['model_expected'],'data':wire({'choices':[{'message':{'content':'Answer following the actual peer result.'},'finish_reason':'stop'}]})});saved=d['state'];case=next(c for c in saved['inquiry_records']['items']if c['evidence']);receipt=case['evidence'][0]
  check('reference controller creates actual owned historical receipt',receipt['tool']==old and 'Original controlled peer observation.'in receipt['output'])
 finally:prior.close()
 current=Controller(PROJECT/'output/headless/RLLM')
 try:
  d=current.event(saved,'resume');migrated=next(c for c in d['state']['inquiry_records']['items']if c['id']==case['id']);actual=migrated['evidence'][0]
  check('historical receipt uses current operation identifier',actual['tool']=='mcp')
  check('captured output bytes and digests are unchanged',all(actual[k]==receipt[k]for k in ['output','sha256','prefix_sha256','bytes','source_guard','args']))
  d=current.event(d['state'],'inquiry_select:'+case['id']);check('migrated historical question remains readable',not d['state'].get('notice_error',False) and d['state']['dialog']=='inquiry_view')
  d=current.event(d['state'],'inquiry_section:evidence');check('historical evidence is still available',d['state']['dialog']=='inquiry_view' and 'Original controlled peer observation.'in json.dumps(d['ui']))
  corrupted=copy.deepcopy(saved);c=next(c for c in corrupted['inquiry_records']['items']if c['id']==case['id']);c['evidence'][0]['output']='Tampered input without a matching original receipt'
  d=current.event(corrupted,'resume');d=current.event(d['state'],'inquiry_select:'+case['id']);check('migration refuses to bless altered receipt data',d['state'].get('notice_error',False))
  check('corrupt optional history does not block local operations',current.op({'op':'tool','root':str(root),'tool':'read','args':{'path':'input.txt'}})=='Ordinary local input remains available.')
 finally:current.close()
print(json.dumps({'passed':len(passed),'checks':passed,'scope':'Owner compatibility checks: actual prior and current controllers with controlled peer replies.'},indent=2));(PROJECT/'output/checks/history-migration.json').write_text(json.dumps({'passed':passed},indent=2)+'\n')
