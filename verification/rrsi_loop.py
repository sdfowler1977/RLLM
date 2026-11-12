"""Connected self-improvement loop through the product binary: workspace, journal, critic, resume, tamper, and non-refine."""
import base64,hashlib,json,os,pathlib,subprocess,tempfile,traceback,zlib
PROJECT=pathlib.Path(__file__).resolve().parents[1]
passed=[]
problems=[]
evaluated=[]
def check(name,value,detail=''):
 evaluated.append(name)
 if value:
  passed.append(name)
  return
 problems.append(name)
 print('FAIL '+name,flush=True)
 if detail:print(str(detail)[:2000],flush=True)
TASK='Write the bounded note.'
CHECKS='REQUIRED-ALPHA\nREQUIRED-BETA'
CONTROL='CONTROL-RULES'
REJECTED='REJECTED-REVISION'
REFUSAL='refused because component prompt with that hypothesis was already rejected in round 1'
PROFILE=dict(name='Controlled',endpoint='http://model.test/v1/chat/completions',model='controlled',protocol='chat',stream=False,plain=True,tool_format='structured')
AGENT=dict(name='General',tools='read',instructions='INCUMBENT RULES')
def canonical(value):
 return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False)
def row_seal(row):
 body={key:row[key] for key in row if key!='revision'}
 digest=hashlib.sha256(canonical(body).encode()).hexdigest()
 revision=row.get('revision')
 return isinstance(revision,str) and revision==digest and isinstance(row.get('run_digest'),str) and len(row['run_digest'])==64
def read_rows(path):
 if not path.is_file():return []
 rows=[]
 for line in path.read_text().splitlines():
  if line:rows.append(json.loads(line))
 return rows
def sha256(path):
 return hashlib.sha256(path.read_bytes()).hexdigest()
def git_store(git,kind,payload):
 raw=f'{kind} {len(payload)}\0'.encode()+payload
 digest=hashlib.sha1(raw).hexdigest()
 path=git/'objects'/digest[:2]/digest[2:]
 path.parent.mkdir(parents=True,exist_ok=True)
 path.write_bytes(zlib.compress(raw))
 return digest
def plant_git(workspace):
 git=workspace/'.git'
 blob=git_store(git,'blob',b'seed\n')
 tree=git_store(git,'tree',b'100644 seed.txt\0'+bytes.fromhex(blob))
 body=(f'tree {tree}\nauthor RLLM <rllm@localhost> 1000000000 +0000\ncommitter RLLM <rllm@localhost> 1000000000 +0000\n\nseed\n').encode()
 commit=git_store(git,'commit',body)
 (git/'refs/heads').mkdir(parents=True,exist_ok=True)
 (git/'HEAD').write_text('ref: refs/heads/main\n')
 (git/'refs/heads/main').write_text(commit+'\n')
 (git/'config').write_text('[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n')
 (workspace/'seed.txt').write_text('seed\n')
 return commit
def commit_text(workspace,sha):
 path=workspace/'.git'/'objects'/sha[:2]/sha[2:]
 if not path.is_file():return ''
 raw=zlib.decompress(path.read_bytes())
 kind,rest=raw.split(b'\0',1)
 if kind.split(b' ',1)[0]!=b'commit':return ''
 return rest.decode()
def commit_hex(value):
 return isinstance(value,str) and len(value) in (40,64) and all(c in '0123456789abcdef' for c in value)
def incumbent_tip(workspace):
 ref=workspace/'.git'/'refs/heads'/'rllm-evolve'
 if ref.is_file():
  tip=ref.read_text().strip()
  if commit_hex(tip):return tip
 main=workspace/'.git'/'refs/heads'/'main'
 return main.read_text().strip() if main.is_file() else ''
def leg(name,body):
 print('leg '+name,flush=True)
 try:body()
 except Exception:
  traceback.print_exc()
  if name not in evaluated:check(name,False,traceback.format_exc()[-2000:])
