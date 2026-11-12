"""Real core phase transitions keep provider search inside operation phases."""
import base64,copy,hashlib,json,pathlib,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
with tempfile.TemporaryDirectory(prefix='hosted-phases-',dir=PROJECT/'output')as directory:
 root=pathlib.Path(directory);(root/'evidence.txt').write_text('Actual supplied file evidence.')
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def op(v):
  p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert 'value'in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',state=st,action=a,inputs=i or {},root=str(root)))
 def state():
  st=event(None,'resume')['state'];st['profiles']=[dict(name='Controlled',endpoint='http://provider.test/anthropic/v1/messages',protocol='messages',model='controlled',plain=True,stream=False,hosted_search='messages_search_v1',tool_format='structured')];st['agents']=[dict(name='General',tools='read,web_search,web_read')];return st
 def body(d):return json.loads(d['effect']['wire'].split('\r\n\r\n',1)[1])
 def searches(d):return any(t.get('name')=='web_search'for t in body(d).get('tools',[]))
 def reply(d,text,stop='end_turn',blocks=None):
  b=json.dumps(dict(role='assistant',content=blocks or [dict(type='text',text=text)],stop_reason=stop)).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 try:
  d=event(state(),'send',{'draft':'Search the web only after I confirm.'})
  check('explicit deferral suppresses the configured provider search declaration and keeps normal model requests',not searches(d) and 'current user request defers web search' in d['effect']['wire'])
  d=reply(d,'I will wait for confirmation.')
  check('ordinary response completes while provider search is deferred',not d['state']['busy'] and d['effect'] is None)
  d=event(state(),'refine',{'task':'Refine instructions from observed evidence.'});check('baseline retains explicitly enabled provider search',searches(d)and d['state']['request_hosted_search'])
  d=reply(d,'Observed baseline.');check('proposal phase uses captured evidence without a provider search declaration',d['state']['task_run']['phase']=='propose'and not searches(d)and not d['state']['request_hosted_search'])
  check('proposal prompt states the actual phase reason','phase or exhausted operation allowance uses captured evidence only'in d['effect']['wire']and 'agent has not permitted'not in d['effect']['wire'])
  check('proposal does not leak a provider-search execution instruction','Use the provider\'s declared web_search server tool'not in d['effect']['wire'])
  d=reply(d,'Candidate instructions based on the observed baseline.');check('probe retains explicitly enabled evidence operations',d['state']['task_run']['phase']=='probe'and searches(d)and d['state']['request_hosted_search'])
  d=reply(d,'Observed probe.');check('composition uses captured evidence without provider search',d['state']['task_run']['phase']=='compose'and not searches(d)and not d['state']['request_hosted_search'])
  d=reply(d,'Composed instructions from actual baseline and probe.');check('refinement still reaches its existing adoption review',not d['state']['busy']and d['state']['dialog']=='refinement_review'and d['state']['refinement_pending']['composition'].startswith('Composed instructions'))
  criteria='REQUIRED-ALPHA\nREQUIRED-BETA'
  task='Produce the requested bounded report.'
  d=event(state(),'refine',{'task':task,'response_checks':criteria})
  check('user checks are captured before the first call and withheld from the model',d['state']['task_run']['response_checks']==criteria.splitlines()and 'REQUIRED-' not in d['effect']['wire'])
  d=reply(d,'Partial report REQUIRED-ALPHA.')
  check('baseline records partial progress without strict completion',d['state']['task_run']['baseline_score']['passed']==1 and not d['state']['task_run']['baseline_score']['strict'] and '1/2 required phrases' in d['effect']['wire'])
  d=reply(d,'Candidate rules for the bounded report.')
  d=reply(d,'Completed report REQUIRED-ALPHA REQUIRED-BETA.')
  check('probe records a strict result independently of the model wording',d['state']['task_run']['probe_score']['strict'] and d['state']['task_run']['probe_score']['passed']==2)
  d=reply(d,'Revision rules for the bounded report.')
  check('exact composed rules run in a new owned replay before review',d['state']['task_run']['phase']=='verify' and d['state'].get('refinement_pending') is None and 'Revision rules for the bounded report.' in d['effect']['wire'] and 'Candidate rules for the bounded report.' not in d['effect']['wire'] and 'REQUIRED-' not in d['effect']['wire'])
  d=reply(d,'Revised report missed the second phrase. REQUIRED-ALPHA.')
  candidate=d['state']['refinement_pending']
  check('failed revision retains the earlier exact passing instructions',candidate['retained_probe'] and not candidate['replay_score']['strict'] and candidate['composition']=='Candidate rules for the bounded report.' and candidate['rejected_revision']=='Revision rules for the bounded report.')
  check('outcome comparison is visible and original remains active before approval','1/2 required phrases' in json.dumps(d['ui']['modal']) and len(d['state']['agents'])==1)
  adopted=event(d['state'],'refinement_adopt')
  check('explicit adoption retains the original and records the measured gain',adopted['state']['agents'][1]['instructions']=='Candidate rules for the bounded report.' and adopted['state']['lessons'][-1]['verified_improvement'])
  again=event(adopted['state'],'refine',{'task':'Another bounded report.'})
  check('a later refinement reuses the adopted measured lesson as historical data',again['state']['task_run']['lesson_context']['items'][0]['provenance'].startswith('user-adopted profile; retained instructions met'))
  event(again['state'],'cancel')
  d=event(state(),'refine',{'task':task,'response_checks':criteria})
  for response in ['Baseline with no required text.','Candidate rules.','Partial REQUIRED-ALPHA.','Revised rules.','<think>REQUIRED-ALPHA REQUIRED-BETA</think>Partial REQUIRED-ALPHA.']:d=reply(d,response)
  check('hidden reasoning cannot satisfy a response check',d['state']['refinement_pending']['replay_score']['passed']==1 and not d['state']['refinement_pending']['replay_score']['strict'])
  adopted=event(d['state'],'refinement_adopt')
  again=event(adopted['state'],'refine',{'task':'Another bounded report.'})
  check('an explicitly saved partial candidate does not enter the next learning context',not adopted['state']['lessons'][-1]['verified_improvement'] and not again['state']['task_run']['lesson_context']['items'])
  event(again['state'],'cancel')
  training='Produce a short training report.'
  evaluation='Separate evaluation question, outside the rewrite.'
  expected='EVALUATION_EXPECTED_SECRET\nexact second line'
  options=dict(task=training,evaluation_task=evaluation,evaluation_method='equals',evaluation_checks=expected)
  task_ui=event(state(),'task')['ui']['modal']
  check('the task modal exposes the separate task, comparison method and local expected output',{'evaluation_task','evaluation_method','evaluation_checks'} <= {f['key'] for f in task_ui['fields']})
  bad=event(state(),'refine',dict(options,evaluation_task=training))
  check('reusing the training task is refused before any request',bad['effect'] is None and bad['state']['notice_error'])
  bad=event(state(),'refine',dict(options,evaluation_method='sha256',evaluation_checks='invalid'))
  check('invalid SHA-256 input is refused before any request',bad['effect'] is None and bad['state']['notice_error'])
  def start_evaluation(options):
   st=state();st['agents'][0]['instructions']='ORIGINAL_RULE_MARKER'
   d=event(st,'refine',options)
   for response in ['Training baseline answer.','Training candidate instructions.','Training probe answer.','RETAINED_RULE_MARKER']:
    assert evaluation not in d['effect']['wire'] and expected.splitlines()[0] not in d['effect']['wire']
    d=reply(d,response)
   return d
  d=start_evaluation(options)
  check('only after the rewrite does the separate task reach the original instructions',d['state']['task_run']['phase']=='evaluation_base' and evaluation in d['effect']['wire'] and 'ORIGINAL_RULE_MARKER' in d['effect']['wire'] and 'RETAINED_RULE_MARKER' not in d['effect']['wire'])
  check('the evaluation omits the verifier, prior task and tool declarations',expected.splitlines()[0] not in d['effect']['wire'] and training not in d['effect']['wire'] and 'Training probe answer.' not in d['effect']['wire'] and 'tools' not in body(d) and not d['effect']['stream'])
  chat_before={c['id']:copy.deepcopy(c['messages']) for c in d['state']['chats']}
  stale=event(d['state'],'failure',dict(purpose='model',model_ticket='stale',error='stale'))
  check('stale callback cannot score or settle the current evaluation',stale['effect'] is None and stale['state']['task_run']['phase']=='evaluation_base' and stale['state']['model_expected']==d['state']['model_expected'])
  d=reply(d,json.dumps({'tool':'hammer','description':'Ordinary structured answer'}))
  check('ordinary JSON can be evaluated without invoking a tool',d['effect']['purpose']=='model' and d['state']['task_run']['phase']=='evaluation_candidate' and not d['state'].get('pending') and not d['state']['task_run']['evaluation_base_score']['strict'])
  check('the retained instructions use the same task with no first answer or expected output',evaluation in d['effect']['wire'] and 'RETAINED_RULE_MARKER' in d['effect']['wire'] and 'ORIGINAL_RULE_MARKER' not in d['effect']['wire'] and 'Ordinary structured answer' not in d['effect']['wire'] and expected.splitlines()[0] not in d['effect']['wire'] and 'tools' not in body(d))
  d=reply(d,expected)
  held=d['state']['refinement_pending']
  check('exact comparison passes with visible matching multiline text',held['evaluation_candidate_score']['strict'])
  check('evaluation does not retain the task, answer or verifier in the candidate',held.get('evaluation') is None and expected.splitlines()[0] not in json.dumps(held) and evaluation not in json.dumps(held))
  check('evaluation does not append answers to ordinary chat',{c['id']:c['messages'] for c in d['state']['chats']}==chat_before)
  check('the native review exposes both comparison outcomes','0/1 exact text checks' in json.dumps(d['ui']['modal']) and '1/1 exact text checks' in json.dumps(d['ui']['modal']))
  adopted=event(d['state'],'refinement_adopt')
  check('separate evaluation gain becomes a lesson only after explicit adoption',adopted['state']['lessons'][-1]['schema']==3 and adopted['state']['lessons'][-1]['verified_improvement'] and len(adopted['state']['agents'])==2)
  for proposal in [json.dumps({'tool':'write','args':{'path':'should-not-exist','content':expected}}),
                   'Tool request follows:\n'+json.dumps({'tool':'web_search','args':{'query':expected}}),
                   json.dumps({'memory_proposal':{'observation':expected}})]:
   d=start_evaluation(dict(options,evaluation_method='phrases',evaluation_checks=expected.splitlines()[0]));d=reply(d,'Original answer.')
   d=reply(d,proposal)
   held=d['state']['refinement_pending']
   check('operation-shaped evaluation output cannot score, execute, or stage review',d['effect'] is None and not d['state'].get('pending') and not held.get('evaluation_candidate_score') and 'proposed operation or report' in held['evaluation_error'] and not (root/'should-not-exist').exists())
   adopted=event(d['state'],'refinement_adopt')
   check('operation-shaped output cannot promote a lesson',not adopted['state']['lessons'][-1]['verified_improvement'])
  hash_value=hashlib.sha256(expected.encode()).hexdigest().upper()
  d=start_evaluation(dict(options,evaluation_method='sha256',evaluation_checks=hash_value))
  d=reply(d,'Original answer.');check('expected digest stays local in both requests',hash_value not in d['effect']['wire'])
  d=reply(d,'<think>'+expected+'</think>Visible answer differs.')
  adopted=event(d['state'],'refinement_adopt')
  check('hidden reasoning cannot satisfy a digest check or promote the lesson',not adopted['state']['lessons'][-1]['verified_improvement'])
  again=event(adopted['state'],'refine',{'task':'Another task.'})
  check('ineligible evaluation does not enter later learning prompts',not again['state']['task_run']['lesson_context']['items'])
  event(again['state'],'cancel')
  d=start_evaluation(options);d=reply(d,'Original answer.')
  d=event(d['state'],'failure',dict(purpose='model',model_ticket=d['state']['model_expected'],error='fixture connection lost',sent=True))
  check('evaluation transport failure preserves a candidate and reports an incomplete evaluation',d['effect'] is None and d['state']['dialog']=='refinement_review' and d['state']['refinement_pending']['composition']=='RETAINED_RULE_MARKER' and 'Evaluation incomplete' in json.dumps(d['ui']['modal']))
  d=start_evaluation(options)
  d=event(d['state'],'cancel')
  check('cancelling evaluation retains the candidate and clears the callback ticket',d['effect']['kind']=='cancel' and d['state']['refinement_pending']['composition']=='RETAINED_RULE_MARKER' and d['state'].get('model_expected') is None)
  d=start_evaluation(options)
  d=event(d['state'],'resume')
  check('resuming after interruption retains the candidate without replay',not d['state'].get('model_expected') and d['state']['dialog']=='refinement_review' and d['state']['refinement_pending']['composition']=='RETAINED_RULE_MARKER' and (not d['effect'] or d['effect']['kind']!='network'))
  d=start_evaluation(options);d['state']['instructions']='Changed global instructions before the second observation.'
  d=reply(d,'Original answer.');d=reply(d,expected)
  adopted=event(d['state'],'refinement_adopt')
  check('changed evaluation context prevents classifying the two outcomes as a gain',not adopted['state']['lessons'][-1]['verified_improvement'])
  structured=dict(options,evaluation_method='json',evaluation_checks='{"secret_expected":true,"items":[1,2]}')
  d=start_evaluation(structured)
  check('the structured expected result stays local','secret_expected' not in d['effect']['wire'])
  d=reply(d,'{"secret_expected":true,"items":[2,1]}')
  d=reply(d,'{ "items": [1, 2], "secret_expected": true }')
  held=d['state']['refinement_pending']
  check('JSON verification ignores object layout and preserves array order',not held['evaluation_base_score']['strict'] and held['evaluation_candidate_score']['strict'] and 'secret_expected' not in json.dumps(held))
  bad=event(state(),'refine',dict(structured,evaluation_checks='{invalid'))
  check('malformed expected JSON fails before a model call',bad['effect'] is None and bad['state']['notice_error'])
  second='Independent question for the second evaluation.'
  expected_two='EVALUATION_SECOND_SECRET'
  pair=dict(options,evaluation_task_2=second,evaluation_method_2='phrases',evaluation_checks_2=expected_two)
  check('the native task modal exposes the second local check group',
        {'evaluation_task_2','evaluation_method_2','evaluation_checks_2'} <= {f['key'] for f in task_ui['fields']})
  duplicate=event(state(),'refine',dict(pair,evaluation_task_2='  '+evaluation.replace(' ','  ')+'  '))
  check('a duplicate held-out task is rejected before the first model request',
        duplicate['effect'] is None and duplicate['state']['notice_error'])
  def pair_start():
   st=state();st['agents'][0]['instructions']='ORIGINAL_RULE_MARKER'
   d=event(st,'refine',pair)
   for response in ['Training baseline answer.','Training candidate instructions.','Training probe answer.','RETAINED_RULE_MARKER']:
    wire=d['effect']['wire']
    assert not any(value in wire for value in [evaluation,second,expected.splitlines()[0],expected_two])
    d=reply(d,response)
   return d
  def pair_second(original_one='Original missed the first check.'):
   d=pair_start()
   assert evaluation in d['effect']['wire'] and second not in d['effect']['wire']
   d=reply(d,original_one)
   assert evaluation in d['effect']['wire'] and second not in d['effect']['wire']
   d=reply(d,expected)
   check('the second task starts with original instructions in its own request',
         d['state']['task_run']['phase']=='evaluation_base' and d['state'].get('refinement_pending') is None
         and second in d['effect']['wire'] and 'ORIGINAL_RULE_MARKER' in d['effect']['wire']
         and not any(value in d['effect']['wire'] for value in [evaluation,expected.splitlines()[0],expected_two,training,original_one]))
   return d
  d=pair_second()
  d=reply(d,expected_two)
  check('second candidate request contains only its own task and retained instructions',
        d['state']['task_run']['phase']=='evaluation_candidate' and second in d['effect']['wire']
        and 'RETAINED_RULE_MARKER' in d['effect']['wire'] and not searches(d)
        and not any(value in d['effect']['wire'] for value in [evaluation,expected.splitlines()[0],expected_two,training]))
  d=reply(d,expected_two)
  held=d['state']['refinement_pending'];modal=json.dumps(d['ui']['modal'])
  check('paired evaluation shows both results and retains only scores',
        'Evaluation task 1' in modal and 'Evaluation task 2' in modal and held['evaluation_first']['retained']['strict']
        and held['evaluation_candidate_score']['strict']
        and not any(value in json.dumps(held) for value in [evaluation,second,expected.splitlines()[0],expected_two]))
  d=event(d['state'],'refinement_adopt')
  check('both checks passing with a held-out improvement becomes reusable after adoption',
        d['state']['lessons'][-1]['verified_improvement'])
  d=pair_second();d=reply(d,expected_two);d=reply(d,'The second check regressed.')
  d=event(d['state'],'refinement_adopt')
  check('a regression on the second task cannot be hidden by a first-task gain',
        not d['state']['lessons'][-1]['verified_improvement'])
  d=pair_second();d=event(d['state'],'cancel')
  check('interrupting the second task preserves candidate and first scores',
        d['state']['refinement_pending']['composition']=='RETAINED_RULE_MARKER'
        and d['state']['refinement_pending']['evaluation_first']['retained']['strict']
        and 'Evaluation incomplete' in json.dumps(d['ui']['modal']))
  d=event(d['state'],'refinement_adopt')
  check('an interrupted second task cannot promote a first-task gain',
        not d['state']['lessons'][-1]['verified_improvement'])
  d=pair_second()
  d['state']['instructions']='New context between the two evaluation tasks.'
  d=reply(d,expected_two);d=reply(d,expected_two)
  d=event(d['state'],'refinement_adopt')
  check('a changed context between evaluation tasks cannot promote a lesson',
        not d['state']['lessons'][-1]['verified_improvement'])
  checks={'/nested/0/a~1b~0c':'TRAIN_PRIVATE_VALUE','/nullable':None}
  held_checks={'/held/0':'HELD_PRIVATE_VALUE','/nullable':None}
  inputs=dict(task=training,response_check_method='json_fields',response_checks=json.dumps(checks),
              refinement_rounds='2',evaluation_task=evaluation,evaluation_method='json_fields',
              evaluation_checks=json.dumps(held_checks))
  check('native form offers JSON field checks in training and held-out tasks',
        {'response_check_method','evaluation_method','evaluation_method_2'} <= {f['key'] for f in task_ui['fields']}
        and json.dumps(task_ui).count('json_fields')==3)
  for invalid in ('{}','{"bad":0}','{"/bad~2escape":0}','{"/repeat":0,"/repeat":1}'):
   d=event(state(),'refine',dict(inputs,response_checks=invalid))
   check('malformed field verifier is rejected before dispatch '+invalid,d['effect'] is None and d['state']['notice_error'])
  private=['/nested/0/a~1b~0c','/nullable','/held/0','HELD_PRIVATE_VALUE']
  d=event(state(),'refine',inputs)
  assert not any(value in d['effect']['wire'] for value in private+['TRAIN_PRIVATE_VALUE',evaluation])
  d=reply(d,'{"nested":[{"a/b~c":"wrong"}]}')
  check('missing null and mismatched nested field score zero',d['state']['task_run']['baseline_score']['passed']==0)
  d=reply(d,'JSON candidate instructions.')
  d=reply(d,'{"nullable":null,"extra":true}')
  check('optional extra fields do not prevent partial deterministic gain',
        d['state']['task_run']['probe_score']['passed']==1 and '1/2 JSON field checks' in d['effect']['wire'])
  assert not any(value in d['effect']['wire'] for value in private+['TRAIN_PRIVATE_VALUE',evaluation])
  d=reply(d,'First JSON rewrite.')
  d=reply(d,'{"nullable":null}')
  check('partial JSON gain enters another rewrite with measured counts',d['state']['task_run']['phase']=='propose'
        and 'First round retained instructions' in d['effect']['wire'])
  d=reply(d,'Second JSON candidate.')
  d=reply(d,'{"nullable":null,"nested":[{"a/b~c":"wrong"}]}')
  d=reply(d,'Second JSON rewrite.')
  d=reply(d,'{"nullable":null,"nested":[{"a/b~c":"TRAIN_PRIVATE_VALUE"}],"extra":true}')
  check('strict checked replay reaches the separate held-out task',
        d['state']['task_run']['phase']=='evaluation_base' and evaluation in d['effect']['wire']
        and not any(value in d['effect']['wire'] for value in private+['TRAIN_PRIVATE_VALUE']))
  d=reply(d,'{"held":["wrong"],"nullable":null}')
  d=reply(d,'{"held":["HELD_PRIVATE_VALUE"],"nullable":null,"extra":1}')
  held=d['state']['refinement_pending']
  check('separate JSON field checks distinguish partial and strict results and clear verifiers',
        held['evaluation_base_score']['passed']==1 and held['evaluation_candidate_score']['strict']
        and held.get('evaluation') is None and held.get('response_checks') is None
        and not any(value in json.dumps(held) for value in private))
  d=event(d['state'],'refinement_adopt')
  check('strict field gains become reusable after adoption',d['state']['lessons'][-1]['verified_improvement'])
  d=event(state(),'refine',dict(inputs,response_checks='{"/held/00":null}',refinement_rounds='1'))
  d=reply(d,'{"held":[null]}')
  check('a leading-zero JSON Pointer array index never matches',d['state']['task_run']['baseline_score']['passed']==0)
  json_repair=dict(task=training,response_check_method='json_fields',response_checks='{"/ok":true}',
                   evaluation_task=evaluation,evaluation_method='json',evaluation_checks='{"result":true}')
  d=event(state(),'refine',json_repair)
  d=reply(d,'{"ok":false}')
  d=reply(d,'Candidate JSON instructions.')
  d=reply(d,'```JSON\r\n{"ok":true}\r\n```')
  check('a single code wrapper is removed before deterministic training checks',
        d['state']['task_run']['probe_score']['strict'] and d['state']['task_run']['probe_score']['json_unwrapped']
        and 'JSON code block unwrapped locally' in d['effect']['wire'])
  d=reply(d,'Retained JSON instructions.')
  d=reply(d,'{"ok":true}')
  d=reply(d,'Introductory text.\n```json\n{"result":true}\n```')
  check('prose surrounding a JSON code block is not silently discarded',
        d['state']['task_run']['evaluation_base_score']['passed']==0)
  d=reply(d,'```json\n{"result":true}\n```')
  held=d['state']['refinement_pending']
  check('the held-out verifier reports the local repair and drops the expected answer',
        held['evaluation_candidate_score']['strict'] and held['evaluation_candidate_score']['json_unwrapped']
        and 'JSON code block unwrapped locally' in json.dumps(d['ui']['modal']) and held['evaluation'] is None)
  d=start_evaluation(dict(options,evaluation_method='json',evaluation_checks='{"tool":"write","args":{"path":"forbidden"}}'))
  d=reply(d,'```json\n{"tool":"write","args":{"path":"forbidden"}}\n```')
  check('a wrapped operation cannot execute or score as a held-out JSON answer',
        d['effect'] is None and 'proposed operation' in d['state']['refinement_pending']['evaluation_error']
        and not (root/'forbidden').exists())
  d=start_evaluation(dict(options,evaluation_method='json',evaluation_checks='{"result":true}'))
  d=reply(d,'```json\n{"result":true}\n```\n```json\n{"result":true}\n```')
  check('multiple JSON code blocks never qualify as a repaired value',
        d['state']['task_run']['evaluation_base_score']['passed']==0)
  strict_bits=dict(task=training,response_check_method='json_fields',refinement_rounds='2',
                   response_checks='{"/first":true,"/second":true,"/third":true}')
  d=event(state(),'refine',strict_bits)
  for result in ['{"first":true}','Initial field instructions.','{"second":true,"third":true}',
                 'Rewritten field instructions.','{"second":true,"third":true}']:
   d=reply(d,result)
  check('a higher count that drops a previously passing field does not seed another round',
        d['state']['dialog']=='refinement_review' and not d['state']['refinement_pending'].get('first_refinement'))
  d=event(state(),'refine',strict_bits)
  for result in ['{}','Initial field instructions.','{"first":true}',
                 'First verified instructions.','{"first":true}',
                 'Second field instructions.','{"first":true}',
                 'Second rewritten instructions.','{"second":true,"third":true}']:
   d=reply(d,result)
  check('a higher second-round count that drops an earlier pass retains the first exact instructions',
        d['state']['refinement_pending']['composition']=='First verified instructions.'
        and d['state']['refinement_pending']['selected_score']['passed']==1)
  rounds=dict(task=training,refinement_rounds='2',response_checks=criteria)
  bad=event(state(),'refine',dict(rounds,response_checks=''))
  check('a second refinement round requires deterministic checks',bad['effect'] is None and bad['state']['notice_error'])
  def first_round(extra=None):
   d=event(state(),'refine',dict(rounds,**(extra or {})))
   for response in ['Original reply.','First probe rules.','First partial answer REQUIRED-ALPHA.','First composed rules.','First replay missed everything.']:
    assert evaluation not in d['effect']['wire'] and expected.splitlines()[0] not in d['effect']['wire']
    d=reply(d,response)
   return d
  d=first_round(options)
  run=d['state']['task_run']
  check('the second proposal starts from the exact better checked attempt',run['phase']=='propose' and run['first_refinement']['instructions']=='First probe rules.' and 'First partial answer' in d['effect']['wire'] and 'First probe rules.' in d['effect']['wire'] and evaluation not in d['effect']['wire'])
  check('the original comparison and operation allowance remain intact',run['baseline']=='Original reply.' and run['baseline_score']['passed']==0 and d['state']['steps']==0)
  for response in ['Second probe rules.','Second partial REQUIRED-ALPHA.','Second composed rules.','Second successful REQUIRED-ALPHA REQUIRED-BETA.']:
   d=reply(d,response)
  check('a strict second round enters only the captured separate evaluation',d['state']['task_run']['phase']=='evaluation_base' and d['state']['task_run']['composition']=='Second composed rules.' and evaluation in d['effect']['wire'] and expected.splitlines()[0] not in d['effect']['wire'] and 'First partial answer' not in d['effect']['wire'])
  d=reply(d,'Original evaluation missed the answer.')
  check('separate evaluation uses the exact second-round instructions','Second composed rules.' in d['effect']['wire'] and 'First probe rules.' not in d['effect']['wire'])
  d=reply(d,expected)
  adopted=event(d['state'],'refinement_adopt')
  check('the strictly improved second round can contribute after adoption',adopted['state']['lessons'][-1]['verified_improvement'] and adopted['state']['agents'][1]['instructions']=='Second composed rules.')
  d=first_round()
  for response in ['Second probe rules.','Second answer missed both.','Second composed rules.','Second replay missed both.']:d=reply(d,response)
  check('a regressing second round retains the earlier exact instructions and score',d['effect'] is None and d['state']['refinement_pending']['composition']=='First probe rules.' and d['state']['refinement_pending']['selected_score']['passed']==1 and 'first round was retained' in json.dumps(d['ui']['modal']))
  adopted=event(d['state'],'refinement_adopt')
  check('a partial retained candidate cannot contribute a verified lesson',not adopted['state']['lessons'][-1]['verified_improvement'])
  for action in ['resume','cancel','failure']:
   d=first_round()
   inputs=dict(purpose='model',model_ticket=d['state']['model_expected'],sent=True,error='Second round interrupted') if action=='failure' else None
   d=event(d['state'],action,inputs)
   check('interrupting the second round preserves the tested first candidate '+action,d['state']['refinement_pending']['composition']=='First probe rules.' and d['state'].get('model_expected') is None)
  d=event(state(),'refine',rounds)
  for response in ['Original reply.','First probe rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.','First composed rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.']:d=reply(d,response)
  check('first-round strict completion avoids unnecessary second requests',d['effect'] is None and d['state']['dialog']=='refinement_review' and not d['state']['refinement_pending'].get('first_refinement'))
  d=event(state(),'refine',rounds)
  for response in ['Original REQUIRED-ALPHA REQUIRED-BETA.','First probe rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.','First composed rules.','Replay kept only REQUIRED-ALPHA.']:d=reply(d,response)
  check('a strict original baseline cannot cause a passing probe to be replaced by a regression',d['effect'] is None and d['state']['refinement_pending']['composition']=='First probe rules.' and d['state']['refinement_pending']['retained_probe'])
  d=first_round();d=reply(d,'Second read-only probe instructions.')
  d=reply(d,json.dumps({'tool':'read','args':{'path':'evidence.txt'}}))
  check('second-round tools still require the existing review',d['effect'] is None and d['state']['dialog']=='approval')
  d=event(d['state'],'reject')
  check('rejecting a proposed operation retains the first tested candidate and its displayed reply',d['state']['refinement_pending']['composition']=='First probe rules.' and 'First partial answer' in json.dumps(d['ui']['modal']) and d['effect'] is None)
  plain=state();plain['profiles'][0].pop('hosted_search')
  d=event(plain,'refine',rounds)
  project=root/'AGENTS.md';project.write_text('Actual changed project instructions after the original request.')
  for response in ['Original reply.','Probe rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.','Composed rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.']:d=reply(d,response)
  held=d['state']['refinement_pending']
  adopted=event(d['state'],'refinement_adopt')
  check('a real change to project instructions prevents a comparable training gain',held['baseline_score']['context']!=held['replay_score']['context'] and not adopted['state']['lessons'][-1]['verified_improvement'])
  project.unlink()
  d=event(plain,'refine',rounds)
  d=event(d['state'],'workspace_permissions')
  permissions=dict(phone=True,network=True,cloud=True,mcp=True)
  permissions.update(d['state'].get('storage_access',{}))
  permissions['phone']=False
  d=event(d['state'],'workspace_access_save',permissions)
  for response in ['Original reply.','Probe rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.','Composed rules.','Complete REQUIRED-ALPHA REQUIRED-BETA.']:d=reply(d,response)
  held=d['state']['refinement_pending']
  adopted=event(d['state'],'refinement_adopt')
  check('changing workspace access through the native permission action invalidates training comparison',held['baseline_score']['context']!=held['replay_score']['context'] and not adopted['state']['lessons'][-1]['verified_improvement'])
  plain=state();plain['profiles'][0].pop('hosted_search')
  d=event(plain,'refine',rounds);d=reply(d,'Original reply.')
  project.write_text('A different project context for the probe only.')
  d=reply(d,'Incomparable probe rules.')
  project.unlink()
  for response in ['Incomparable REQUIRED-ALPHA REQUIRED-BETA.','Comparable replay rules.','Comparable REQUIRED-ALPHA.']:d=reply(d,response)
  check('a comparable partial replay outranks an incomparable strict probe',d['state']['task_run']['first_refinement']['instructions']=='Comparable replay rules.' and d['state']['task_run']['phase']=='propose')
  event(d['state'],'cancel')
  st=state()
  for n in range(3):d=event(st,'send',{'draft':'Controlled ordinary question '+str(n)});assert searches(d);d=reply(d,'Observed ordinary answer '+str(n));st=d['state']
  chat=next(c for c in st['chats']if c['id']==st['active']);original=copy.deepcopy(chat['messages']);d=event(st,'compact');check('actual compaction declares no provider search',d['state']['task_run']['phase']=='compact'and not searches(d)and not d['state']['request_hosted_search'])
  check('compaction keeps intrinsic method and supplied-evidence-only instructions','Investigation is intrinsic'in d['effect']['wire']and 'This phase summarizes supplied evidence'in d['effect']['wire'])
  d=reply(d,'Summary of supplied conversation evidence.');chat=next(c for c in d['state']['chats']if c['id']==d['state']['active']);check('actual compaction preserves original history and its summary',not d['state']['busy']and chat['messages']==original and chat['context_summary']=='Summary of supplied conversation evidence.')
  st=state();st['agents'][0]['tools']='read';d=event(st,'send',{'draft':'Answer without web permission.'});check('missing web permission keeps provider search disabled',not searches(d)and 'selected agent has not permitted web_search'in d['effect']['wire']);d=reply(d,'Answer from supplied context.');check('permission-disabled search still permits an ordinary answer',not d['state']['busy'])
  d=event(state(),'send',{'draft':'Read the actual evidence within this turn.'})
  for n in range(8):
   assert searches(d);d=reply(d,json.dumps({'tool':'read','args':{'path':'evidence.txt'}}));assert d['state']['dialog']=='approval';d=event(d['state'],'approve');e=d['effect'];output=op(dict(e,op='tool',root=str(root)));assert output=='Actual supplied file evidence.';i={k:e[k]for k in ['purpose','tool_ticket','tool_binding','child_ticket','child_owner']if k in e};i['output']=output;d=event(d['state'],'tool_result',i)
  check('eight actual reads suppress provider search at the existing allowance',d['state']['steps']==8 and not searches(d)and not d['state']['request_hosted_search'])
  check('exhausted hosted profile cannot substitute local web tools','tools'not in body(d)and 'Available operations for this agent: .'in d['effect']['wire']and 'Actual supplied file evidence.'in d['effect']['wire'])
  d=reply(d,'Answer from the eight actual observations.');check('exhausted provider-search turn can finish from actual evidence',not d['state']['busy']and d['effect']is None)
  d=event(d['state'],'send',{'draft':'A fresh ordinary question.'});check('fresh user request restores explicitly enabled provider search',d['state']['steps']==0 and searches(d));d=reply(d,'Fresh answer.')
  task=event(state(),'task');check('manual task controls describe standalone execution',any(b.get('text')=='Run agent task' or b.get('label')=='Run agent task'for b in task['ui']['modal']['buttons'])and 'its answer is saved in this conversation'in json.dumps(task['ui']))
  task=event(task['state'],'delegate',{'task':'Answer this standalone task.'});blocks=[{'type':'thinking','thinking':'CONTROLLED_HIDDEN_THINKING','signature':'exact-signature'},{'type':'text','text':'Actual standalone answer.'}];task=reply(task,'',blocks=blocks);c=next(c for c in task['state']['chats']if c['id']==task['state']['active']);last=c['messages'][-1]
  check('standalone task result is not represented as a spawned child',last['content']=='Agent task result:\nActual standalone answer.'and not task['state'].get('delegation_stack')and 'Agent task result saved'in task['state']['notice'])
  check('new result presentation retains exact hosted protocol context',last.get('hosted_content')==blocks and all('CONTROLLED_HIDDEN_THINKING'not in m.get('content','')for m in task['ui'].get('messages',[])))
  task=event(task['state'],'send',{'draft':'Continue from the actual saved task answer.'});check('same-connection continuation retains original hosted blocks',any(m.get('role')=='assistant'and m.get('content')==blocks for m in body(task)['messages']));task=reply(task,'Continued from the actual task.');assert not task['state']['busy']
  d=event(state(),'send',{'draft':'Use current provider search if needed.'});d=reply(d,'Provider paused.','pause_turn');check('eligible provider pause retains explicit continuation',d['state']['dialog']=='hosted_pause'and not d['state']['busy'])
  resumed=event(d['state'],'hosted_continue');check('eligible explicit continuation retains its declared server tool',resumed['effect']is not None and searches(resumed));resumed=reply(resumed,'Actual continued answer.');check('explicit continuation still settles normally',not resumed['state']['busy'])
  d=event(state(),'send',{'draft':'Bounded search turn.'});d=reply(d,'Provider paused.','pause_turn');changed=copy.deepcopy(d['state']);changed['steps']=8;refused=event(changed,'hosted_continue');check('exhausted restored pause cannot re-enable provider search',refused['effect']is None and refused['state']['notice_error']and 'cannot continue provider search'in refused['state']['notice'])
  legacy=state();legacy['profiles'][0]['hosted_search']='minimax_messages_v1';legacy['profiles'][0]['endpoint']='http://proxy.test/custom-search-route';d=event(legacy,'send',{'draft':'Preserve imported hosted settings.'});check('imported search mode migrates while custom server routes remain usable',d['state']['profiles'][0]['hosted_search']=='messages_search_v1'and searches(d)and d['effect']['wire'].startswith('POST /custom-search-route '));d=reply(d,'Preserved hosted pause.','pause_turn');legacy=copy.deepcopy(d['state']);legacy['profiles'][0]['hosted_search']='minimax_messages_v1';legacy['hosted_request']['profile']['hosted_search']='minimax_messages_v1';resumed=event(legacy,'hosted_continue');check('imported paused continuation preserves captured route and server tools',resumed['effect']is not None and searches(resumed)and resumed['effect']['wire'].startswith('POST /custom-search-route '));resumed=reply(resumed,'Actual migrated continuation.');check('migrated hosted continuation completes without replay',not resumed['state']['busy']and resumed['effect']is None)
 finally:p.stdin.close();p.wait(timeout=10)
print(json.dumps({'passed':len(passed),'checks':passed,'scope':'Owner connected phase checks: controlled Messages replies, actual core file work and lifecycle transitions.'},indent=2));(PROJECT/'output/checks/hosted-phases.json').write_text(json.dumps({'passed':passed},indent=2)+'\n')
