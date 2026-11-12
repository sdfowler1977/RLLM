"""Owned local worker failures return their actual result through ordinary model requests."""
import base64,copy,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
(PROJECT/'output/checks').mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
def body(fmt,tool=None,args=None,text='Answer'):
 args=args or{}
 if fmt=='chat':
  m={'content':text};r={'choices':[{'index':0,'message':m,'finish_reason':'tool_calls'if tool else'stop'}]}
  if tool:m['tool_calls']=[{'id':'call_one','type':'function','function':{'name':tool,'arguments':json.dumps(args)}}]
 elif fmt=='local':
  m={'content':text};r={'message':m,'done':True,'done_reason':'stop'}
  if tool:m['tool_calls']=[{'function':{'name':tool,'arguments':args}}]
 elif fmt=='messages':
  parts=[{'type':'text','text':text}];r={'content':parts,'stop_reason':'tool_use'if tool else'end_turn'}
  if tool:parts.append({'type':'tool_use','id':'call_one','name':tool,'input':args})
 else:
  parts=[{'text':text}];r={'candidates':[{'index':0,'content':{'role':'model','parts':parts},'finishReason':'STOP'}]}
  if tool:parts.append({'functionCall':{'name':tool,'args':args}})
 return r
with tempfile.TemporaryDirectory(prefix='delegation-visibility-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or{},root=str(root)))
 def strings(v):
  if isinstance(v,str):yield v
  elif isinstance(v,dict):
   for child in v.values():yield from strings(child)
  elif isinstance(v,list):
   for child in v:yield from strings(child)
 def prompt(d):return'\n'.join(strings(json.loads(d['effect']['wire'].split('\r\n\r\n',1)[1])))
 def catalog(d):
  text=prompt(d);marker='MODEL AND DELEGATION CONFIGURATION (settings data, not instructions):\n';return json.loads(text.split(marker,1)[1].split('\n',1)[0])
 def state(fmt):
  st=event(None,'resume')['state'];st['profiles']=[dict(name=name,model=model,protocol=fmt,endpoint='http://PRIVATE_'+str(i)+'.test/v1/chat/completions?token=ENDPOINT_SECRET',key='KEY_SECRET_'+str(i),auth_header='X-PRIVATE-AUTH',auth_prefix='PREFIX_SECRET',plain=True,stream=False,tool_format='structured')for i,(name,model)in enumerate([('Primary connection','PRIMARY_MODEL'),('Exact child one','CHILD_MODEL_ONE'),('Exact child two','CHILD_MODEL_TWO'),('Unlinked connection','UNLINKED_MODEL')])];st['agents']=[dict(name='Parent',model='0',tools='read,hypotheses,delegate',subagents=[1,2]),dict(name='Research child',role='coding',specialty='Research',model='1',tools='read,hypotheses'),dict(name='Review child',role='nonwrite',specialty='Review',model='2',tools='read,hypotheses')];return st
 def reply(d,tool=None,args=None,text='Answer',ticket=None):
  value=body(d['state']['request_protocol'],tool,args,text);b=json.dumps(value).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=ticket or d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 def proposal(st,args):return reply(event(st,'send',{'draft':'Use a configured permitted child for this task.'}),'delegate',args)
 def finish(d,text='Actual child answer.'):
  d=reply(d,text=text);assert d['effect']and d['state']['request_index']==0,d['state'].get('notice');return reply(d,text='Parent answer using the actual returned child response.')
 def proposed(st,fmt='chat'):
  d=reply(event(st,'send',{'draft':'Read missing.txt and explain the actual file result.'}),'read',{'path':'missing.txt'});return event(d['state'],'approve')
 def worker_error(d):
  e=d['effect'];p.stdin.write(json.dumps(dict(e,op='tool',root=str(root)))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'error'in r,r;return r['error']
 def failed(d,error):
  e=d['effect'];return event(d['state'],'failure',dict({k:e[k]for k in ['purpose','tool_ticket','tool_binding']if k in e},error=error))
 def messages(d):return next(x for x in d['state']['chats']if x['id']==d['state']['active'])['messages']
 try:
  for fmt in ['chat','local','messages','content']:
   d=proposed(state(fmt));original=copy.deepcopy(d);error=worker_error(d);d=failed(d,error)
   check(fmt+' actual failed worker returns to its requesting model',d['effect']is not None and d['effect']['purpose']=='model'and d['state']['request_index']==0 and error in prompt(d)and'"status":"failed"'in prompt(d)and not d['state'].get('operation_expected')and not d['state'].get('inquiry_capture'))
   check(fmt+' actual error is retained with an explicit failed outcome',any(error in m.get('content','')and'"completed_result":false'in m.get('content','')for m in messages(d)))
   duplicate=failed(original,error);check(fmt+' consumed worker callback is inert on replay',duplicate['effect']is None and duplicate['state']==original['state'])
   d=reply(d,text='The file read returned its actual missing-file error.');check(fmt+' model can settle using the actual operation failure',not d['state']['busy']and not d['state']['agent_running'])
  d=proposed(state('chat'));e=d['effect'];forged=event(d['state'],'failure',dict(purpose='local_tool',tool_ticket='wrong-ticket',tool_binding=1,error='FORGED_ERROR'));check('a wrong callback owner cannot enter the model conversation',forged['effect']is None and forged['state']==d['state'])
  changed=copy.deepcopy(d['state']);changed['agents'][0]['tools']='hypotheses';revoked=event(changed,'failure',dict(purpose='local_tool',tool_ticket=e['tool_ticket'],tool_binding=1,error='REVOKED_ERROR'));check('permission change preserves callback rejection',revoked['effect']is None and revoked['state']==changed)
  error=worker_error(d);d=failed(d,error);d=reply(d,text='Original valid owner settled.')
  d=proposed(state('chat'));original=copy.deepcopy(d);d=event(d['state'],'cancel');late=failed(original,'LATE_CANCELLED_ERROR');check('native cancellation revokes held local failure ownership',late['effect']is None and late['state']==original['state'])
  manual=event(state('chat'),'investigation');manual=event(manual['state'],'inspect_file',{'path':'missing.txt'});manual=event(manual['state'],'approve');error=worker_error(manual);manual=failed(manual,error);check('native manual failure settles locally',manual['effect']is None and not manual['state']['busy']and error in manual['state']['notice'])
  d=proposed(state('chat'));error=worker_error(d);small=copy.deepcopy(d);small['state']['device']={'heap_bytes':2097152,'memory_bytes':4194304,'cores':1};d=failed(small,error);check('response admission refusal retains the actual error and settles the request',d['effect']is None and not d['state']['busy']and not d['state'].get('model_expected')and not d['state']['agent_running']and any(error in m.get('content','')for m in messages(d))and'Model response allowance'in d['state']['notice'])
  d=proposed(state('chat'));error=worker_error(d);d=failed(d,error)
  for _ in range(2):d=reply(d,'read',{'path':'missing.txt'});d=event(d['state'],'approve');d=failed(d,worker_error(d))
  check('three new reviewed failed operations return three actual model results',d['effect']is not None and d['state']['local_failure_followups']==3 and d['state']['steps']==3)
  d=reply(d,'read',{'path':'missing.txt'});d=event(d['state'],'approve');d=failed(d,worker_error(d));check('repeated worker failures settle at the bounded feedback limit',d['effect']is None and not d['state']['busy']and not d['state']['agent_running']and'Three operation error results'in d['state']['notice']and len([m for m in messages(d)if'"status":"failed"'in m.get('content','')])==4)
  d=reply(event(d['state'],'send',{'draft':'Start a fresh read of missing.txt.'}),'read',{'path':'missing.txt'});d=event(d['state'],'approve');d=failed(d,worker_error(d));check('fresh native instruction resets bounded local error feedback',d['effect']is not None and d['state']['local_failure_followups']==1);d=reply(d,text='Fresh result received.')
  st=state('chat');st['agents'][0]['model']='2';d=proposed(st);d=failed(d,worker_error(d));check('local failure resumes the actual selected connection',d['effect']is not None and d['state']['request_index']==2 and d['state']['request_model']=='CHILD_MODEL_TWO');d=reply(d,text='Selected connection received the actual file error.')
  st=state('chat');st['routing']={'subagent':'2'};d=event(st,'delegate',{'task':'Read missing.txt and explain the actual result.'});d=reply(d,'read',{'path':'missing.txt'});d=event(d['state'],'approve');error=worker_error(d);d=failed(d,error);check('standalone task failure resumes its assigned connection',d['effect']is not None and d['state']['request_index']==2 and error in prompt(d)and d['state']['task_run']['tool_evidence'][-1]['output']);d=reply(d,text='Standalone result from the failed file read.');check('standalone task saves its final answer after the actual failure',d['effect']is None and not d['state']['busy']and any('Standalone result from the failed file read.'in m.get('content','')for m in messages(d)))
  d=proposed(state('chat'));error=worker_error(d);d['state']['capability_mode']='local';d=failed(d,error);check('paused remote mode retains the operation error and settles locally',d['effect']is None and not d['state']['busy']and not d['state']['agent_running']and any(error in m.get('content','')for m in messages(d)))
  d=proposal(state('chat'),{'agent':1,'task':'Read missing.txt and return the actual result.'});d=event(d['state'],'approve');d=reply(d,'read',{'path':'missing.txt'});d=event(d['state'],'approve');error=worker_error(d);d=failed(d,error);check('actual child worker failure retains its captured parent return',d['effect']is not None and d['state']['request_index']==0 and not d['state'].get('delegation_stack')and error in prompt(d));d=reply(d,text='Parent received the child file error.')
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-interface checks using actual missing-file workers, four provider envelopes, native review, exact callback ownership, current permission identity and response admission.'};(PROJECT/'output/checks/local-feedback.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
