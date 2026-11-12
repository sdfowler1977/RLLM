import tempfile
import subprocess,json,pathlib,base64,hashlib
PROJECT=pathlib.Path(__file__).resolve().parents[1]
(PROJECT/'output').mkdir(exist_ok=True)
testdir=tempfile.TemporaryDirectory(prefix='checks-',dir=PROJECT/'output')
RESULTS=PROJECT/'output'/'checks';RESULTS.mkdir(exist_ok=True)
root=testdir.name;pathlib.Path(root,'evidence.txt').write_text('Observed evidence from an actual held file.')
p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
passed=[]
def op(v):
 p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert 'value' in r,r;return r['value']
def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=root))
def body(fmt,tool=None,args=None,text='Answer'):
 args=args or {};reason='stop'
 if fmt=='chat':
  m={'content':text};r={'choices':[{'index':0,'message':m,'finish_reason':'tool_calls' if tool else 'stop'}]}
  if tool:m['tool_calls']=[{'id':'call_one','type':'function','function':{'name':tool,'arguments':json.dumps(args)}}]
 elif fmt=='local':
  m={'content':text};r={'message':m,'done':True,'done_reason':'stop'}
  if tool:m['tool_calls']=[{'function':{'name':tool,'arguments':args}}]
 elif fmt=='messages':
  parts=[{'type':'text','text':text}];r={'content':parts,'stop_reason':'tool_use' if tool else 'end_turn'}
  if tool:parts.append({'type':'tool_use','id':'call_one','name':tool,'input':args})
 else:
  parts=[{'text':text}];r={'candidates':[{'index':0,'content':{'role':'model','parts':parts},'finishReason':'STOP'}]}
  if tool:parts.append({'functionCall':{'name':tool,'args':args}})
 return r
def raw(b):
 b=json.dumps(b).encode();return base64.b64encode(b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b).decode()
def reply(d,b):
 e=d['effect'];i={'purpose':'model','model_ticket':d['state']['model_expected'],'data':raw(b)}
 for k in ['child_ticket','child_owner']:
  if k in e:i[k]=e[k]
 return event(d['state'],'network_result',i)
