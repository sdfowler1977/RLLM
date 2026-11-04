"""Owned WebDAV errors continue the requesting command with their actual outcome."""
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
 def drive_state(fmt='chat'):
  st=state(fmt);st['drive']={'url':'https://nas.test/share/','user':'fixture','password':'fixture'};st['agents'][0]['tools']='read,delegate,drive_list,drive_read,drive_create,drive_update,drive_folder';st['agents'][1]['tools']='drive_read';return st
 def request(st,tool='drive_read',args=None):
  d=reply(event(st,'send',{'draft':'Use the configured network folder and report the actual result.'}),tool,args if args is not None else {'path':'missing.txt'});assert d['state']['dialog']=='approval',d['state'].get('notice');d=event(d['state'],'approve');assert d['effect'],d['state'].get('notice');return d
 def response(d,body='',status=200,headers=None,source=None):
  e=(source or d)['effect'];b=body.encode();h={'Content-Length':str(len(b))};h.update(headers or{});wire=('HTTP/1.1 %d Result\r\n'%status+''.join(k+': '+v+'\r\n'for k,v in h.items())+'\r\n').encode()+b;i={k:e[k]for k in ['purpose','drive_ticket','child_ticket']if k in e};i['data']=base64.b64encode(wire).decode();return event(d['state'],'network_result',i)
 def failed(d,reason,source=None):
  e=(source or d)['effect'];return event(d['state'],'failure',dict({k:e[k]for k in ['purpose','drive_ticket','child_ticket']if k in e},error=reason,sent=False))
 def messages(d):return next(c for c in d['state']['chats']if c['id']==d['state']['active'])['messages']
 def returned(name,d,tool,reason):
  check(name,d['effect']is not None and d['effect']['purpose']=='model'and d['state']['request_index']==0 and reason in prompt(d)and'"status":"failed"'in prompt(d)and('"tool":"'+tool+'"')in prompt(d)and not d['state'].get('drive_expected')and not d['state'].get('inquiry_operation'))
 try:
  for fmt in ['chat','local','messages','content']:
   d=request(drive_state(fmt));old=copy.deepcopy(d);d=response(d,'Missing file',404);returned(fmt+' missing network file returns its owned HTTP error',d,'drive_read','404');check(fmt+' failed outcome is saved in the requesting conversation',any('404'in m.get('content','')and'"completed_result":false'in m.get('content','')for m in messages(d)));duplicate=response(old,'Missing file',404);check(fmt+' consumed network error callback stays inert',duplicate['effect']is None and duplicate['state']==old['state']);d=reply(d,text='Actual network read error received.');check(fmt+' command can finish after the actual network error',not d['state']['busy']and not d['state']['agent_running'])
  d=request(drive_state());d=failed(d,'CONTROLLED_NAS_TRANSPORT');returned('current transport error returns through the normal command flow',d,'drive_read','CONTROLLED_NAS_TRANSPORT');d=reply(d,text='The network transport returned its error.')
  d=request(drive_state(),'drive_list',{'path':''});d=response(d,'<broken>',207);returned('malformed current folder response returns an actual parser error',d,'drive_list','Expected one WebDAV multistatus root');d=reply(d,'drive_read',{'path':'usable.txt'});d=event(d['state'],'approve');d=response(d,'ACTUAL_VALID_FILE');check('a scoped folder error leaves a separately reviewed read usable','ACTUAL_VALID_FILE'in prompt(d)and'"status":"failed"'in prompt(d));d=reply(d,text='The separate file read returned its actual text.')
  d=request(drive_state(),'drive_folder',{'path':'new-folder'});d=response(d,'Forbidden',403);returned('folder creation refusal returns its actual error',d,'drive_folder','403');d=reply(d,text='The folder request returned403.')
  d=request(drive_state(),'drive_create',{'path':'new.txt','content':'Expected'});write=copy.deepcopy(d);d=response(d,'Precondition failed',412);returned('refused network create returns its actual outcome',d,'drive_create','412');check('failed create retains its unconfirmed outcome without replay',d['state']['drive_creation']['phase']=='unconfirmed'and d['effect']['purpose']=='model');d=reply(d,text='The create outcome was returned explicitly.')
  d=request(drive_state(),'drive_create',{'path':'new.txt','content':'Expected'});d=response(d,'',201);verify=copy.deepcopy(d);check('create acknowledgement still waits for exact readback',d['effect']['purpose']=='drive_create_verify'and d['effect']['wire'].startswith('GET '));d=response(d,'Different');returned('mismatched create readback reaches the requesting model',d,'drive_create','readback');check('mismatched readback remains unconfirmed with a model-only continuation',d['state']['drive_creation']['phase']=='unconfirmed'and d['effect']['purpose']=='model');d=reply(d,text='Readback mismatch received.')
  d=request(drive_state(),'drive_create',{'path':'new.txt','content':'Expected'});d=response(d,'',201);d=failed(d,'CURRENT_READBACK_TRANSPORT');returned('readback transport error reaches the requesting model',d,'drive_create','CURRENT_READBACK_TRANSPORT');check('readback transport error preserves unconfirmed file state',d['state']['drive_creation']['phase']=='unconfirmed');d=reply(d,text='Verification transport error received.')
  d=request(drive_state(),'drive_create',{'path':'new.txt','content':'Expected'});d=response(d,'',201);d['state']['capability_mode']='local';d=response(d,'Expected');check('verified create remains verified when model continuation is paused',d['effect']is None and d['state'].get('drive_creation')is None and any('readback verified'in m.get('content','')for m in messages(d))and not any('"status":"failed"'in m.get('content','')for m in messages(d))and'unconfirmed'not in d['state']['notice'])
  d=request(drive_state(),'drive_update',{'path':'existing.txt','content':'New'});d=response(d,'Old',200,{'ETag':'"prior"'});d=response(d,'',201);d=response(d,'Old');check('revision reaches target write only after exact backup verification',d['effect']['purpose']=='drive_update_write'and'If-Match: "prior"'in d['effect']['wire']);d=response(d,'',204);d=response(d,'Different');returned('target readback mismatch reaches the model with actual revision details',d,'drive_update','Prior contents remain saved');check('revision failure does not dispatch another write',d['effect']['purpose']=='model'and d['state'].get('drive_revision')is not None);d=reply(d,text='The target readback returned a mismatch.')
  d=request(drive_state(),'drive_update',{'path':'existing.txt','content':'New'});d=response(d,'Old',200,{'ETag':'"prior"'});d=response(d,'',201);d=response(d,'Old');d=response(d,'',204);d['state']['capability_mode']='local';d=response(d,'New');check('verified revision survives model continuation refusal',d['effect']is None and d['state'].get('drive_revision')is None and any('Network text file updated.'in m.get('content','')for m in messages(d))and not any('"status":"failed"'in m.get('content','')for m in messages(d)))
  d=request(drive_state());original=copy.deepcopy(d);wrong=copy.deepcopy(d);wrong['effect']['drive_ticket']='forged';x=response(d,'FORGED',404,source=wrong);check('incorrect network owner cannot enter model feedback',x['effect']is None and x['state']==d['state']);d['state']['agents'][0]['tools']='read';x=response(d,'REVOKED',404);check('changed agent permissions still reject the old network owner',x['effect']is None and x['state']==d['state']);d=event(original['state'],'cancel');x=response(original,'CANCELLED',404);check('cancelled network failure stays inert even with restored state',x['effect']is None and x['state']==original['state'])
  d=request(drive_state());d['state']['device']={'heap_bytes':2097152,'memory_bytes':4194304,'cores':1};d=response(d,'Missing',404);check('resource refusal retains the actual network error and retires model ownership',d['effect']is None and not d['state']['busy']and not d['state']['agent_running']and not d['state'].get('model_expected')and any('404'in m.get('content','')for m in messages(d)))
  d=request(drive_state());d=response(d,'Missing',404)
  for _ in range(2):d=reply(d,'drive_read',{'path':'missing.txt'});d=event(d['state'],'approve');d=response(d,'Missing',404)
  check('new reviewed network errors share the existing bounded feedback allowance',d['effect']is not None and d['state']['local_failure_followups']==3 and d['state']['steps']==3)
  d=reply(d,'drive_read',{'path':'missing.txt'});d=event(d['state'],'approve');d=response(d,'Missing',404);check('repeated network errors settle at the operation feedback bound',d['effect']is None and not d['state']['busy']and not d['state']['agent_running']and'Three operation error results'in d['state']['notice'])
  d=event(drive_state(),'drive_list');d=response(d,'Forbidden',403);check('manual network errors stay local',d['effect']is None and not d['state']['busy']and'403'in d['state']['notice'])
  st=drive_state();st['routing']={'subagent':'2'};d=event(st,'delegate',{'task':'Read missing.txt from the network folder.'});d=reply(d,'drive_read',{'path':'missing.txt'});d=event(d['state'],'approve');d=response(d,'Missing',404);check('standalone task receives the network failure on its assigned model',d['effect']is not None and d['state']['request_index']==2 and'404'in prompt(d));d=reply(d,text='Standalone network result retained.')
  d=proposal(drive_state(),{'agent':1,'task':'Read missing.txt on the network folder.'});d=event(d['state'],'approve');d=reply(d,'drive_read',{'path':'missing.txt'});d=event(d['state'],'approve');d=response(d,'Missing',404);check('child network failure keeps the captured parent return',d['effect']is not None and d['state']['request_index']==0 and not d['state'].get('delegation_stack')and'404'in prompt(d));d=reply(d,text='Parent received the child network error.')
 finally:p.stdin.close();p.wait(timeout=10)
result={'passed':passed,'scope':'Owner public-event checks: four controlled provider formats, exact held WebDAV callbacks, HTTP/transport/parser failure returns, create/update readback phases, current permissions, cancellation, response admission and captured child returns.'};(PROJECT/'output/checks/network-feedback.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