with tempfile.TemporaryDirectory(prefix='rrsi-loop-',dir=PROJECT/'output') as tmp:
 room=pathlib.Path(tmp)
 data_home=room/'xdg'
 data_home.mkdir()
 default_root=data_home/'rllm'
 env=dict(os.environ)
 env['XDG_DATA_HOME']=str(data_home)
 env.pop('RLLM_SELF_JOB',None)
 env.pop('RLLM_SELF_INSTRUCTION',None)
 product=None
 error_file=None
 def close_product():
  global product,error_file
  if product is not None:
   try:product.stdin.close()
   except Exception:pass
   try:product.wait(timeout=10)
   except Exception:
    product.kill()
    product.wait(timeout=5)
   product=None
  if error_file is not None:
   error_file.close()
   error_file=None
 def open_product():
  global product,error_file
  close_product()
  error_file=open(room/'product.err','w')
  product=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=error_file,text=True,env=env)
 def raw(value):
  product.stdin.write(json.dumps(value)+'\n')
  product.stdin.flush()
  line=product.stdout.readline()
  if not line:
   tail=(room/'product.err').read_text()[-2000:]
   raise RuntimeError('product closed stdout '+tail)
  return json.loads(line)
 def op(value):
  reply=raw(value)
  if 'value' not in reply:raise AssertionError(reply.get('error') or reply)
  return reply['value']
 def event(state,action,inputs=None):
  return op(dict(op='event',state=state,action=action,inputs=inputs or {},root=str(room)))
 def reply(pending,text):
  body=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode()
  wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
  return event(pending['state'],'network_result',dict(purpose='model',model_ticket=pending['state']['model_expected'],data=base64.b64encode(wire).decode()))
 def fresh():
  state=event(None,'resume')['state']
  state['profiles']=[dict(PROFILE)]
  state['agents']=[dict(AGENT)]
  return state
 def run_of(pending):
  return pending['state'].get('task_run') or pending['state'].get('refinement_pending') or {}
 def outcome_text(pending):
  run=run_of(pending)
  return 'phase='+str(run.get('phase'))+' notice='+str(pending['state'].get('notice') or '')+' decision='+str(run.get('round_note') or '')
 def files_here():
  if not default_root.is_dir():return []
  return sorted(path.name for path in default_root.iterdir() if path.is_file())
 held={}
 def source_digest_of(workspace):
  # The refine arm records rrsi::source_digest when approval_root is non-empty.
  # A missing rllm-harness.txt is the digest "absent", not a missing field.
  path=pathlib.Path(workspace)/'rllm-harness.txt'
  if path.is_symlink() or not path.is_file():return 'absent'
  return hashlib.sha256(path.read_bytes()).hexdigest()
 def workspace_leg():
  opened=event(fresh(),'refine',{'task':TASK})
  run=opened['state'].get('task_run') or {}
  approval=opened['state'].get('approval_root')
  workspace=run.get('workspace')
  source=run.get('source_digest')
  expected=source_digest_of(approval) if isinstance(approval,str) and approval else ''
  held['after_entry']=opened
  check('refine entry copies approval_root onto task_run.workspace and records source_digest', isinstance(approval,str) and approval==str(room.resolve()) and workspace==approval and source==expected and expected!='', 'approval_root='+str(approval)+' workspace='+str(workspace)+' source_digest='+str(source)+' expected='+str(expected)+' canonical='+str(room.resolve())+' keys='+str(sorted(run)))
 def non_refine_leg():
  event(held['after_entry']['state'],'cancel')
  before=files_here()
  delegated=event(fresh(),'delegate',{'task':'Read the notes.'})
  delegate_run=delegated['state'].get('task_run') or {}
  delegate_workspace=delegate_run.get('workspace')
  pooled=reply(delegated,'candidates: 1\nrewritten harness')
  delegate_note=pooled['state'].get('notice') or ''
  delegate_left=pooled['state'].get('task_run')
  state=pooled['state']
  chat=next(item for item in state['chats'] if item['id']==state['active'])
  while len(chat['messages'])<5:
   sent=event(state,'send',{'draft':'Extra line '+str(len(chat['messages']))})
   state=reply(sent,'Extra answer '+str(len(chat['messages'])))['state']
   chat=next(item for item in state['chats'] if item['id']==state['active'])
  compacted=event(state,'compact')
  compact_run=compacted['state'].get('task_run') or {}
  compact_workspace=compact_run.get('workspace')
  done=reply(compacted,'candidates: 2\nrewritten harness\n---\nanother rewrite')
  summary=next(item for item in done['state']['chats'] if item['id']==done['state']['active']).get('context_summary')
  after=files_here()
  grew=[name for name in after if name not in before]
  pool_text=''
  for name in after:
   pool_text+= (default_root/name).read_text()
  worktrees=(room/'.rllm-worktrees').exists() or (default_root/'.rllm-worktrees').exists()
  check('a non-refine run carries no workspace and does not attempt the pool', delegated['state'].get('approval_root')==str(room.resolve()) and delegate_run.get('mode')=='delegate' and not delegate_workspace and delegate_left is None and delegate_note=='Agent task result saved' and compact_run.get('mode')=='compact' and compact_workspace in (None,'') and done['state'].get('task_run') is None and summary=='candidates: 2\nrewritten harness\n---\nanother rewrite' and not grew and 'pool_status' not in pool_text and not worktrees, 'delegate_workspace='+str(delegate_workspace)+' delegate_note='+delegate_note +' compact_workspace='+str(compact_workspace)+' summary='+str(summary) +' grew='+str(grew)+' worktrees='+str(worktrees))
 def critic_leg():
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS,'refinement_rounds':'2'})
  pending=reply(pending,'Original reply.')
  control=reply(pending,CONTROL)
  control_text=outcome_text(control)
  pending=reply(control,'First partial answer REQUIRED-ALPHA.')
  pending=reply(pending,REJECTED)
  pending=reply(pending,'First replay missed everything.')
  treated=reply(pending,REJECTED)
  treated_text=outcome_text(treated)
  held['refusal']=treated
  held['critic_files']=files_here()
  check('a second round decision changes when the ledger already rejected that component', control['state']['task_run']['phase']=='probe' and REFUSAL not in control_text and REFUSAL in treated_text and control_text!=treated_text and treated['state'].get('task_run',{}).get('phase')!='probe', 'control='+control_text+'\ntreated='+treated_text)
 def journal_leg():
  if held.get('refusal'):event(held['refusal']['state'],'cancel')
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS})
  for text in ['Partial report REQUIRED-ALPHA.','Candidate rules for the bounded report.','Completed report REQUIRED-ALPHA REQUIRED-BETA.','Revision rules for the bounded report.','Revised report missed the second phrase. REQUIRED-ALPHA.']:
   pending=reply(pending,text)
  settled=pending['state'].get('refinement_pending') or {}
  held['settled']=settled
  digest=settled.get('run_digest') if isinstance(settled.get('run_digest'),str) else ''
  ledger_name=digest+'.jsonl' if len(digest)==64 else ''
  ledger_path=default_root/ledger_name if ledger_name else None
  library_path=default_root/'experience.jsonl'
  ledger_rows=read_rows(ledger_path) if ledger_path else []
  library_rows=read_rows(library_path)
  held['ledger_name']=ledger_name
  held['ledger_rows']=ledger_rows
  linked=bool(ledger_rows) and all(row.get('run_digest')==digest and row_seal(row) for row in ledger_rows)
  chained=bool(ledger_rows) and ledger_rows[0].get('previous')=='' and all(ledger_rows[index].get('previous')==ledger_rows[index-1].get('revision') for index in range(1,len(ledger_rows)))
  this_library=[row for row in library_rows if row.get('run_digest')==digest]
  library_ok=bool(this_library) and all(row_seal(row) for row in this_library) and any(row.get('harness')==settled.get('composition') for row in this_library)
  named=settled.get('edit_ledger_root')==str(default_root) and settled.get('edit_ledger_path')==ledger_name and ledger_name!='edits.jsonl'
  no_fixed_name=not (default_root/'edits.jsonl').is_file()
  chain=[(row.get('previous')==('' if index==0 else ledger_rows[index-1].get('revision')), row.get('run_digest')==digest, row_seal(row), row.get('verdict'), (row.get('hypothesis') or '')[:64]) for index,row in enumerate(ledger_rows)]
  check('the digest-named ledger chains each revision to this run and experience.jsonl seals the settled row', named and linked and chained and library_ok and no_fixed_name and settled.get('phase')=='review', 'root='+str(settled.get('edit_ledger_root'))+' path='+str(settled.get('edit_ledger_path')) +' digest='+digest+' phase='+str(settled.get('phase'))+' files='+str(files_here()) +' edits.jsonl='+str((default_root/'edits.jsonl').is_file()) +' rows='+str(len(ledger_rows))+' linked='+str(linked)+' chained='+str(chained) +' library_ok='+str(library_ok)+' chain='+str(chain) +' library_error='+str(settled.get('library_error')))
 def resume_leg():
  close_product()
  open_product()
  settled=held.get('settled') or {}
  digest=settled.get('run_digest') if isinstance(settled.get('run_digest'),str) else ''
  ledger_name=digest+'.jsonl' if len(digest)==64 else ''
  rows=read_rows(default_root/ledger_name) if ledger_name else []
  rejected=[row for row in rows if row.get('verdict')=='rejected' and row.get('run_digest')==digest and isinstance(row.get('hypothesis'),str) and row.get('hypothesis')]
  chosen=rejected[0] if rejected else {}
  hypothesis=chosen.get('hypothesis') or ''
  component=chosen.get('component') if isinstance(chosen.get('component'),str) else ''
  round_no=chosen.get('round')
  round_text=str(int(round_no)) if isinstance(round_no,int) or (isinstance(round_no,str) and round_no.isdigit()) else ''
  refusal='refused because component '+component+' with that hypothesis was already rejected in round '+round_text
  harness=settled.get('composition') or ''
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS,'refinement_rounds':'2'})
  pending=reply(pending,'Original reply.')
  wire=(pending.get('effect') or {}).get('wire') or ''
  history_at=wire.find('EDIT HISTORY')
  library_at=wire.find('HISTORICAL EXPERIENCE')
  history=wire[history_at:library_at if library_at>history_at else None] if history_at>=0 else ''
  library=wire[library_at:] if library_at>=0 else ''
  # The library section is historical_experience. The ledger section is prompt_document,
  # which only resume_run rebuilds into a fresh process. A lesson may quote a hypothesis;
  # that quote is not the ledger prompt.
  library_read='Adopted harness (historical)' in library and bool(harness) and harness in library
  ledger_rebuilt='Rejected hypotheses (do not propose these again)' in history and bool(hypothesis) and hypothesis in history
  proposed=reply(pending,hypothesis or REJECTED)
  proposed_run=proposed['state'].get('task_run') or {}
  decision=proposed_run.get('round_note') or proposed['state'].get('notice') or ''
  phase=str(proposed_run.get('phase'))
  decision_resumed=bool(hypothesis) and bool(component) and bool(round_text) and refusal in decision and phase!='probe'
  files_alone=bool(ledger_name) and (default_root/ledger_name).is_file() and not (default_root/'edits.jsonl').is_file()
  if library_read and not ledger_rebuilt:
   gap='rrsi::resume_run is not reachable from the JSON-lines route; its only callers are the two unit tests in core/rrsi.rs. Library rows are read into the propose prompt. The ledger prompt is not rebuilt from '+ledger_name+'.'
  elif ledger_rebuilt and not decision_resumed:
   gap='The ledger prompt was rebuilt from '+ledger_name+' but the resumed decision did not refuse the recorded rejection. resume_run returns that ledger and its run digest; the JSON-lines route has not installed them on the run.'
  elif not library_read and not ledger_rebuilt:
   gap='The propose prompt has neither the library section nor a ledger prompt rebuilt from '+ledger_name+'.'
  else:
   gap='The resumed decision did not refuse the hypothesis recorded in '+ledger_name+'.'
  check('files alone rebuild the resumed phase and decision', library_read and ledger_rebuilt and decision_resumed and files_alone, gap+'\nlibrary_read='+str(library_read)+' ledger_rebuilt='+str(ledger_rebuilt) +' decision_resumed='+str(decision_resumed)+' files_alone='+str(files_alone) +'\ndecision='+decision+' phase='+phase+' ledger='+ledger_name +' hypothesis='+hypothesis[:180]+'\nedit_history='+str(history_at>=0) +' historical='+str(library_at>=0)+' files='+str(files_here()) +'\nprompt_tail='+wire[-700:])
 def recursion_leg():
  phrases='PHRASE-ONE\nPHRASE-TWO'
  skills_refusal='refused because component skills with that hypothesis was already rejected in round 1'
  ended='The first round ended the refinement. A second round requires a comparable partial gain.'
  def settle():
   state=event(None,'resume')['state']
   if state.get('task_run'):state=event(state,'cancel')['state']
   if state.get('refinement_pending'):
    state=event(state,'refinement_discard_review')['state']
    state=event(state,'refinement_discard')['state']
   state['profiles']=[dict(PROFILE)]
   state['agents']=[dict(AGENT)]
   return state
  def wire_of(pending):
   return ((pending.get('effect') or {}).get('wire') or '')
  observed={}
  try:
   seed=plant_git(room)
   state=settle()
   pending=event(state,'refine',{'task':'Flat skills task.','response_checks':phrases,'refinement_rounds':'2'})
   pending=reply(pending,'baseline says nothing')
   wire1=wire_of(pending)
   pending=reply(pending,'component: plugins\nhypothesis: keep the index\nindex body stays local')
   pending=reply(pending,'PHRASE-ONE only')
   pending=reply(pending,'component: skills\nhypothesis: flat one\nflat body one')
   pending=reply(pending,'replay says nothing')
   wire2=wire_of(pending)
   pending=reply(pending,'component: skills\nhypothesis: flat two\nflat body two')
   pending=reply(pending,'still nothing')
   pending=reply(pending,'component: skills\nhypothesis: flat three\nflat body three')
   pending=reply(pending,'replay still nothing')
   settled=pending['state'].get('refinement_pending') or {}
   digest=settled.get('run_digest') if isinstance(settled.get('run_digest'),str) else ''
   rows=read_rows(default_root/(digest+'.jsonl')) if len(digest)==64 else []
   observed['digest']=digest
   observed['rows']=rows
   observed['wire1']='Edits remaining this round: 4. The annealed budget is 4.' in wire1 and 'Pruned:' not in wire1
   observed['wire2']='Edits remaining this round: 3. The annealed budget is 3.' in wire2 and 'Pruned:' not in wire2
   observed['pruned_run']=settled.get('pruned_components')
   state=settle()
   pending=event(state,'refine',{'task':'Flat skills task.','response_checks':phrases,'refinement_rounds':'2'})
   pending=reply(pending,'baseline again')
   wire3=wire_of(pending)
   observed['wire3']='Edits remaining this round: 1. The annealed budget is 3.' in wire3
   observed['wire3_pruned']='Pruned:' in wire3
   observed['resumed']=pending['state'].get('task_run',{}).get('run_digest')
   pending=reply(pending,'component: skills\nhypothesis: flat one\nflat body proposed again')
   run=pending['state'].get('task_run') or {}
   observed['refusal']=run.get('round_note')
   observed['refusal_notice']=pending['state'].get('notice')
   observed['refusal_phase']=run.get('phase')
   state=settle()
   pending=event(state,'refine',{'task':'No gain task.','response_checks':phrases,'refinement_rounds':'2'})
   pending=reply(pending,'baseline says nothing')
   pending=reply(pending,'component: memory\nhypothesis: leave the floor\nmemory body stays local')
   pending=reply(pending,'still nothing')
   pending=reply(pending,'component: memory\nhypothesis: leave the floor again\nmemory body rewritten')
   pending=reply(pending,'replay still nothing')
   quiet=pending['state'].get('refinement_pending') or {}
   quiet_digest=quiet.get('run_digest') if isinstance(quiet.get('run_digest'),str) else ''
   observed['quiet_note']=quiet.get('round_note')
   observed['quiet_rows']=read_rows(default_root/(quiet_digest+'.jsonl')) if len(quiet_digest)==64 else []
   state=settle()
   pending=event(state,'refine',{'task':'Pool commit task.','response_check_method':'json_fields','response_checks':'{"/flag":""}','refinement_rounds':'1','evaluation_task':'Held out pool task.','evaluation_method':'phrases','evaluation_checks':'HELD-TOKEN'})
   pending=reply(pending,'baseline is not json')
   pending=reply(pending,'candidates: 1\n{"flag":""}')
   pool_run=pending['state'].get('task_run') or {}
   pool_digest=pool_run.get('run_digest') if isinstance(pool_run.get('run_digest'),str) else ''
   pool_rows=read_rows(default_root/(pool_digest+'.jsonl')) if len(pool_digest)==64 else []
   pool_row=pool_rows[-1] if pool_rows else {}
   pending=reply(pending,'HELD-TOKEN present')
   done=reply(pending,'no token here')
   reviewed=done['state'].get('refinement_pending') or {}
   base=reviewed.get('evaluation_base_score') or {}
   cand=reviewed.get('evaluation_candidate_score') or {}
   head_file=room/'.git'/'worktrees'/'c0'/'HEAD'
   commit=pool_row.get('commit')
   observed['pool']=dict(note=pool_run.get('round_note'),commit=commit,branch=pool_row.get('branch'),status=pool_row.get('pool_status'),before=pool_row.get('score_before'),after=pool_row.get('score_after'),held=[key for key in ('held_out_known','held_out_passed','held_out_task_key','held_out_verifier') if key in pool_row],tip=incumbent_tip(room),head=head_file.read_text().strip() if head_file.is_file() else '',harness=(room/'.rllm-worktrees'/'c0'/'rllm-harness.txt').read_text() if (room/'.rllm-worktrees'/'c0'/'rllm-harness.txt').is_file() else '',parent=('parent '+seed) in commit_text(room,commit or ''),seed=seed,main=(room/'.git'/'refs/heads'/'main').read_text().strip() if (room/'.git'/'refs/heads'/'main').is_file() else '',eval_base=base.get('passed'),eval_cand=cand.get('passed'))
   state=settle()
   drive={}
   observed['drive']=drive
   pending=event(state,'refine',{'task':'Drive three rounds.','response_check_method':'json_fields','response_checks':'{"/a":"","/b":""}','refinement_rounds':'3','evaluation_task':'Held out drive task.','evaluation_method':'phrases','evaluation_checks':'HELD-TOKEN'})
   drive['opened_phase']=(pending['state'].get('task_run') or {}).get('phase')
   pending=reply(pending,'baseline says nothing')
   drive['after_base']=outcome_text(pending)
   pending=reply(pending,'HELD-TOKEN is the separate answer')
   drive['propose1']=wire_of(pending)
   pending=reply(pending,'candidates: 3\n{"a":""}\n---\nCopy the task Drive three rounds. into the harness\n---\ncomponent: skills\nhypothesis: flat one\nflat body one')
   drive['after_pool']=outcome_text(pending)
   pending=reply(pending,'{"a":"","b":""}')
   drive['propose2']=wire_of(pending)
   pending=reply(pending,'component: plugins\nhypothesis: raise the flag\nplugin body stays local')
   pending=reply(pending,'{"a":"","b":""}')
   pending=reply(pending,'component: skills\nhypothesis: flat two\nflat body two')
   pending=reply(pending,'replay says nothing')
   drive['propose3']=wire_of(pending)
   pending=reply(pending,'component: skills\nhypothesis: flat three\nround three body')
   pending=reply(pending,'still nothing')
   pending=reply(pending,'component: skills\nhypothesis: flat four\nround three composed')
   pending=reply(pending,'still nothing')
   drive['after_round3']=outcome_text(pending)
   pending=reply(pending,'HELD-TOKEN is here')
   done=reply(pending,'no token')
   reviewed=done['state'].get('refinement_pending') or {}
   digest=reviewed.get('run_digest') if isinstance(reviewed.get('run_digest'),str) else ''
   drive_rows=read_rows(default_root/(digest+'.jsonl')) if len(digest)==64 else []
   head_file=room/'.git'/'worktrees'/'c0'/'HEAD'
   harness_file=room/'.rllm-worktrees'/'c0'/'rllm-harness.txt'
   drive['rows']=drive_rows
   drive['evidence']=reviewed.get('evidence') or ''
   drive['pruned']=reviewed.get('pruned_components')
   drive['phase']=reviewed.get('phase')
   drive['tip']=incumbent_tip(room)
   drive['head']=head_file.read_text().strip() if head_file.is_file() else ''
   drive['harness']=harness_file.read_text() if harness_file.is_file() else ''
   drive['main']=(room/'.git'/'refs/heads'/'main').read_text().strip() if (room/'.git'/'refs/heads'/'main').is_file() else ''
   drive['seed']=seed
   print('DRIVE '+json.dumps({key:drive[key] for key in ('opened_phase','after_base','after_pool','after_round3','phase','pruned','tip','head','harness','main')},default=str),flush=True)
   settle()
  except Exception:
   traceback.print_exc()
   observed['crash']=traceback.format_exc()[-1500:]
  rows=observed.get('rows') or []
  got=[(row.get('round'),row.get('component'),row.get('hypothesis'),row.get('verdict'),row.get('edit_budget'),row.get('noise_floor'),row.get('decision'),row.get('score_before'),row.get('score_after')) for row in rows]
  wanted=[(1,'skills','flat one','rejected',4,0,'rejected revision',0,0),(1,'plugins','keep the index','accepted',4,0,'cost not recorded',0,1),(2,'skills','flat three','rejected',3,0,'score',1,0)]
  detail='crash='+str(observed.get('crash') or '')+'\nrows='+str(got)+'\nresumed='+str(observed.get('resumed'))+' digest='+str(observed.get('digest'))+'\nwires='+str((observed.get('wire1'),observed.get('wire2'),observed.get('wire3')))+' gain=-1 cost=(68,56) scores 1 to 0; lower cost needs equal scores, cost rose needs a longer text, cost bound needs an admitted gain, so reason stays score'
  check('three propose prompts anneal remaining edits from 4 to 3 to 1', observed.get('wire1') is True and observed.get('wire2') is True and observed.get('wire3') is True and got==wanted and rows[2].get('accept_reason')=='score' and rows[2].get('cost_known') is True and rows[2].get('cost_before')==68 and rows[2].get('cost_after')==56 and rows[1].get('cost_known') is False and rows[1].get('cost_after')==68 and observed.get('resumed')==observed.get('digest') and len(str(observed.get('digest')))==64, detail)
  drive=observed.get('drive') or {}
  drive_rows=drive.get('rows') or []
  def quoted_digest(value):
   return hashlib.sha256(json.dumps(value,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
  budgets={}
  for row in drive_rows:
   if 'edit_budget' not in row:continue
   budgets.setdefault(row.get('round'),set()).add(row.get('edit_budget'))
  leak=[row for row in drive_rows if row.get('leakage')=='task_text']
  floor=[row for row in drive_rows if row.get('component')=='plugins']
  skills=[row.get('round') for row in drive_rows if row.get('component')=='skills']
  floor_row=floor[0] if len(floor)==1 else {}
  propose1='Edits remaining this round: 4. The annealed budget is 4.' in drive.get('propose1','') and 'Pruned:' not in drive.get('propose1','')
  propose2='Edits remaining this round: 3. The annealed budget is 3.' in drive.get('propose2','') and 'Pruned:' not in drive.get('propose2','')
  propose3='Edits remaining this round: 2. The annealed budget is 2.' in drive.get('propose3','') and 'Pruned:' not in drive.get('propose3','')
  commit=next((row.get('commit') for row in drive_rows if row.get('branch')=='rllm-evolve'),None)
  held_keys=('held_out_known','held_out_passed','held_out_task_key','held_out_verifier')
  check('three refinement rounds record budgets 4, 3 and 2, a leakage refusal, noise floor 2, pruned skills, the evolve commit, and held-out keys', propose1 and propose2 and propose3 and budgets=={1:{4},2:{3},3:{2}} and len(leak)==1 and leak[0].get('round')==1 and leak[0].get('decision')=='refused because the candidate contains a recorded task' and 'score_before' not in leak[0] and 'score_after' not in leak[0] and leak[0].get('verdict')=='rejected' and floor_row.get('decision')=='refused because the gain 1 does not exceed the noise floor 2' and floor_row.get('noise_floor')==2 and floor_row.get('round')==2 and floor_row.get('score_before')==1 and floor_row.get('score_after')==2 and skills==[1,2,3] and drive.get('pruned')==['skills'] and 'Pruned: skills.' in drive.get('evidence','') and 'The annealed budget is 1.' in drive.get('evidence','') and commit_hex(commit) and len(commit)==40 and commit==drive.get('tip') and commit==drive.get('head') and drive.get('harness')=='{"a":""}' and drive.get('main')==drive.get('seed') and floor_row.get('held_out_known') is True and floor_row.get('held_out_passed')==1 and floor_row.get('held_out_task_key')==quoted_digest('Held out drive task.') and floor_row.get('held_out_verifier')==quoted_digest(['HELD-TOKEN']) and all(key in floor_row for key in held_keys) and drive.get('phase')=='review', 'crash='+str(observed.get('crash') or '')+'\nbudgets='+str(budgets)+'\nskills='+str(skills)+'\nleak='+str([{key:row.get(key) for key in ('round','decision','verdict','leakage','score_before','score_after')} for row in leak])+'\nfloor='+str({key:floor_row.get(key) for key in ('decision','noise_floor','round','score_before','score_after',*held_keys)})+'\npruned='+str(drive.get('pruned'))+'\ncommit='+str(commit)+' tip='+str(drive.get('tip'))+' head='+str(drive.get('head'))+' harness='+str(drive.get('harness'))+' main='+str(drive.get('main'))+' seed='+str(drive.get('seed'))+'\nphases='+str((drive.get('opened_phase'),drive.get('after_base'),drive.get('after_pool'),drive.get('after_round3'),drive.get('phase')))+'\nrows='+str([(row.get('round'),row.get('component'),row.get('hypothesis'),row.get('verdict'),row.get('edit_budget'),row.get('noise_floor'),row.get('decision'),row.get('score_before'),row.get('score_after')) for row in drive_rows]))
  check('a re-proposed skills hypothesis is refused by name for round 1', observed.get('refusal')==skills_refusal and observed.get('refusal_notice')==skills_refusal and observed.get('refusal_phase')=='propose', 'note='+str(observed.get('refusal'))+' notice='+str(observed.get('refusal_notice'))+' phase='+str(observed.get('refusal_phase')))
  quiet_rows=observed.get('quiet_rows') or []
  quiet_row=quiet_rows[-1] if len(quiet_rows)==1 else {}
  check('a round that gains nothing records noise floor 0 and decision cost not recorded', len(quiet_rows)==1 and quiet_row.get('noise_floor')==0 and quiet_row.get('decision')=='cost not recorded' and quiet_row.get('score_before')==0 and quiet_row.get('score_after')==0 and observed.get('quiet_note')==ended, 'note='+str(observed.get('quiet_note'))+' rows='+str([{key:row.get(key) for key in ('noise_floor','decision','score_before','score_after','round')} for row in quiet_rows]))
  check('two flat skills rounds leave the budget line unpruned', observed.get('wire1') is True and observed.get('wire2') is True and observed.get('wire3_pruned') is False and not observed.get('pruned_run') and got==wanted and rows[2].get('decision')=='score' and rows[2].get('cost_before')==68 and rows[2].get('cost_after')==56 and all('was pruned' not in str(row.get('decision')) for row in rows), 'pruned_run='+str(observed.get('pruned_run'))+' wire3_pruned='+str(observed.get('wire3_pruned'))+' decisions='+str([row.get('decision') for row in rows])+' gain=-1 cost=(68,56) reason stays score')
  pool=observed.get('pool') or {}
  commit=pool.get('commit')
  check('the incumbent commit in .rllm-worktrees is the evolve tip', commit_hex(commit) and len(commit)==40 and commit!=pool.get('seed') and commit==pool.get('tip') and commit==pool.get('head') and pool.get('branch')=='rllm-evolve' and pool.get('status')=='admissible' and pool.get('harness')=='{"flag":""}' and pool.get('parent') is True and pool.get('main')==pool.get('seed') and pool.get('before')==0 and pool.get('after')==1 and pool.get('note')=='kept candidate 0', 'pool='+str(pool))
  check('the separate evaluation score 0 differs from the in-round score 1 and is not a held-out key', pool.get('held')==[] and pool.get('eval_base')==1 and pool.get('eval_cand')==0 and pool.get('after')==1 and pool.get('eval_cand')!=pool.get('after'), 'pool='+str({key:pool.get(key) for key in ('held','eval_base','eval_cand','after','before')}))
 def recursion_follow():
  for name in recursion:
   if name not in evaluated:check(name,False,'recursion leg did not record this check')
 STALL_CHECK='a stalled ledger refuses an already-tried component and does not refuse one that was never tried'
 SHAPE_CHECK='a flat memory shape is accepted for structural novelty, a shorter equal score is accepted for lower cost, and the same shape is refused when the cost rose'
 def settle_gap():
  state=event(None,'resume')['state']
  if state.get('task_run'):state=event(state,'cancel')['state']
  if state.get('refinement_pending'):state=event(event(state,'refinement_discard_review')['state'],'refinement_discard')['state']
  state['profiles']=[dict(PROFILE)];state['agents']=[dict(AGENT)];return state
 def drive_gap(task,checks,steps):
  pending=event(settle_gap(),'refine',{'task':task,'response_checks':checks,'refinement_rounds':'3'})
  for text in steps:pending=reply(pending,text)
  return pending
 def ledger_gap(pending):
  run=pending['state'].get('refinement_pending') or pending['state'].get('task_run') or {};digest=run.get('run_digest') if isinstance(run.get('run_digest'),str) else ''
  return read_rows(default_root/(digest+'.jsonl')) if len(digest)==64 else []
 def edit_text(name,hypothesis,body):return 'component: '+name+'\nhypothesis: '+hypothesis+'\n'+body
 def round_trip(text):return [text,'nothing here',text,'nothing here']
 def scored_band(rows):
  best,floor,components={},0,set()
  for row in rows:
   if isinstance(row.get('component'),str):components.add(row['component'])
   if isinstance(row.get('score_before'),int) and isinstance(row.get('score_after'),int):
    gain=row['score_after']-row['score_before'];rnd=row.get('round');best[rnd]=max(best.get(rnd,gain),gain);floor=max(floor,row['noise_floor']) if isinstance(row.get('noise_floor'),int) else floor
  return best,floor,components
 def require_check(ok,name,detail):
  if not ok:print(detail,flush=True)
  assert ok,name;check(name,True)
 def stall_leg():
  task,checks,sentence='Stall band task.','BAND-A\nBAND-B','refused because component skills was already tried while progress stayed inside the noise band for 3 rounds'
  drive_gap(task,checks,['baseline says nothing',*round_trip(edit_text('skills','stall one','skills body one')),'BAND-A is the second base',*round_trip(edit_text('memory','stall two','memory body two')),*round_trip(edit_text('tools','stall three','tools body three'))])
  pending=drive_gap(task,checks,['baseline says nothing',*round_trip(edit_text('subagents','stall extra','subagents body extra')),'BAND-A opens the live floor',edit_text('skills','stall again','skills body again')])
  run=pending['state'].get('task_run') or {};rows=ledger_gap(pending);best,floor,components=scored_band(rows)
  tried=next((row for row in rows if row.get('hypothesis')=='stall again'),{})
  refused=run.get('round_note')==sentence and run.get('phase')=='propose' and tried.get('decision')==sentence and len(best)>=3 and floor>=1 and all(gain<=floor for gain in best.values()) and 'config' not in components
  retried=event(pending['state'],'retry');control=reply(retried,edit_text('config','never tried','config body fresh'))
  control_run=control['state'].get('task_run') or {};control_rows=ledger_gap(control);notice=control['state'].get('notice') or ''
  same=len(control_rows)==len(rows) and not any(row.get('component')=='config' for row in control_rows) and control_run.get('phase')=='probe' and sentence not in notice
  detail='note='+str(run.get('round_note'))+' phase='+str(run.get('phase'))+' best='+str(sorted((str(key),value) for key,value in best.items()))+' floor='+str(floor)+' components='+str(sorted(components))+' tried='+str(tried.get('decision'))+' control_phase='+str(control_run.get('phase'))+' notice='+notice+' rows='+str(len(rows))+'/'+str(len(control_rows))
  require_check(refused and same,STALL_CHECK,detail)
 def shape_leg():
  task,checks,novel,rose,lower='Novelty dup task.','MARK-A\nMARK-B','accepted because of structural novelty','refused because the cost rose while the score did not','accepted because the equal score costs less'
  one,two=edit_text('memory','flat one','body alpha'),edit_text('memory','flat two','body beta')
  drive_gap(task,checks,['baseline says nothing',*round_trip(edit_text('skills','opener','body opener')),'MARK-A is the second base',*round_trip(one),*round_trip(two)])
  pending=event(settle_gap(),'refine',{'task':task,'response_checks':checks,'refinement_rounds':'3'})
  pending=reply(pending,'baseline says nothing')
  for component in ['prompt','control flow','config','context management','tools','subagents','skills']:
   pending=reply(pending,'component: '+component+'\nhypothesis: leak '+component+'\nLeak task '+task);pending=event(pending['state'],'retry')
  for text in [*round_trip(edit_text('prompt','filler','body filler')),'MARK-A opens the floor',*round_trip(one)]:pending=reply(pending,text)
  rows=ledger_gap(pending)
  first=next((row for row in rows if row.get('hypothesis')=='flat one' and row.get('decision')==novel),{})
  again=next((row for row in rows if row.get('hypothesis')=='flat one' and row.get('decision')==rose),{})
  other=next((row for row in rows if row.get('hypothesis')=='flat two' and row.get('decision')==lower),{})
  triple=lambda row:tuple(row.get(key) for key in ('component','hypothesis','diff_digest'))
  ok=bool(first) and bool(again) and bool(other) and triple(first)==triple(again) and other.get('diff_digest')!=first.get('diff_digest') and first.get('accept_reason')=='structural novelty' and first.get('cost_known') is False and first.get('cost_before')==15 and first.get('cost_after')==49 and again.get('accept_reason')=='cost rose' and again.get('cost_known') is True and again.get('cost_before')==48 and again.get('cost_after')==49 and other.get('accept_reason')=='lower cost' and other.get('cost_known') is True and other.get('cost_before')==49 and other.get('cost_after')==48 and first.get('beta0')==again.get('beta0')==other.get('beta0')==0 and first.get('beta1')==again.get('beta1')==other.get('beta1')==64 and first.get('verdict')=='accepted' and again.get('verdict')=='rejected' and other.get('verdict')=='accepted' and first.get('score_before')==again.get('score_before')==other.get('score_before')==0 and first.get('score_after')==again.get('score_after')==other.get('score_after')==0 and first.get('noise_floor')==again.get('noise_floor')==other.get('noise_floor')==1 and first.get('decision')!=again.get('decision')
  detail='gain 0. flat one has no incumbent cost, so structural novelty fires. flat two cost (49,48) takes the lower-cost arm before novelty. the repeated flat one cost (48,49) takes cost rose before the duplicate-shape block. first='+str({key:first.get(key) for key in ('decision','accept_reason','cost_before','cost_after','verdict')})+' again='+str({key:again.get(key) for key in ('decision','accept_reason','cost_before','cost_after','verdict')})+' other='+str({key:other.get(key) for key in ('decision','accept_reason','cost_before','cost_after','verdict')})+' n='+str(len(rows))
  require_check(ok,SHAPE_CHECK,detail)
 def tamper_leg():
  library=default_root/'experience.jsonl'
  if not library.is_file():
   check('a tampered library entry is refused and its bytes stay unchanged',False,'experience.jsonl was not written')
   return
  original=library.read_bytes()
  token=b'Candidate rules for the bounded report.'
  at=original.find(token)
  if at<0:
   check('a tampered library entry is refused and its bytes stay unchanged',False,'sealed harness text was not in experience.jsonl')
   return
  tampered=bytearray(original)
  tampered[at]=ord('X') if tampered[at]!=ord('X') else ord('Y')
  library.write_bytes(tampered)
  before=sha256(library)
  close_product()
  open_product()
  pending=event(fresh(),'refine',{'task':TASK,'response_checks':CHECKS})
  pending=reply(pending,'Original reply.')
  wire=(pending.get('effect') or {}).get('wire') or ''
  after=sha256(library)
  refused='library record digest does not match its body; existing history was left unchanged' in wire
  adopted='Adopted harness (historical)' in wire
  check('a tampered library entry is refused and its bytes stay unchanged', refused and not adopted and before==after and library.read_bytes()==bytes(tampered), 'refused='+str(refused)+' adopted_line='+str(adopted)+' before='+before+' after='+after)
 recursion=['three propose prompts anneal remaining edits from 4 to 3 to 1','three refinement rounds record budgets 4, 3 and 2, a leakage refusal, noise floor 2, pruned skills, the evolve commit, and held-out keys','a re-proposed skills hypothesis is refused by name for round 1','a round that gains nothing records noise floor 0 and decision cost not recorded','two flat skills rounds leave the budget line unpruned','the incumbent commit in .rllm-worktrees is the evolve tip','the separate evaluation score 0 differs from the in-round score 1 and is not a held-out key']
 names=['refine entry copies approval_root onto task_run.workspace and records source_digest','a non-refine run carries no workspace and does not attempt the pool','a second round decision changes when the ledger already rejected that component','the digest-named ledger chains each revision to this run and experience.jsonl seals the settled row','files alone rebuild the resumed phase and decision',*recursion,STALL_CHECK,SHAPE_CHECK,'a tampered library entry is refused and its bytes stay unchanged']
 bodies=[workspace_leg,non_refine_leg,critic_leg,journal_leg,resume_leg,recursion_leg,*([recursion_follow]*(len(recursion)-1)),stall_leg,shape_leg,tamper_leg]
 try:
  open_product()
 except Exception:
  traceback.print_exc()
  for name in names:
   if name not in evaluated:check(name,False,'product setup failed\n'+traceback.format_exc()[-1500:])
 else:
  for name,body in zip(names,bodies):leg(name,body)
 finally:
  err_path=room/'product.err'
  close_product()
  err_text=err_path.read_text()[-4000:] if err_path.exists() else ''
  if err_text and problems:print(err_text,flush=True)
 record=dict(checks=evaluated,count=len(evaluated),failed=problems,scope='Owner checks through the product JSON-lines route: refine copies approval_root onto task_run.workspace and records source_digest, the digest-named ledger chains revisions for that run and experience.jsonl seals the settled row, a second round reads a recorded rejection, resume rebuilds the ledger prompt and decision from those files alone, a tampered library entry is refused without a rewrite, and delegate or compact carries no workspace and does not run the pool. A resumed refine records propose budgets 4/4, 3/3 and 3/1, refuses a repeated skills hypothesis by the round 1 name, records noise floor 0 and decision cost not recorded when a round gains nothing, leaves two flat skills rounds unpruned, stores the accepted incumbent as the rllm-evolve worktree commit, and keeps the separate evaluation score off the held-out keys. A three-round refine records budgets 4, 3 and 2, a leakage refusal before scoring, noise floor 2, pruned skills, the evolve commit, and the four held-out keys. A stalled ledger refuses an already-tried component while an untried component is not refused. A flat memory shape is accepted for structural novelty while its cost is unknown. The next equal score, one byte shorter, is accepted because the cost fell. The same shape one byte longer is refused because the cost rose.')
 if not problems:record['passed']=passed
 out=PROJECT/'output/checks'
 out.mkdir(parents=True,exist_ok=True)
 (out/'rrsi-loop.json').write_text(json.dumps(record,indent=2)+'\n')
 if not problems:print(str(len(passed))+' rrsi-loop checks passed',flush=True)
 print(json.dumps(record,indent=2),flush=True)
 if problems:raise SystemExit(1)