try:
 for fmt in ['chat','local','messages','content']:
  st=event(None,'resume')['state'];st['dialog']=None
  st['profiles']=[dict(name='Connection '+str(i),protocol=fmt,endpoint='http://model'+str(i)+'.test/v1/chat/completions',model='model-'+str(i),plain=True,stream=False,tool_format='structured') for i in range(2)]
  st['agents']=[dict(name='Parent',model='0',tools='read,hypotheses,delegate',subagents=[1]),dict(name='Child',model='1',tools='read,hypotheses',subagents=[])]
  d=event(st,'send',{'draft':'Investigate the evidence file and design an original answer.'});assert '"tools"' in d['effect']['wire'];passed.append(fmt+' declaration');assert d['state']['inquiry_records']['items'][-1]['plan']is None and d['state']['inquiry_records']['items'][-1]['question'].startswith('Investigate the evidence file');passed.append(fmt+' normal conversation starts a question without manual forms')
  d=reply(d,body(fmt,'read',{'path':'evidence.txt'},''));assert d['state']['dialog']=='approval';passed.append(fmt+' review')
  d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=root));assert out=='Observed evidence from an actual held file.'
  i={k:v for k,v in e.items() if k in ['purpose','tool_ticket','tool_binding','child_ticket','child_owner']};i['output']=out;d=event(d['state'],'tool_result',i)
  c=d['state']['inquiry_records']['items'][-1];assert c['evidence'][-1]['output']==out;passed.append(fmt+' held worker and retained evidence')
  actions=[{'action':'plan','questions':['What was observed?'],'requirements':['Use original observations'],'constraints':['Do not copy an implementation','Implementation language: '+{'chat':'Rust','local':'C++','messages':'C#','content':'Swift'}[fmt]]},{'action':'note','kind':'design','text':'Author an original design from the observed interface.','citations':[1]},{'action':'check','evidence':1,'method':'equals','expected':out,'requirement':'Use original observations'}]
  for a in actions:
   d=reply(d,body(fmt,'workspace',{'tool':'hypotheses','arguments_json':json.dumps({'process':a})},''));assert d['state']['dialog']=='approval';d=event(d['state'],'approve');assert d['effect']['purpose']=='model';passed.append(fmt+' '+a['action'])
  d=reply(d,body(fmt,'hypotheses',{'process':{'action':'brief'}},''));assert d['state']['dialog']=='approval';d=event(d['state'],'approve');assert d['effect']['purpose']=='model'
  request=json.loads(d['effect']['wire'].split('\r\n\r\n',1)[1])
  def texts(v):
   if isinstance(v,str):yield v
   elif isinstance(v,dict):
    for child in v.values():yield from texts(child)
   elif isinstance(v,list):
    for child in v:yield from texts(child)
  brief=next(t for t in texts(request)if '"kind":"design_brief"'in t);assert out not in brief and 'Implementation language:'in brief;assert 'Author an original design from the observed interface.'in brief;passed.append(fmt+' ordinary model plans produce a brief without raw returned source')
  c=d['state']['inquiry_records']['items'][-1];digest=hashlib.sha256(json.dumps(c['plan'],sort_keys=True,separators=(',',':')).encode()).hexdigest()
  a={'action':'assess','plan_sha256':digest,'item':'requirements','index':1,'status':'answered','explanation':'The actual worker result was retained and compared.','citations':[1],'checks':[1]}
  d=reply(d,body(fmt,'hypotheses',{'process':a},''));d=event(d['state'],'approve');assert d['state']['inquiry_records']['items'][-1]['assessments'][-1]['status']=='answered';passed.append(fmt+' assessment')
  d=reply(d,body(fmt,'delegate',{'agent':1,'task':'Reconcile the observation'},''));d=event(d['state'],'approve');assert d['state']['request_index']==1;d=reply(d,body(fmt,text='Actual child reply'));assert d['state']['request_index']==0 and 'Actual child reply' in d['effect']['wire'];passed.append(fmt+' captured child return')
  d=reply(d,body(fmt,text='Original answer based on the retained observations.'));assert not d['state']['busy'] and d['effect'] is None;passed.append(fmt+' final answer')
  d=event(st,'send',{'draft':'Have the child inspect the problem.'});d=reply(d,body(fmt,'delegate',{'agent':1,'task':'Investigate'},''));d=event(d['state'],'approve');child_effect=d['effect'];child_ticket=d['state']['model_expected']
  i={k:child_effect[k] for k in ['child_ticket','child_owner'] if k in child_effect};i.update(purpose='model',model_ticket=child_ticket,error='Controlled child transport failure',sent=False)
  failed=event(d['state'],'failure',i);assert failed['state']['request_index']==0 and failed['effect']['purpose']=='model' and 'Controlled child transport failure' in failed['effect']['wire'];passed.append(fmt+' child failure returns to captured parent')
  duplicate=event(failed['state'],'failure',i);assert duplicate['state']==failed['state'] and duplicate['effect'] is None;passed.append(fmt+' duplicate child failure cannot disturb parent request')
  d=reply(failed,body(fmt,text='The child could not complete its request.'));assert not d['state']['busy'];passed.append(fmt+' parent can answer after child failure')
  d=event(st,'send',{'draft':'Investigate'});d=event(d['state'],'cancel');late=reply(dict(state=d['state'],effect={}),body(fmt,'read',{'path':'evidence.txt'},''));assert not late['state']['busy'] and late['state'].get('pending') is None;passed.append(fmt+' cancelled callback refusal')
 (RESULTS/'current-results.json').write_text(json.dumps({'passed':passed,'provider_replies':'controlled fixtures; actual file worker and process execution'},indent=2));print(len(passed),'fresh connected checks passed')
finally:p.stdin.close();p.wait();testdir.cleanup()
