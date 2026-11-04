"""Current review ownership survives later model proposals and status questions."""
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
  (root/'actual.txt').write_text('A separately reviewed read.')
  for fmt in ['chat','local','messages','content']:
   d=proposal(state(fmt),{'agent':1,'task':'ORIGINAL_REVIEW_TASK'});original=copy.deepcopy(d['state']['pending']);origin=original['delegate_question'];d=event(d['state'],'send',{'draft':'What is the original task status?'});d=reply(d,'read',{'path':'actual.txt'});check(fmt+' later valid proposal retains the exact current review',refused(d,'read','already awaiting review')and d['state']['pending']==original and not d['state'].get('delegation_stack'));check(fmt+' current child review remains visible to the model',catalog(d)['pending_delegation']['task']['text']=='ORIGINAL_REVIEW_TASK'and catalog(d)['pending_delegation']['originating_question']==origin)
   d=reply(d,'delegate',{'agent':1,'task':'ORIGINAL_REVIEW_TASK'});check(fmt+' duplicate child proposal retains the original review owner',refused(d,'delegate','already awaiting review')and d['state']['pending']==original)
   d=reply(d,'delegate',{'model':'GUESS','task':'REPLACEMENT_TASK'});check(fmt+' rejected child selector leaves the original review intact',refused(d,'delegate','already awaiting review')and d['state']['pending']==original)
   d=reply(d,text='The original child task is still awaiting review.');d=event(d['state'],'review');d=event(d['state'],'approve');child_id=d['state']['task_run']['inquiry_id'];case=next(c for c in d['state']['inquiry_records']['items']if c['id']==child_id);check(fmt+' original review starts only its exact child and question',d['state']['request_index']==1 and case['parent']==origin and d['state']['task_run']['task']=='ORIGINAL_REVIEW_TASK');d=finish(d)
  d=event(state('chat'),'editor');d=event(d['state'],'editor_save',{'path':'manual.txt','content':'PRIVATE_MANUAL_CONTENT'});original=copy.deepcopy(d['state']['pending']);d=event(d['state'],'send',{'draft':'What action is currently waiting?'});d=reply(d,'read',{'path':'actual.txt'});check('manual write review survives a conflicting model proposal',refused(d,'read','already awaiting review')and d['state']['pending']==original and not d['state']['agent_pending']);check('review conflict does not disclose unapproved manual arguments','PRIVATE_MANUAL_CONTENT'not in prompt(d)and'manual.txt'not in prompt(d));d=reply(d,text='The manual operation is awaiting review.');d=event(d['state'],'review');d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));d=event(d['state'],'tool_result',dict({k:e[k]for k in ['purpose','tool_ticket','tool_binding']if k in e},output=out));check('original manual review still executes its exact approved file operation',(root/'manual.txt').read_text()=='PRIVATE_MANUAL_CONTENT'and d['effect']is None and not d['state']['busy'])
  st=state('chat');st['agents'][0]['tools']+=',web_read';d=proposal(st,{'agent':1,'task':'Keep original child review.'});original=copy.deepcopy(d['state']['pending']);d=event(d['state'],'send',{'draft':'Report current status.'});d=reply(d,'web_read',{'url':'https://example.test/'});check('later web proposal preserves current review before any page dispatch',refused(d,'web_read','already awaiting review')and d['state']['pending']==original and not d['state'].get('web_expected'));d=reply(d,text='Original review is still current.');d=event(d['state'],'reject');check('explicit native dismissal clears the original review',d['state'].get('pending')is None);d=reply(event(d['state'],'send',{'draft':'Read actual.txt instead.'}),'read',{'path':'actual.txt'});check('a fresh operation is available after explicit dismissal',d['state']['dialog']=='approval'and d['state']['pending']['tool']=='read');d=event(d['state'],'reject')
  d=proposal(state('chat'),{'agent':1,'task':'Preserve through bounded conflicting feedback.'});original=copy.deepcopy(d['state']['pending']);d=event(d['state'],'send',{'draft':'Report status.'})
  for _ in range(3):d=reply(d,'read',{'path':'actual.txt'})
  d=reply(d,'read',{'path':'actual.txt'});check('feedback exhaustion retains the original current review',d['effect']is None and not d['state']['busy']and d['state']['pending']==original);d=event(d['state'],'review');d=event(d['state'],'approve');check('current review still works after conflicting feedback settles',d['effect']is not None and d['state']['request_index']==1);d=finish(d)
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-event checks: current review identity, four provider formats, queued child question/route ownership, duplicate/conflicting proposals, actual manual file execution, unapproved argument privacy and bounded feedback.'};(PROJECT/'output/checks/review-ownership.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
