"""Connected command handling, settings migration and ordinary evidence flow."""
import base64,copy,json,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[];old=bytes([114,101,97]).decode()
def check(name,value):assert value,name;passed.append(name)
with tempfile.TemporaryDirectory(prefix='commands-',dir=PROJECT/'output') as temp:
 root=pathlib.Path(temp);(root/'evidence.txt').write_text('Actual evidence from a normal command.')
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):
  r=raw(v);assert 'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=str(root)))
 def reply(d,text):
  body=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
  return event(d['state'],'network_result',{'purpose':'model','model_ticket':d['state']['model_expected'],'data':base64.b64encode(wire).decode()})
 try:
  st=event(None,'resume')['state'];st[old]={'endpoint':'http://127.0.0.1:9127/mcp','key':'migration-fixture'};st[old+'_expected']=old+'_server:expired';st['busy']=True;st['agents']=[{'name':'General','tools':'read,'+old,'instructions':'User-authored instructions','investigation_skill':False}];st['investigation_skill']={'enabled':False,'instructions':'Preserve my additional guidance'}
  migrated=event(st,'resume');st=migrated['state'];check('saved connection migrates into protocol settings',st['mcp']['endpoint']=='http://127.0.0.1:9127/mcp' and st['mcp']['key']=='migration-fixture' and old not in st)
  check('saved tool permission migrates without an alias',st['agents'][0]['tools']=='read,mcp')
  check('old in-flight owner cannot resume',st.get('mcp_client_expected') is None and not st['busy'])
  check('optional preferences and user instructions are preserved',st['investigation_preferences']['instructions']=='Preserve my additional guidance' and st['agents'][0]['investigation_preferences'] is False and st['agents'][0]['instructions']=='User-authored instructions')
  settings=event(st,'instruction_settings');buttons=settings['ui']['modal']['buttons'];check('preferences are in agent settings',any(x['event']=='investigation_preferences' for x in buttons))
  skills=event(st,'skills');check('core process is not presented as a selectable skill',all(x['event']!='investigation_preferences' for x in skills['ui']['modal']['buttons']))
  called=event(st,'mcp_client_call');check('remote call UI uses actual protocol naming',called['ui']['modal']['title']=='MCP tool call')
  unknown=event(st,old+'_list');check('removed action is not an active alias',unknown['effect'] is None)
  listed=event(st,'mcp_client_list');check('migrated connection emits an actual protocol initialization',listed['effect']['kind']=='network' and listed['effect']['purpose'].startswith('mcp_client_init:') and 'initialize' in listed['effect']['wire'])
  event(listed['state'],'cancel')
  # No peer, no assigned skill, no optional guidance, and an unavailable journal.
  st=event(None,'resume')['state'];st['profiles']=[dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',protocol='chat',model='controlled',plain=True,stream=False)];st['agents']=[dict(name='General',tools='read',investigation_preferences=False)];st['investigation_preferences']={'enabled':False};st['inquiry_records']={'schema':999,'items':[]}
  d=event(st,'send',{'draft':'Read the evidence file and answer from what it says.'});check('ordinary command runs without a peer or journal',d['effect']['purpose']=='model');wire=d['effect']['wire'];check('intrinsic process is present with additional guidance off','Investigation is intrinsic to following commands' in wire and 'Investigation preferences:' not in wire)
  check('no branded workflow or remote activation is required',not re.search(r'(?i)\b'+old+r'(?:\b|_)',wire) and 'request mcp args' not in wire)
  d=reply(d,json.dumps({'tool':'read','args':{'path':'evidence.txt'}}));check('normal evidence operation uses existing review',d['state']['dialog']=='approval')
  d=event(d['state'],'approve');e=d['effect'];check('normal command dispatches the actual file operation',e['kind']=='tool' and e['tool']=='read')
  output=op(dict(e,op='tool',root=str(root)));check('file operation executes despite unavailable bookkeeping',output=='Actual evidence from a normal command.')
  i={k:e[k] for k in ['purpose','tool_ticket','tool_binding','child_ticket','child_owner'] if k in e};i['output']=output;d=event(d['state'],'tool_result',i);check('actual evidence feeds directly back into command continuation',output in d['effect']['wire'] and 'Investigation is intrinsic' in d['effect']['wire'])
  d=reply(d,'The answer follows the actual file observation.');check('normal command completes through the existing answer path',not d['state']['busy'] and d['effect'] is None)
  # Both provider encodings and text prompts describe the same real availability.
  def enums(value):
   if isinstance(value,dict):
    if isinstance(value.get('properties'),dict) and isinstance(value['properties'].get('tool'),dict) and 'enum'in value['properties']['tool']:yield value['properties']['tool']['enum']
    for item in value.values():yield from enums(item)
   elif isinstance(value,list):
    for item in value:yield from enums(item)
  for fmt in ['chat','local','messages','content']:
   candidate=event(None,'resume')['state'];candidate['profiles']=[dict(name='Controlled',protocol=fmt,endpoint='http://model.test/v1/chat/completions',model='controlled',plain=True,stream=False,tool_format='structured')];candidate['agents']=[dict(name='General',tools='read,mcp,delegate',subagents=[],investigation_preferences=False)]
   outgoing=event(candidate,'send',{'draft':'Understand the evidence.'});body=json.loads(outgoing['effect']['wire'].split('\r\n\r\n',1)[1]);allowed=list(enums(body));check(fmt+' structured catalog excludes absent connections and children',allowed and all(v==['read'] for v in allowed))
   candidate['mcp']={'endpoint':'http://peer.test/mcp','key':'MCP_SECRET_FIXTURE'};outgoing=event(candidate,'send',{'draft':'Use permitted evidence.'});body=json.loads(outgoing['effect']['wire'].split('\r\n\r\n',1)[1]);allowed=list(enums(body));check(fmt+' configured peer is explicitly available without leaking credentials',allowed and all('mcp'in v and 'delegate'not in v for v in allowed) and 'MCP_SECRET_FIXTURE'not in outgoing['effect']['wire'])
   scoped=copy.deepcopy(candidate);scoped['agents'][0]['tools']='read,write,web_read,web_search,mcp,phone_read,phone_create,drive_read,drive_create';scoped['phone_access_mode']='limited';scoped['phone_uri']='content://fixture/tree/selected';scoped['drive']={'url':'http://nas.test/share/','password':'SOURCE_SECRET_FIXTURE'}
   for form in ['structured','json']:
    scoped['profiles'][0]['tool_format']=form;scoped['storage_access']={'phone':False,'network':False,'mcp':False}
    outgoing=event(scoped,'send',{'draft':'Follow the current source access switches.'});wire=outgoing['effect']['wire'];body=json.loads(wire.split('\r\n\r\n',1)[1]);allowed=list(enums(body))
    check(fmt+' '+form+' disabled source access is reflected in the text catalog','Available operations for this agent: read,write,web_read,web_search.'in wire and all(x not in wire for x in ['phone_read(path)','phone_create(path,content)','drive_read(path)','drive_create(path,content)','An MCP connection is configured']))
    check(fmt+' '+form+' structured source catalog agrees with actual access',all(set(v)=={'read','write','web_read','web_search'}for v in allowed)and(bool(allowed)==(form=='structured')))
    check(fmt+' '+form+' source revocation preserves intrinsic process and independent web access','Investigation is intrinsic to following commands'in wire and 'web_search args {query}'in wire and 'SOURCE_SECRET_FIXTURE'not in wire and 'MCP_SECRET_FIXTURE'not in wire)
    event(outgoing['state'],'cancel');scoped['storage_access']={'phone':True,'network':True,'mcp':True};outgoing=event(scoped,'send',{'draft':'Use the re-enabled configured sources.'});wire=outgoing['effect']['wire'];check(fmt+' '+form+' re-enabled sources regain their existing contracts',all(x in wire for x in ['phone_read(path)','phone_create(path,content)','drive_read(path)','drive_create(path,content)','An MCP connection is configured']));event(outgoing['state'],'cancel')
   scoped['profiles'][0]['tool_format']='structured';scoped['agents'][0]['role']='nonwrite';outgoing=event(scoped,'send',{'draft':'Inspect the evidence with this read-only role.'});wire=outgoing['effect']['wire'];body=json.loads(wire.split('\r\n\r\n',1)[1]);allowed=list(enums(body));expected=['read','web_read','web_search','phone_read','drive_read'];check(fmt+' non-writing role offers only executable operation families',allowed and all(set(v)==set(expected)for v in allowed)and'Available operations for this agent: '+','.join(expected)+'.'in wire);event(outgoing['state'],'cancel')
   candidate['agents'][0].update(tools='read,write,mcp,delegate',subagents=[1]);candidate['agents'].append(dict(name='Child',tools='read',model='0'))
   refined=event(candidate,'refine',{'task':'Read evidence and improve the instructions.'});wire=refined['effect']['wire'];body=json.loads(wire.split('\r\n\r\n',1)[1]);allowed=list(enums(body));check(fmt+' refinement structured catalog matches executor restrictions',allowed and all(v==['read'] for v in allowed))
   check(fmt+' refinement text catalog and child guidance match','Available operations for this agent: read.' in wire and 'Callable subagents:' not in wire and 'request mcp args' not in wire)
   candidate['profiles'][0]['tool_format']='json';refined=event(candidate,'refine',{'task':'Read the same evidence.'});assert refined['effect'],(fmt,refined['state'].get('notice'),refined['state'].get('dialog'));wire=refined['effect']['wire'];check(fmt+' text-only refinement has the same real availability','Available operations for this agent: read.' in wire and 'Callable subagents:' not in wire)

  st['profiles'][0]['tool_format']='structured';d=event(st,'send',{'draft':'Read the evidence within the current operation allowance.'})
  for attempt in range(8):
   d=reply(d,json.dumps({'tool':'read','args':{'path':'evidence.txt'}}));assert d['state']['dialog']=='approval';d=event(d['state'],'approve');e=d['effect'];actual=op(dict(e,op='tool',root=str(root)));assert actual==output
   i={k:e[k] for k in ['purpose','tool_ticket','tool_binding','child_ticket','child_owner'] if k in e};i['output']=actual;d=event(d['state'],'tool_result',i)
  check('eight actual reviewed operations reach their existing allowance',d['state']['steps']==8 and d['effect']['purpose']=='model')
  wire=d['effect']['wire'];request=json.loads(wire.split('\r\n\r\n',1)[1]);check('exhausted request offers an evidence-based answer without unusable tools','tools' not in request and 'operation allowance is exhausted' in wire and output in wire)
  refused=reply(d,json.dumps({'tool':'read','args':{'path':'evidence.txt'}}));check('ninth proposal is refused before an impossible review',refused['effect'] is None and refused['state'].get('pending') is None and refused['state']['dialog']!='approval' and refused['state']['notice_error'])
  continued=event(refused['state'],'send',{'draft':'Answer from the returned evidence.'});wire=continued['effect']['wire'];check('a new user instruction restores its ordinary operation allowance',continued['state']['steps']==0 and 'tools' in json.loads(wire.split('\r\n\r\n',1)[1]));continued=reply(continued,'Answer from the actual evidence.');assert not continued['state']['busy']
  failed=raw({'op':'tool','root':str(root),'tool':'read','args':{'path':'missing.txt'}});check('legitimate missing input stays scoped','error' in failed)
  check('unrelated work remains available after scoped failure',op({'op':'tool','root':str(root),'tool':'read','args':{'path':'evidence.txt'}})==output)
 finally:p.stdin.close();p.wait(timeout=10)
print(json.dumps({'passed':len(passed),'checks':passed,'scope':'Owner connected checks: controlled provider responses and actual shared-core command/file operations.'},indent=2));(PROJECT/'output/checks/commands.json').write_text(json.dumps({'passed':passed},indent=2)+'\n')
