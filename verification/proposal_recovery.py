"""Safety checks for explicit review of a single request retained in prose."""
import argparse,base64,copy,json,pathlib,socket,subprocess,tempfile,threading,time
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
PROJECT=pathlib.Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--controller',type=pathlib.Path,default=PROJECT/'output/headless/RLLM');parser.add_argument('--label',default='current');args=parser.parse_args();passed=[];requests=[]
out=PROJECT/'output/checks';out.mkdir(parents=True,exist_ok=True)
def check(name,value):assert value,name;passed.append(name)
class Peer(BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_GET(self):
  requests.append(self.path);body=b'<html><body>Actual separately reviewed page bytes.</body></html>';self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
server=ThreadingHTTPServer(('127.0.0.1',0),Peer);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='proposal-recovery-',dir=PROJECT/'output')as root:
 p=subprocess.Popen([str(args.controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def event(st,action,inputs=None):
  p.stdin.write(json.dumps(dict(op='event',root=root,state=st,action=action,inputs=inputs or{}))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert'value'in r,r;return r['value']
 def configured(fmt):
  st=event(None,'resume')['state'];st['profiles']=[dict(name='Controlled',model='controlled',protocol=fmt,endpoint='http://model.test/model',plain=True,stream=False)];st['agents']=[dict(name='General',tools='read,write,web_search,web_read',investigation_preferences=False)];st['web']=dict(search_without_review=False);return st
 def reply(d,text):
  fmt=d['state']['request_protocol'];v={'chat':{'choices':[{'index':0,'message':{'content':text},'finish_reason':'stop'}]},'local':{'message':{'content':text},'done':True,'done_reason':'stop'},'messages':{'content':[{'type':'text','text':text}],'stop_reason':'end_turn'},'content':{'candidates':[{'index':0,'content':{'parts':[{'text':text}]},'finishReason':'STOP'}]}}[fmt];b=json.dumps(v).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b
  return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 def answer(fmt,text):return reply(event(configured(fmt),'send',dict(draft='Use actual returned evidence.')),text)
 def messages(d):return next(c for c in d['state']['chats']if c['id']==d['state']['active'])['messages']
 def actions(d):return json.dumps(d['ui'].get('modal',{}))
 def request(d):
  e=d['effect'];wire=e['wire'].encode()
  with socket.create_connection((e['host'],e['port']),timeout=5)as peer:
   peer.sendall(wire);parts=[]
   while True:
    block=peer.recv(65536)
    if not block:break
    parts.append(block)
  return event(d['state'],'network_result',dict(purpose=e['purpose'],web_ticket=e['web_ticket'],data=base64.b64encode(b''.join(parts)).decode()))
 try:
  for fmt in ['chat','local','messages','content']:
   proposal=dict(tool='web_search',args=dict(query='Tampa, Florida current weather'));raw='Observation: the retained search has no current weather reading.\n\nProposal for review:\n'+json.dumps(proposal)
   d=answer(fmt,raw);(out/f'proposal-recovery-{args.label}-observation.json').write_text(json.dumps(dict(format=fmt,raw=messages(d)[-1]['content'],display=d['ui']['messages'][-1]['content'],effect=d['effect'],pending=d['state'].get('pending')),indent=2)+'\n')
   check(fmt+' chat keeps prose and raw details stay in history',raw.split('\n\n',1)[0] in d['ui']['messages'][-1]['content'] and json.dumps(proposal) not in d['ui']['messages'][-1]['content'] and raw==messages(d)[-1]['content']);check(fmt+' malformed reply itself runs no operation',d['effect']is None and d['state'].get('pending')is None)
   shown=event(d['state'],'chat_actions');check(fmt+' current reply exposes a direct review control','format_review'in actions(shown));review=event(shown['state'],'format_review');check(fmt+' explicit recovery opens exact normal review without execution',review['effect']is None and review['state']['dialog']=='approval'and review['state']['pending']['args']==proposal['args']and review['state']['agent_pending'])
   check(fmt+' recovered request exposes one-time approval only','approve_session'not in actions(review)and'approve_always'not in actions(review)and'Approve once'in actions(review))
   matching=event(review['state'],'approve_matching_rule');check(fmt+' automatic approval action cannot dispatch recovered work',matching['effect']is None and matching['state']['pending']==review['state']['pending'])
   persistence=event(review['state'],'approve_session');check(fmt+' recovered request cannot create a persistent approval',persistence['effect']is None and persistence['state']['pending']==review['state']['pending']and persistence['state'].get('approval_rules')==review['state'].get('approval_rules'))
   approved=event(review['state'],'approve');check(fmt+' only approval emits the exact search query',approved['effect']and approved['effect']['purpose']=='web_tool'and'Tampa%2C%20Florida%20current%20weather'in approved['effect']['wire']);event(approved['state'],'cancel')
   raw='Observed context retained.\n'+json.dumps(dict(tool='web_read',args=dict(url=f'http://127.0.0.1:{server.server_port}/observed')));d=answer(fmt,raw);before=len(requests);review=event(d['state'],'format_review');check(fmt+' reviewing a recovered page performs no network I/O',review['effect']is None and len(requests)==before);approved=event(review['state'],'approve');done=request(approved);check(fmt+' actual approved page result returns to requesting model',done['effect']and done['effect']['purpose']=='model'and'Actual separately reviewed page bytes.'in done['effect']['wire']and len(requests)==before+1);reply(done,'Actual page evidence received.')
   inspected=event(d['state'],'branch:'+str(len(messages(d))-1));check(fmt+' selected malformed reply shows response details and review','Response details'in actions(inspected)and'format_review'in actions(inspected)and raw in'\n'.join(f.get('value','')for f in inspected['ui']['modal']['fields']))
   for label,text in [('multiple','First:\n'+json.dumps(proposal)+'\nSecond:\n'+json.dumps(proposal)),('fenced','Example:\n```json\n'+json.dumps(proposal)+'\n```'),('quoted','Example: `'+json.dumps(proposal)+'`'),('nested','Configuration: '+json.dumps(dict(example=proposal)))]:
    sample=answer(fmt,text);menu=event(sample['state'],'chat_actions');check(fmt+label+' data or ambiguous requests cannot become recovered review','format_review'not in actions(menu));blocked=event(sample['state'],'format_review');check(fmt+label+' direct action still emits no operation',blocked['effect']is None and blocked['state'].get('pending')is None)
   d=answer(fmt,'Proposal:\n'+json.dumps(proposal));changed=copy.deepcopy(d['state']);changed['profiles'][0]['model']='changed';blocked=event(changed,'format_review');check(fmt+' changed originating connection cannot recover the request',blocked['effect']is None and blocked['state'].get('pending')is None and blocked['state'].get('notice_error'))
   for label,text in [('reasoning_only','<think>'+json.dumps(proposal)+'</think>Ordinary answer.'),('unfinished_reasoning','<think>'+json.dumps(proposal))]:
    d=answer(fmt,text);menu=event(d['state'],'chat_actions');check(fmt+label+' hidden reasoning cannot become a recovered request','format_review'not in actions(menu)and not messages(d)[-1].get('embedded_tool_json'))
   d=answer(fmt,'<think>'+json.dumps(dict(tool='write',args=dict(path='hidden.txt',content='not authorized')))+'</think>Visible proposal:\n'+json.dumps(proposal));recovered=event(d['state'],'format_review');check(fmt+' only visible request is staged after leading reasoning',recovered['effect']is None and recovered['state']['pending']['tool']=='web_search'and not(pathlib.Path(root)/'hidden.txt').exists());event(recovered['state'],'reject')
   st=configured(fmt);st['agents'][0]['tools']='read';d=reply(event(st,'send',dict(draft='Return a response.')),'Proposal:\n'+json.dumps(proposal));blocked=event(d['state'],'format_review');check(fmt+' disabled tool remains unavailable through request recovery',blocked['effect']is None and blocked['state'].get('pending')is None)
   d=answer(fmt,'Proposal:\n'+json.dumps(dict(tool='web_search',args=dict(query='Tampa, Florida current weather',source='alternative'))));d=event(d['state'],'format_review');check(fmt+' alternative source is disclosed by the actual review destination','duckduckgo.com'in d['state']['pending']['destination']);approved=event(d['state'],'approve');check(fmt+' explicit alternative emits the exact query to the other source once',approved['effect']and approved['effect']['host']=='html.duckduckgo.com'and'Tampa%2C%20Florida%20current%20weather'in approved['effect']['wire']and approved['state']['web_expected']['search_chain']['remaining']==[]);event(approved['state'],'cancel')
   exact=dict(tool='web_read',args=dict(url=f'http://127.0.0.1:{server.server_port}/saved-rule'));ordinary=answer(fmt,json.dumps(exact));grant=event(ordinary['state'],'approve_session');check(fmt+' ordinary request still supports an actual saved session approval',grant['effect']and grant['effect']['purpose']=='web_tool');event(grant['state'],'cancel');control=answer(fmt,json.dumps(exact));check(fmt+' saved session approval actually matches the same normal request',control['effect']and control['effect']['purpose']=='web_tool');event(control['state'],'cancel');wrapped=answer(fmt,'Review this request:\n'+json.dumps(exact));review=event(wrapped['state'],'format_review');check(fmt+' matching saved rule cannot automatically dispatch recovered prose',review['effect']is None and review['state']['pending'].get('format_review')is True);blocked=event(review['state'],'approve_matching_rule');check(fmt+' matching-rule action also preserves the current recovered review',blocked['effect']is None and blocked['state']['pending']==review['state']['pending']);done=request(event(review['state'],'approve'));check(fmt+' explicit one-time approval still completes actual HTTP under a saved rule',done['effect']and done['effect']['purpose']=='model');reply(done,'Actual reviewed page received.')
  def search_response(d,status,body,kind='application/json'):
   data=body.encode()
   raw=f'HTTP/1.1 {status} Test\r\nContent-Type: {kind}\r\nContent-Length: {len(data)}\r\n\r\n'.encode()+data
   return event(d['state'],'network_result',dict(purpose='web_tool',web_ticket=d['effect']['web_ticket'],data=base64.b64encode(raw).decode()))
  # Exercise turn authorization with its own process-wide search queue. The
  # format-review cases above intentionally reserve requests without sending them.
  p.stdin.close();p.wait(timeout=10)
  p=subprocess.Popen([str(args.controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  default=configured('chat');default.pop('web')
  shopping="I would like to know if you can find any cmp 170hx that have damaged pcb's for sale"
  item=dict(tool='web_search',args=dict(query='CMP 170HX damaged PCB for sale',source='alternative'))
  prose="I'll search the web for CMP 170HX devices with damaged PCBs for sale. Let me start by looking for this product."
  request_started=reply(event(default,'send',dict(draft=shopping)),prose+'\n'+json.dumps(item))
  check('the reported shopping wording with wrapped model JSON runs directly on the default setting',request_started['effect'] and request_started['effect']['purpose']=='web_tool' and 'CMP%20170HX%20damaged%20PCB%20for%20sale' in request_started['effect']['wire'] and not request_started['state'].get('pending') and 'Search request recovered' in request_started['ui']['messages'][-1]['content'])
  request_fallback=search_response(request_started,202,'<html>Challenge</html>','text/html')
  check('the default policy also covers a same-query fallback without review',request_fallback['effect']['host']=='api.mwmbl.org' and 'CMP%20170HX%20damaged%20PCB%20for%20sale' in request_fallback['effect']['wire'] and not request_fallback['state'].get('pending'))
  listings=json.dumps(dict(query=item['args']['query'],number_of_results=1,results=[dict(url='https://example.test/listing',title='Controlled listing evidence')]))
  continuation=search_response(request_fallback,200,listings)
  check('the recovered search returns parsed evidence to the original model','Controlled listing evidence' in continuation['effect']['wire'])
  followup=reply(continuation,json.dumps(dict(tool='web_search',args=dict(query='CMP 170HX parts only',source='alternative'))))
  check('the model can run a further search on its own without review',followup['effect']['purpose']=='web_tool' and not followup['state'].get('pending'))
  event(followup['state'],'cancel')
  for fmt in ['local','messages','content']:
   blank=configured(fmt);blank.pop('web')
   recovered=reply(event(blank,'send',dict(draft=shopping)),prose+'\n'+json.dumps(item))
   check(fmt+' response with a single unambiguous wrapped search runs by default',recovered['effect']['purpose']=='web_tool');event(recovered['state'],'cancel')
  for label in ['Example:\n','Example:\n- ','For illustration only:\n','Here is how the search payload should look:\n','Documentation payload follows:\n','Do not execute the example search below:\n','This is only an example:\n','Here is sample JSON:\n','This is a sample search request:\n','I will not search.\n','I am not going to search.\n','I will show a sample search request.\n','Looking at this example is enough.\n']:
   example=reply(event(default,'send',dict(draft='Explain a JSON example without running it.')),label+json.dumps(item))
   check('an explicitly labeled example cannot trigger automatic search or correction: '+label.strip(),example['effect'] is None and not example['state'].get('pending'))
  unclear=reply(event(default,'send',dict(draft=shopping)),'Current information would help.\n'+json.dumps(item))
  check('an unclear wrapped proposal asks the model for one correction without sending a search',unclear['effect']['purpose']=='model' and 'Proposal format review' in unclear['effect']['wire'] and not unclear['state'].get('pending'))
  recovered=reply(unclear,json.dumps(item))
  check('a clean corrected search still runs without approval',recovered['effect']['purpose']=='web_tool');event(recovered['state'],'cancel')
  custom=configured('chat');custom['web']=dict(search_url=f'http://127.0.0.1:{server.server_port}/search?q={{query}}')
  custom_reply=reply(event(custom,'send',dict(draft=shopping)),prose+'\n'+json.dumps(dict(tool='web_search',args=dict(query='controlled custom search'))))
  check('automatic search sends only to the saved custom endpoint',custom_reply['effect']['port']==server.server_port and 'GET /search?q=controlled%20custom%20search' in custom_reply['effect']['wire'] and not custom_reply['state'].get('pending'))
  custom_done=request(custom_reply)
  check('actual custom response returns to the requesting model',custom_done['effect']['purpose']=='model' and 'Actual separately reviewed page bytes.' in custom_done['effect']['wire']);reply(custom_done,'Received actual custom evidence.')
  p.stdin.close();p.wait(timeout=10)
  p=subprocess.Popen([str(args.controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  off=configured('chat')
  off['web']=dict(search_without_review=False)
  prompt='Please search the internet for current public information.'
  query='bounded current information'
  proposal=dict(tool='web_search',args=dict(query=query,source='alternative'))
  search=reply(event(off,'send',dict(draft=prompt)),json.dumps(proposal))
  check('explicit user request authorizes built-in alternative search with the setting off',search['effect']['host']=='html.duckduckgo.com' and search['state'].get('pending') is None)
  check('turn authorization adds no saved approval rule',search['state'].get('approval_rules')==off.get('approval_rules'))
  fallback=search_response(search,202,'<html>source did not return results</html>','text/html')
  check('HTTP 202 automatically falls back to the independent primary without review',fallback['effect']['host']=='api.mwmbl.org' and fallback['state'].get('pending') is None and 'bounded%20current%20information' in fallback['effect']['wire'])
  body=json.dumps(dict(query=query,number_of_results=1,results=[dict(url='https://example.test/public',title='Current source evidence',content='Verified result fixture',engine='test')]))
  completed=search_response(fallback,200,body)
  check('the actual parsed fallback result returns to the requesting model',completed['effect']['purpose']=='model' and 'Current source evidence' in completed['effect']['wire'])
  revised=dict(tool='web_search',args=dict(query='a second focused query'))
  next_search=reply(completed,json.dumps(revised))
  check('a second search in the same turn dispatches without another approval',next_search['effect']['purpose']=='web_tool' and next_search['effect']['host']=='api.mwmbl.org' and next_search['state'].get('pending') is None)
  check('rapid repeated requests queue the primary source within the rate bound',0<next_search['effect'].get('search_delay_ms',0)<=4000)
  fallback=search_response(next_search,503,'temporary primary failure','text/plain')
  failed=search_response(fallback,202,'<html>no results</html>','text/html')
  check('both-source failure resumes the same model and hides raw status in ordinary chat',failed['effect']['purpose']=='model' and 'HTTP 202' in failed['effect']['wire'] and 'Tap for details.' in failed['ui']['messages'][-1]['content'] and 'failure_kind' not in failed['ui']['messages'][-1]['content'])
  details=event(failed['state'],'branch:'+str(len(messages(failed))-1))
  check('failure details retain the exact source outcome for inspection','HTTP 202' in actions(details))
  answer_done=reply(failed,'The search did not return a verified answer.')
  check('the final reply completes after a scoped search failure',not answer_done['state']['busy'] and 'verified answer' in answer_done['ui']['messages'][-1]['content'])
  neutral=reply(event(answer_done['state'],'send',dict(draft='Summarize the notes in this conversation.')),json.dumps(proposal))
  check('a fresh unrelated message does not inherit turn authorization',neutral['effect'] is None and neutral['state']['dialog']=='approval')
  event(neutral['state'],'reject')
  wrapped='I will look for sources.\n'+json.dumps(proposal)
  recovered=reply(event(off,'send',dict(draft='Could you look up current information online?')),wrapped)
  check('one unambiguous wrapped search in an authorized turn runs without a correction or approval round trip',recovered['effect']['purpose']=='web_tool' and 'Search request recovered' in recovered['ui']['messages'][-1]['content'])
  event(recovered['state'],'cancel')
  no_prompt=reply(event(off,'send',dict(draft="Search the web for current public information, and don't ask me again.")),json.dumps(proposal))
  check('a request explicitly declining repeated prompts remains authorized',no_prompt['effect']['purpose']=='web_tool')
  event(no_prompt['state'],'cancel')
  for draft in ['Search the web.', 'Please search the internet.', 'Could you search the web now?']:
   directive=reply(event(off,'send',dict(draft=draft)),json.dumps(proposal))
   check('an explicit follow-up authorizes search without repeating the topic: '+draft,directive['effect']['purpose']=='web_tool' and directive['state'].get('pending') is None)
   event(directive['state'],'cancel')
  forged=dict(proposal,manual=True)
  refused=reply(event(off,'send',dict(draft='Explain this excerpt.')),json.dumps(forged))
  check('model output cannot claim the manual Search button authority',refused['effect'] is None and refused['state']['dialog']=='approval')
  event(refused['state'],'reject')
  for draft in ['Please search the local project files for a note about the internet.','Do not search the web.','"Search the internet" is an example prompt.','“Search the web”','("Search the web")','Please “search the web”','Search the web is the command label in this example.','Can you search the web?','Search the web for transport standards, but do not make any network requests.','Search the web for transport standards only after I confirm.','Search the web for transport standards, but ask me first.','Search the web if I ask for it later.','Search the web for transport standards, but keep this offline.',"Search the web for transport standards, but don't make any network requests."]:
   guarded=reply(event(off,'send',dict(draft=draft)),json.dumps(proposal))
   check('non-authorizing text keeps the review: '+draft,guarded['effect'] is None and guarded['state']['dialog']=='approval')
   event(guarded['state'],'reject')
  configured_search=configured('chat')
  setting=event(configured_search,'web_settings')
  setting=event(setting['state'],'web_save',dict(search_without_review=True))
  standing=setting['state']
  check('built-in standing approval survives state restoration',event(standing,'resume')['state']['web']['search_without_review'] is True)
  base_proposal=dict(tool='web_search',args=dict(query='bounded search check',source='alternative'))
  wrapped='I will search for evidence.\n'+json.dumps(base_proposal)
  deferred_prompts=['Do not search the web.', 'Search the web only after I confirm.', 'Please check online, but ask me first.', 'Keep this offline.', 'Search the web later.', 'Hold off on web search.', 'Please delay web search.', 'No internet access.', "Don't go online.", '- Do not search the web.', '1. Do not search the web.', 'Search the web when I confirm.', 'Search the web once I confirm.', 'Ask me before searching the web.', 'Wait for my confirmation before searching the web.']
  deferred_prompts.append("Summarize the developers' notes. Do not search the web.")
  for draft in deferred_prompts:
   held=reply(event(standing,'send',dict(draft=draft)),json.dumps(base_proposal))
   check('an explicit deferral overrides automatic approval: '+draft,held['effect'] is None and held['state']['dialog']=='approval')
   repeated=event(held['state'],'approve_matching_rule')
   check('matching-rule action cannot bypass an explicit deferral: '+draft,repeated['effect'] is None and repeated['state']['pending']==held['state']['pending'])
   event(repeated['state'],'reject')
  for draft in ['Search the web for when I need to renew my passport.', 'Search the web for how long to wait for the train.', 'Search the web for no-network deployment patterns.']:
   allowed=reply(event(off,'send',dict(draft=draft)),json.dumps(base_proposal))
   check('topic wording still authorizes a direct search: '+draft,allowed['effect']['purpose']=='web_tool')
   event(allowed['state'],'cancel')
  quoted=reply(event(standing,'send',dict(draft='Explain the sentence “Do not search the web.”')),json.dumps(base_proposal))
  check('quoted example does not change the standing setting',quoted['effect']['purpose']=='web_tool');event(quoted['state'],'cancel')
  held=reply(event(standing,'send',dict(draft=deferred_prompts[1])),wrapped)
  check('explicit deferral also suppresses the automatic web format correction',held['effect'] is None and 'format_review' in actions(event(held['state'],'chat_actions')))
  scope=configured('chat');scope['profiles'][0]['name']='Saved approval control'
  rule=reply(event(scope,'send',dict(draft='Answer the ordinary question.')),json.dumps(base_proposal))
  rule=event(rule['state'],'approve_session');assert rule['effect']['purpose']=='web_tool'
  scope=event(rule['state'],'cancel')['state']
  allowed=reply(event(scope,'send',dict(draft='Answer another ordinary question.')),json.dumps(base_proposal))
  check('control dispatches under the matching saved session rule',allowed['effect']['purpose']=='web_tool')
  scope=event(allowed['state'],'cancel')['state']
  held=reply(event(scope,'send',dict(draft=deferred_prompts[0])),json.dumps(base_proposal))
  check('explicit deferral also overrides a matching saved session rule',held['effect'] is None and held['state']['dialog']=='approval')
  authorized=event(held['state'],'approve');check('later explicit approval still authorizes the pending search',authorized['effect']['purpose']=='web_tool');event(authorized['state'],'cancel')
  draft=event(standing,'send',dict(draft='Search the web.'))
  preview=event(draft['state'],'model_progress',dict(model_ticket=draft['state']['model_expected'],stream_text=wrapped[:-5]))
  check('a partial streamed envelope remains hidden while prose is visible','I will search' in preview['ui']['messages'][-1]['content'] and '"tool"' not in preview['ui']['messages'][-1]['content'])
  stopped=event(preview['state'],'cancel')
  check('cancelled stream keeps raw history but hides the request envelope','"tool"' not in stopped['ui']['messages'][-1]['content'] and messages(stopped)[-1]['content']==wrapped[:-5])
  ambiguous=wrapped+'\n'+json.dumps(dict(tool='web_search',args=dict(query='a second proposal')))
  correction=reply(event(standing,'send',dict(draft='Investigate the returned evidence.')),ambiguous)
  check('one malformed web proposal gets a model correction only',correction['effect']['purpose']=='model' and not correction['state'].get('pending'))
  repeated=reply(correction,ambiguous)
  check('a second ambiguous reply stays in history and stops automatic correction',repeated['effect'] is None and messages(repeated)[-1]['content']==ambiguous and 'format_correction' not in actions(event(repeated['state'],'chat_actions')))
  correction=reply(event(standing,'send',dict(draft='Investigate the returned evidence.')),ambiguous)
  approved=reply(correction,json.dumps(base_proposal))
  check('corrected valid built-in search uses the saved approval',approved['effect']['purpose']=='web_tool' and approved['effect']['host']=='html.duckduckgo.com')
  check('valid bare proposal stays in history and its envelope is hidden','"tool"' not in approved['ui']['messages'][-1]['content'] and json.loads(messages(approved)[-1]['content'])==base_proposal)
  event(approved['state'],'cancel')
  d=event(None,'resume');d=event(d['state'],'web_search');check('manual search exposes the same generic alternative-source choice','Alternative source only'in actions(d));d=event(d['state'],'web_search_review',dict(query='Tampa current weather',source='alternative'));check('manual search submission authorizes the exact query and source immediately',d['effect'] and d['effect']['host']=='html.duckduckgo.com' and 'Tampa%20current%20weather' in d['effect']['wire']);check('manual alternate-only choice keeps its one-source scope',d['state']['web_expected']['search_chain']['remaining']==[]);event(d['state'],'cancel')
  learned=event(None,'resume')['state']
  html='<div class="result"><a class="result__a" href="https://example.test/evidence">Structured result</a></div>'
  def manual(st,query):
   d=event(st,'web_search')
   return event(d['state'],'web_search_review',dict(query=query))
  for n in range(3):
   time.sleep(1.05)
   query=f'Exact comparison {n}'
   first=manual(learned,query)
   assert first['effect'],first['state']['notice']
   check('default order remains stable below the evidence margin',first['effect']['host']=='api.mwmbl.org')
   empty=json.dumps(dict(query=query,number_of_results=0,results=[]))
   second=search_response(first,200,empty)
   check('a valid empty first source does not earn a comparison vote',second['effect']['host']=='html.duckduckgo.com' and second['state'].get('search_preference',0)==n)
   returned=search_response(second,200,html,'text/html')
   learned=returned['state']
   check('a completed fallback votes once only after validated results',learned['search_preference']==n+1 and 'Structured result' in learned['output'])
   late=search_response(dict(second,state=learned),200,html,'text/html')
   check('the consumed callback cannot train twice',late['state']['search_preference']==n+1)
  learned=event(learned,'resume')['state']
  ordering=manual(learned,'Future search after reload')
  check('learned order changes an ordinary search after state restoration',ordering['effect']['host']=='html.duckduckgo.com' and ordering['state']['web_expected']['search_chain']['remaining']==[0])
  ordering=event(ordering['state'],'cancel')
  check('cancelled search has no effect on learned order',ordering['state']['search_preference']==3)
  settings=event(ordering['state'],'web_settings')
  check('settings expose the current default and a working opt-out','automatic order starts with DuckDuckGo' in actions(settings))
  disabled=event(settings['state'],'web_save',dict(search_learning=False))
  time.sleep(1.05)
  disabled_search=manual(disabled['state'],'Learning paused')
  check('opt-out restores the original order without erasing the saved observations',disabled_search['effect']['host']=='api.mwmbl.org' and disabled_search['state']['search_preference']==3)
  event(disabled_search['state'],'cancel')
  saved=event(settings['state'],'web_save',dict(search_learning=True,search_url='https://api.mwmbl.org/api/v2/search/?q={query}'))
  time.sleep(1.05)
  pinned=manual(saved['state'],'Pinned source')
  check('explicitly configured order outranks the learned default',pinned['effect']['host']=='api.mwmbl.org' and not pinned['state']['web_expected']['search_chain']['adaptive'])
  event(pinned['state'],'cancel')
  query='Comparison after learning'
  first=manual(learned,query)
  time.sleep(1.05)
  fallback=event(first['state'],'failure',dict(purpose='web_tool',web_ticket=first['effect']['web_ticket'],error='Local transport unavailable',sent=False))
  record=json.dumps(dict(query=query,number_of_results=1,results=[dict(url='https://example.test/data',title='Validated primary result')]))
  transport_result=search_response(fallback,200,record)
  check('local transport failure cannot train a source preference',transport_result['state']['search_preference']==3 and 'Validated primary result' in transport_result['state']['output'])
  first=manual(transport_result['state'],query)
  time.sleep(1.05)
  fallback=search_response(first,202,'<html>no completed result</html>','text/html')
  learned_again=search_response(fallback,200,record)
  check('a measured result in the opposite direction corrects the learned order',learned_again['state']['search_preference']==2 and 'Validated primary result' in learned_again['state']['output'])
  targeted=event(standing,'harness_rules')
  targeted=event(targeted['state'],'harness_item_new')
  targeted=event(targeted['state'],'harness_item_save',dict(scope='tool',tool='web_search',decision='ask',target='https://html.duckduckgo.com/html/',text='Review this search destination',enabled=True))
  pending=reply(event(targeted['state'],'send',dict(draft='Search the web.')),json.dumps(dict(tool='web_search',args=dict(query='bounded default source check'))))
  check('an explicit fallback-destination review outranks standing permission',pending['effect'] is None and pending['state']['dialog']=='approval' and 'Review this search destination' in actions(pending))
  event(pending['state'],'reject')
  st=configured('chat');st['web']=dict(search_url='http://custom.test/search?q={query}',key='PRIVATE_CUSTOM_KEY',header='X-Key');d=answer('chat','Ordinary reply.');d=reply(event(st,'send',dict(draft='Use search.')),'Proposal:\n'+json.dumps(dict(tool='web_search',args=dict(query='Tampa',source='alternative'))));refused=event(d['state'],'format_review');check('custom search alternative refuses before any endpoint or credential use',refused['effect']is None and refused['state'].get('pending')is None and refused['state']['web']['key']=='PRIVATE_CUSTOM_KEY')
 finally:p.stdin.close();p.wait(timeout=10);server.shutdown();server.server_close()
record=dict(passed=len(passed),checks=passed,scope='Owner public-event checks across four formats; explicit review, current ownership, examples/ambiguity refusal, actual local HTTP and controlled model continuation. Physical Android and actual user server not exercised.');(out/'proposal-recovery.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2))
