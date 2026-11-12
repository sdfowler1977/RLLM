"""Proposal refusals return their actual reason through the ordinary command flow."""
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
 def messages(d):return next(c for c in d['state']['chats']if c['id']==d['state']['active'])['messages']
 def refused(d,tool,reason):return d['effect']is not None and d['effect']['purpose']=='model'and'"status":"rejected_before_start"'in prompt(d)and('"tool":"'+tool+'"')in prompt(d)and reason in prompt(d)
 try:
  (root/'actual.txt').write_text('ACTUAL_FILE_AFTER_REJECTION')
  for fmt in ['chat','local','messages','content']:
   d=reply(event(state(fmt),'send',{'draft':'Use the permitted file operations.'}),'write',{'path':'unapproved.txt','content':'UNAPPROVED_WRITE'});check(fmt+' disabled operation returns its actual refusal',refused(d,'write','disabled')and not(root/'unapproved.txt').exists()and not d['state'].get('pending'));check(fmt+' original proposal and diagnostic remain in the conversation',any('UNAPPROVED_WRITE'in m.get('content','')for m in messages(d))and any('rejected_before_start'in m.get('content','')for m in messages(d)))
   d=reply(d,'read',{'path':'actual.txt'});check(fmt+' corrected operation still requires ordinary review',d['state']['dialog']=='approval'and d['effect']is None);d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));d=event(d['state'],'tool_result',dict({k:e[k]for k in ['purpose','tool_ticket','tool_binding']if k in e},output=out));check(fmt+' separate reviewed worker returns its actual file contents','ACTUAL_FILE_AFTER_REJECTION'in prompt(d));d=reply(d,text='The reviewed file read returned its actual contents.')
   d=reply(event(state(fmt),'send',{'draft':'Read a workspace file.'}),'read',{});check(fmt+' malformed permitted arguments return their actual validation error',refused(d,'read','Missing path'));d=reply(d,text='The proposed read needs a path.');check(fmt+' model can finish after the argument refusal',d['effect']is None and not d['state']['busy'])
  st=state('chat');st['agents'][0]['tools']+=',web_read';d=reply(event(st,'send',{'draft':'Read the chosen page.'}),'web_read',{'url':'file:///private.txt'});check('invalid website proposal returns a refusal before any web request',refused(d,'web_read','Address must begin')and d['effect']['purpose']=='model'and not d['state'].get('web_expected'));d=reply(d,text='The URL must use the supported page scheme.')
  st=state('chat');st['routing']={'subagent':'2'};d=event(st,'delegate',{'task':'Read the selected file.'});d=reply(d,'write',{'path':'unapproved.txt','content':'TASK_UNAPPROVED'});check('standalone task receives the refusal on its assigned connection',refused(d,'write','disabled')and d['state']['request_index']==2 and any('TASK_UNAPPROVED'in m.get('content','')for m in d['state']['task_run']['tool_trail']));d=reply(d,text='Standalone task received the actual refusal.');check('standalone task saves its final answer after rejection',not d['state']['busy']and any('Standalone task received the actual refusal.'in m.get('content','')for m in messages(d)))
  st=state('chat');st['agents'][0]['model']='2';d=reply(event(st,'send',{'draft':'Use this configured model.'}),'read',{});check('refusal resumes the exact selected connection',refused(d,'read','Missing path')and d['state']['request_index']==2);d=reply(d,text='Selected connection received the actual refusal.')
  d=proposal(state('chat'),{'agent':1,'task':'Keep this queued child task.'});pending=copy.deepcopy(d['state']['pending']);d=event(d['state'],'send',{'draft':'What is waiting for review?'});d=reply(d,'read',{});check('refusal feedback preserves a previously queued child review',refused(d,'read','already awaiting review')and d['state']['pending']==pending and catalog(d)['pending_delegation']['task']['text']=='Keep this queued child task.');d=reply(d,text='The queued child task still awaits review.');d=event(d['state'],'review');d=event(d['state'],'approve');check('preserved queued review still starts its exact original child',d['state']['request_index']==1 and len(d['state']['delegation_stack'])==1);d=finish(d)
  d=reply(event(state('chat'),'send',{'draft':'Return a refusal for the current invalid proposal.'}),'read',{})
  for _ in range(2):d=reply(d,'read',{})
  check('invalid proposals share the bounded ordinary feedback allowance',d['effect']is not None and d['state']['local_failure_followups']==3 and d['state']['steps']==0)
  d=reply(d,'read',{});check('repeated invalid proposals settle without executing an operation',d['effect']is None and not d['state']['busy']and not d['state']['agent_running']and'Three operation error results'in d['state']['notice']and not(root/'unapproved.txt').exists())
  d=reply(event(d['state'],'send',{'draft':'A fresh instruction.'}),'read',{});check('fresh instruction resets proposal feedback allowance',refused(d,'read','Missing path')and d['state']['local_failure_followups']==1);d=reply(d,text='Fresh refusal received.')
  d=event(state('chat'),'send',{'draft':'Return an actual refusal under the current memory policy.'});d['state']['device']={'heap_bytes':2097152,'memory_bytes':4194304,'cores':1};d=reply(d,'read',{});check('response admission refusal retains the actual rejection and settles ownership',d['effect']is None and not d['state']['busy']and not d['state'].get('model_expected')and any('Missing path'in m.get('content','')for m in messages(d)))
  d=event(state('chat'),'send',{'draft':'Return the refusal locally after pausing remote requests.'});d['state']['capability_mode']='local';d=reply(d,'read',{});check('paused remote mode retains the refusal without a continuation request',d['effect']is None and not d['state']['busy']and any('Missing path'in m.get('content','')for m in messages(d)))
  d=proposal(state('chat'),{'agent':1,'task':'Use the permitted file operations.'});d=event(d['state'],'approve');d=reply(d,'write',{'path':'unapproved.txt','content':'CHILD_UNAPPROVED'});check('child refusal keeps its captured parent return',d['effect']is not None and d['state']['request_index']==0 and'disabled'in prompt(d)and not d['state'].get('delegation_stack')and not(root/'unapproved.txt').exists());d=reply(d,text='Parent received the actual child refusal.')
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-event checks: actual proposal/refusal/review transitions through four provider formats, real subsequent file workers, exact routes, bounded feedback, queued review preservation, response admission and captured child returns.'};(PROJECT/'output/checks/proposal-feedback.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
