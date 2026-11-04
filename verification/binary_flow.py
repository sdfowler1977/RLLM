"""Actual assembled ELF -> reviewed inspection -> retained query/native view/export."""
import argparse,base64,copy,hashlib,json,os,pathlib,re,subprocess,tempfile,zipfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];BIN=PROJECT/'output/headless/RLLM';passed=[]
parser=argparse.ArgumentParser();parser.add_argument('--previous-flow-controller',type=pathlib.Path,default=PROJECT/'output/pre-branch-controller');parser.add_argument('--previous-auth-controller',type=pathlib.Path,default=PROJECT/'output/pre-auth-controller');parser.add_argument('--published-controller',type=pathlib.Path,default=PROJECT/'output/published-controller');parser.add_argument('--auth-baseline',type=pathlib.Path);parser.add_argument('--auth-baseline-sha256');args=parser.parse_args();retained_auth=None
if args.auth_baseline:
 baseline=args.auth_baseline.read_bytes();baseline_hash=hashlib.sha256(baseline).hexdigest();origin='3d51564e00233805e4cb6cbc0145d1227ed125b7545a036e4a9cb0c8aaebaa6a';assert baseline_hash==(args.auth_baseline_sha256 or origin)
 with zipfile.ZipFile(args.auth_baseline)as z:
  manifest=json.loads(z.read('RLLM/PUBLICATION-MANIFEST.json'));prior=manifest['validation'].get('retained_compatibility')
  if baseline_hash!=origin:assert prior and prior['source_sha256']==origin
  # Only retained graph readers are fingerprinted here. The original three
  # outcomes stay historical; current dispatch and views are exercised afresh.
  reader_names=['lib','binary_evidence','binary_flow','binary_declarations','control_flow','observation_graph','observation_records','file_evidence','mcp_schema','harness_rules','layout','reconstruction','pe_observation','incremental_digest']
  reader_paths={'core/'+name+'.rs'for name in reader_names}
  inherited=(prior.get('inputs')if prior else None)or manifest['build_inputs']
  assert inherited and reader_paths<=set(inherited)
  inputs={name:inherited[name]for name in sorted(reader_paths)}
  for name,digest in inputs.items():assert hashlib.sha256((PROJECT/name).read_bytes()).hexdigest()==digest,('Changed historical-reader input requires fresh compatibility execution',name)
  names=manifest['validation']['checks']['binary_declarations_and_flow']['passed'][-3:];assert names==['actual revision-two controller retains original authenticated-word meaning','current native graph accepts actual revision-two bytes','revision-two authenticated word remains historical evidence'];retained_auth=dict(source_sha256=origin,version=1793600489,retained_from=dict(version=manifest['version'],sha256=baseline_hash),inputs=inputs,checks=names,scope='Previously executed owner checks from verified signed source; named decoder/representation-reader modules unchanged. Current dispatch, native views, HTTP decoding, capture-time interpretation and approved-command completion are exercised separately with current and actual published captures. These three checks describe their original historical execution and were not re-executed.')
def check(name,value):assert value,name;passed.append(name)
ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
with tempfile.TemporaryDirectory(prefix='flow-',dir=PROJECT/'output') as temp:
 root=pathlib.Path(temp);asm=root/'original.S';elf=root/'original.elf'
 asm.write_text('''.text
.global entry
.global target
.global path_edge
.global arithmetic_target
.global consistent_edge
.type entry,%function
entry:
 bl target
 b finish
compare:
 cbz x0,target
 tbz x1,#2,compare
 b.eq target
consistent_edge:
 bc.eq target
 blr x2
 br x3
target:
 ret
finish:
 nop
literal:
 .word 0x11223344
resume_code:
 nop
 mov x4,#41
path_edge:
 b arithmetic_target
 mov x4,#99
arithmetic_target:
 add x5,x4,#1
 ret
.global auth_braa
auth_braa:
 braa x3,x4
.global auth_braaz
auth_braaz:
 braaz x5
.global auth_brab
auth_brab:
 brab x6,sp
.global auth_brabz
auth_brabz:
 brabz x7
.global auth_blraa
auth_blraa:
 blraa x8,x9
.global auth_blraaz
auth_blraaz:
 blraaz x10
.global auth_blrab
auth_blrab:
 blrab x11,sp
.global auth_blrabz
auth_blrabz:
 blrabz x12
.global auth_retaa
auth_retaa:
 retaa
.global auth_retab
auth_retab:
 retab
.global knowledge_start
.global knowledge_mask
knowledge_start:
 movk x16,#0x1234,lsl#16
 mov x10,#16
 lsrv x17,x16,x10
knowledge_mask:
 and x18,x17,#0xffff
 ret
.global join_start
.global join_mask
.global join_follow
join_start:
 mov x4,#41
 b join_mask
 mov x4,#99
join_mask:
 and x6,x4,#1
join_follow:
 add x7,x6,#1
 ret
.global arithmetic_bits_start
.global arithmetic_bits_mask
arithmetic_bits_start:
 movk x24,#0x1234,lsl#16
 add x25,x24,#1
arithmetic_bits_mask:
 and x26,x25,#0xfffe0000
 ret
.global selection_start
.global selection_edge
.global selection_result
selection_start:
 mov x19,#41
 mov x20,#43
selection_edge:
 csel x21,x19,x20,eq
 and x22,x21,#0xfffffffffffffffd
selection_result:
 add x23,x22,#1
 ret
.size entry,.-entry
''')
 subprocess.run([str(ndk/'aarch64-linux-android26-clang'),'-march=armv8.8-a','-nostdlib','-static','-Wl,--build-id=none','-Wl,--emit-relocs','-Wl,-e,entry',str(asm),'-o',str(elf)],check=True,env=dict(os.environ,TMPDIR=str(root)))
 symbols={name:int(address,16) for address,kind,name in (line.split() for line in subprocess.check_output([str(ndk/'llvm-nm'),str(elf)],text=True).splitlines() if len(line.split())==3)}
 disassembly=subprocess.check_output([str(ndk/'llvm-objdump'),'-d','--mattr=+hbc,+pauth',str(elf)],text=True);check('independent assembler and disassembler produce branch fixture',all(x in disassembly for x in ['bl\t','cbz\t','tbz\t','ret']))
 p=subprocess.Popen([str(BIN),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def raw(v):p.stdin.write(json.dumps(v)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
 def op(v):
  r=raw(v);assert 'value' in r,r;return r['value']
 def event(st,a,i=None):return op(dict(op='event',root=str(root),state=st,action=a,inputs=i or {}))
 def collect_fields(v):
  if isinstance(v,dict):
   if 'value' in v and isinstance(v['value'],str):yield v['value']
   for x in v.values():yield from collect_fields(x)
  elif isinstance(v,list):
   for x in v:yield from collect_fields(x)
 try:
  d=event(None,'resume');d=event(d['state'],'investigation');d=event(d['state'],'inspect_file',{'path':'original.elf'});check('native inspection enters existing review',d['state']['dialog']=='approval')
  d=event(d['state'],'approve');effect=d['effect'];check('review creates owned shared-engine worker',effect['kind']=='tool' and effect['tool']=='inspect' and bool(effect['tool_ticket']))
  result=op(dict(effect,op='tool',root=str(root)));payload=json.loads(result);graph=payload['snapshots'][0];flow=graph['control_flow'];rows=flow['relations'];nodes=graph['nodes'];by_address={n['encoding']['address']:n['id'] for n in nodes[1:]}
  check('inspection retains typed source-bound control-flow',flow['kind']=='encoded_control_flow' and flow['coverage']=='Partial' and rows)
  consistent_node=nodes[by_address[symbols['consistent_edge']]];consistent=next(r for r in rows if r['source']==consistent_node['id']and r['relation']=='encoded_conditional_target');check('independent assembler recognizes BC.cond encoding','bc.eq'in disassembly and consistent_node['encoding']['mnemonic']=='BC.cond');check('consistent branch retains exact target and feature hint',consistent['target_address']==symbols['target']and consistent['operand']['consistency_hint']is True and consistent['operand']['required_feature']=='FEAT_HBC'and consistent['operand']['condition']==0 and graph['decoder_version']==3);check('consistent branch retains a separate conditional continuation',any(r['source']==consistent_node['id']and r['relation']=='candidate_not_taken_address'and r['target_address']==symbols['consistent_edge']+4 for r in rows))
  auth_rows={}
  for name,kind,register,modifier,key in [('braa',0,3,4,'A'),('braaz',0,5,None,'A'),('brab',0,6,31,'B'),('brabz',0,7,None,'B'),('blraa',1,8,9,'A'),('blraaz',1,10,None,'A'),('blrab',1,11,31,'B'),('blrabz',1,12,None,'B'),('retaa',2,30,31,'A'),('retab',2,30,31,'B')]:
   node=nodes[by_address[symbols['auth_'+name]]];relation=['encoded_authenticated_indirect_branch','encoded_authenticated_indirect_call','encoded_authenticated_indirect_return'][kind];r=next(r for r in rows if r['source']==node['id']and r['relation']==relation);operand=r['operand'];auth_rows[name]=r;check('independent '+name+' retains exact authenticated operands',name in disassembly and node['encoding']['mnemonic']==name.upper()and operand['register']==register and operand['modifier_register']==modifier and operand['authentication_key']==key and operand['required_feature']=='FEAT_PAuth'and r['target_address']is None and r['target_node']is None);check(name+' retains modifier and encoded link semantics',operand['modifier_kind']==('zero'if modifier is None else'SP'if modifier==31 else'register')and(any(x['source']==node['id']and x['relation']=='encoded_link_return_address'and x['target_address']==symbols['auth_'+name]+4 for x in rows)==(kind==1)))
  check('authenticated return retains the conditional second-modifier context',all(auth_rows[n]['operand']['second_modifier']=={'register':16,'condition':'FEAT_PAuth_LR and PSTATE.PACM = 1'}for n in ['retaa','retab']))
  declarations=graph['declarations'];check('typed declaration inventory retains original parser facts',declarations['kind']=='binary_declarations' and declarations['mapping_inventory_complete'])
  headers=subprocess.check_output([str(ndk/'llvm-readobj'),'--program-headers',str(elf)],text=True)
  refs=re.findall(r'Type: PT_LOAD.*?Offset: (0x[0-9A-Fa-f]+|[0-9]+).*?VirtualAddress: (0x[0-9A-Fa-f]+|[0-9]+).*?FileSize: (0x[0-9A-Fa-f]+|[0-9]+).*?MemSize: (0x[0-9A-Fa-f]+|[0-9]+)',headers,re.S);assert refs,headers
  expected=[tuple(int(v,0) for v in row) for row in refs];actual=[tuple(row[k] for k in ['file_offset','address','file_size','memory_size']) for row in declarations['mappings']['rows']];check('LOAD declarations match independent program headers',actual==expected)
  classified=declarations['classifications']['rows'];check('mapping-symbol data interval remains explicit',any(not row['code_declaration'] and row['start']<=symbols['literal']<row['end'] and row['excludes_whole_data_words'] for row in classified))
  check('declared data is excluded from encoding nodes',symbols['literal'] not in by_address and symbols['resume_code'] in by_address)
  reference=subprocess.check_output([str(ndk/'llvm-readobj'),'--relocations',str(elf)],text=True);relocations=declarations['relocations']['rows'];call_decl=next(row for row in relocations if row['name']=='CALL26');check('actual relocation declaration matches independent reference',call_decl['code']==283 and call_decl['position']==symbols['entry'] and 'R_AARCH64_CALL26' in reference)
  check('relocation is a declaration without applied binding',call_decl['addend_kind']=='explicit signed declaration' and 'Declarations from the held source'in declarations['meaning'])
  call=next(r for r in rows if r['source']==by_address[symbols['entry']] and r['relation']=='encoded_call_target')
  check('BL target matches independently linked symbol',call['target_address']==symbols['target'] and call['target_node']==by_address[symbols['target']])
  link=next(r for r in rows if r['source']==call['source'] and r['relation']=='encoded_link_return_address');check('link address kept separate from call target',link['target_address']==symbols['entry']+4 and link['target_address']!=call['target_address'])
  branch=next(r for r in rows if r['source']==by_address[symbols['entry']+4]);check('B target matches independently linked symbol',branch['target_address']==symbols['finish'])
  compare=next(r for r in rows if r['source']==by_address[symbols['compare']] and r['relation']=='encoded_compare_target');check('CBZ target and register retained',compare['target_address']==symbols['target'] and compare['operand']['register']==0)
  ret=next(r for r in rows if r['relation']=='encoded_indirect_return');check('return target stays unresolved',ret['target_address'] is None and ret['target_node'] is None and ret['operand']['register']==30)
  check('data has no execution/reachability claim','execution' in flow['meaning'] and 'unresolved' in flow['meaning'])
  path_node=nodes[by_address[symbols['arithmetic_target']]];path_text=path_node['content']
  check('actual ELF retains separate predecessor alternatives','X5 <- unknown' in path_text and 'encoded integer 0x2a' in path_text and 'encoded integer 0x64' in path_text and 'Other incoming entries remain unknown' in path_text)
  check('captured predecessor join retains common bits separately','Captured-entry join'in path_text and'X4 <- partial integer bits0x21, known mask0xffffffffffffffb5'in path_text and'across 2 distinct predecessor edges'in path_text);check('captured join preserves the unknown external-entry interpretation','X5 <- unknown'in path_text and'External entries keep the original unknown-entry interpretation.'in path_text)
  check('predecessor identity matches independent assembled edge',('edge at ELF declared VA 0x%x'%symbols['path_edge']) in path_text or ('edge at declared VA 0x%x'%symbols['path_edge']) in path_text)
  check('path representation authenticates exact retained text',path_node['representation']['bytes']==len(path_text.encode()) and path_node['representation']['sha256']==hashlib.sha256(path_text.encode()).hexdigest())
  knowledge_node=nodes[by_address[symbols['knowledge_mask']]];check('known mask and variable shift retain the original encoded constant',knowledge_node['encoding']['mnemonic']=='AND'and'X18 <- encoded integer 0x1234'in knowledge_node['content']);check('knowledge source is independently assembled and disassembled','movk'in disassembly and'lsr'in disassembly and'and'in disassembly)
  join_mask=nodes[by_address[symbols['join_mask']]];join_node=nodes[by_address[symbols['join_follow']]];join_text=join_node['content']
  check('common captured-entry bits feed the following mask','Captured-entry interpretation'in join_mask['content']and'X6 <- encoded integer 0x1'in join_mask['content'])
  check('common captured-entry result feeds successor arithmetic','Captured-entry interpretation'in join_text and'under its 2 captured incoming edges'in join_text and'X7 <- encoded integer 0x2'in join_text and'X7 <- partial integer bits0x0, known mask0xfffffffffffffffc'in join_text.split('\n',1)[0])
  arithmetic_node=nodes[by_address[symbols['arithmetic_bits_mask']]];selection_node=nodes[by_address[symbols['selection_edge']]];selection_result=nodes[by_address[symbols['selection_result']]];selection_text=selection_result['content']
  check('partial addition retains a constant through the following mask','X26 <- encoded integer 0x12340000'in arithmetic_node['content'])
  check('conditional selection retains common bits through following arithmetic','X21 <- partial integer bits0x29, known mask0xfffffffffffffffd'in selection_node['content']and'X23 <- encoded integer 0x2a'in selection_text)
  check('independent assembler supplies the arithmetic and select encodings','csel'in disassembly and arithmetic_node['encoding']['mnemonic']=='AND'and selection_node['encoding']['mnemonic']=='CSEL'and selection_result['encoding']['mnemonic']=='ADD')
  inputs={k:effect[k] for k in ['tool_ticket','tool_binding','editor','editor_dialog'] if k in effect};inputs.update(purpose='local_tool',output=result)
  d=event(d['state'],'tool_result',inputs);records=d['state']['investigation_records']['items'];record=records[-1];check('real worker receipt retains graph',record['snapshots'][0]['control_flow']==flow)
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');text='\n'.join(collect_fields(d['ui']));check('native graph page displays relationship meaning','Encoded control-flow relationships' in text and 'encoded_call_target' in text);check('native view includes typed declaration summaries','Typed binary declarations' in text and 'relocations:' in text)
  d=event(d['state'],'investigation_graph_node:'+str(call['source']));text='\n'.join(collect_fields(d['ui']));check('native node view contains its outgoing target','encoded_call_target' in text and str(symbols['target']) in text)
  d=event(d['state'],'investigation_graph_node:'+str(knowledge_node['id']));check('native node view retains derived known bits','X18 <- encoded integer 0x1234'in'\n'.join(collect_fields(d['ui'])))
  d=event(d['state'],'investigation_graph_node:'+str(join_node['id']));check('native node query retains the common-entry successor result','Captured-entry interpretation'in'\n'.join(collect_fields(d['ui']))and'X7 <- encoded integer 0x2'in'\n'.join(collect_fields(d['ui'])))
  d=event(d['state'],'investigation_graph_node:'+str(arithmetic_node['id']));check('UI description retains partial-arithmetic derived constant','X26 <- encoded integer 0x12340000'in'\n'.join(collect_fields(d['ui'])))
  d=event(d['state'],'investigation_graph_node:'+str(selection_result['id']));check('UI description retains conditional-selection derived constant','X23 <- encoded integer 0x2a'in'\n'.join(collect_fields(d['ui'])))
  d=event(d['state'],'investigation_graph_node:'+str(auth_rows['braa']['source']));text='\n'.join(collect_fields(d['ui']));check('native authenticated node exposes encoded key and modifier','encoded_authenticated_indirect_branch'in text and'FEAT_PAuth'in text and'authentication_key'in text)
  d=event(d['state'],'investigation_graph_node:'+str(path_node['id']));text='\n'.join(collect_fields(d['ui']));check('native encoding view includes conditional facts','encoded integer 0x2a' in text and 'Other incoming entries remain unknown' in text);check('native node view retains captured-entry common bits','Captured-entry join'in text and'known mask0xffffffffffffffb5'in text)
  # Controlled provider text exercises the ordinary model query path; the
  # returned data is the real retained observation, not a provider invention.
  st=copy.deepcopy(d['state']);st['profiles']=[{'name':'Controlled','endpoint':'http://model.test/v1/chat/completions','protocol':'chat','model':'controlled-model','plain':True,'stream':False}]
  d=event(st,'send',{'draft':'Query the saved branch observation.'});assert d['effect']['purpose']=='model',d['state'].get('notice')
  def reply(d,text):
   body=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode();raw=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
   return event(d['state'],'network_result',{'purpose':'model','model_ticket':d['state']['model_expected'],'data':base64.b64encode(raw).decode()})
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':call['source']}}));check('ordinary model query uses existing review',d['state']['dialog']=='approval')
  d=event(d['state'],'approve');check('ordinary model receives actual typed relationship','encoded_call_target' in d['effect']['wire'] and str(symbols['target']) in d['effect']['wire']);check('ordinary model receives actual typed declarations','binary_declarations' in d['effect']['wire'] and 'CALL26' in d['effect']['wire'])
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':path_node['id']}}));d=event(d['state'],'approve');check('ordinary model query receives captured-entry common facts','Captured-entry join'in str(d['effect'])and'known mask0xffffffffffffffb5'in str(d['effect']));check('ordinary model query receives actual predecessor facts','encoded integer 0x2a' in d['effect']['wire'] and 'Other incoming entries remain unknown' in d['effect']['wire'])
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':auth_rows['blraa']['source']}}));d=event(d['state'],'approve');check('ordinary model query receives actual authenticated call operands','encoded_authenticated_indirect_call'in str(d['effect'])and'FEAT_PAuth'in str(d['effect'])and'authentication_key'in str(d['effect']));
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':knowledge_node['id']}}));d=event(d['state'],'approve');check('ordinary model query receives actual known-bit result','X18 <- encoded integer 0x1234'in str(d['effect']));
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':join_node['id']}}));d=event(d['state'],'approve');check('ordinary model query receives common-entry successor facts','Captured-entry interpretation'in d['effect']['wire']and'X7 <- encoded integer 0x2'in d['effect']['wire'])
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':arithmetic_node['id']}}));d=event(d['state'],'approve');check('ordinary model query receives partial-arithmetic findings','X26 <- encoded integer 0x12340000'in d['effect']['wire'])
  d=reply(d,json.dumps({'tool':'observations','args':{'id':record['id'],'snapshot':0,'node':selection_result['id']}}));d=event(d['state'],'approve');check('ordinary model query receives conditional-selection findings','X23 <- encoded integer 0x2a'in d['effect']['wire'])
  d=reply(d,'Controlled continuation after actual retained observation.');check('model query settles through existing continuation',not d['state']['busy'])
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:0')
  d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',{'record':record['id'],'snapshot':'0','node':'0','mode':'specification','output':'structure.json'});check('structural export enters review',d['state']['dialog']=='approval')
  d=event(d['state'],'approve');e=d['effect'];check('structural export shares existing authority',e['tool']=='reconstruct' and e['args']['path']=='original.elf')
  out=op(dict(e,op='tool',root=str(root)));export=json.loads((root/'structure.json').read_text());export_flow=export['source']['structure']['control_flow'];check('actual structural export retains typed relationships',export_flow['relations']==sorted(rows,key=lambda r:json.dumps(r,sort_keys=True,separators=(',',':'))))
  check('actual export retains typed declaration inventory',export['source']['structure']['declarations']==declarations)
  check('export labels semantics unresolved','Unresolved' in export['content'] and 'read back exactly' in out)
  d=event(d['state'],'tool_result',dict({k:e[k] for k in ['tool_ticket','tool_binding','editor','editor_dialog'] if k in e},purpose='local_tool',output=out))
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(path_node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',{'record':record['id'],'snapshot':'0','node':str(path_node['id']),'mode':'representation','output':'conditional.txt'});d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('actual conditional representation exports exact authenticated bytes',(root/'conditional.txt').read_bytes()==path_text.encode() and 'read back exactly' in out)
  # Settle the actual export callback before attempting another native event.
  i={k:e[k] for k in ['tool_ticket','tool_binding','editor','editor_dialog'] if k in e};i.update(purpose='local_tool',output=out);d=event(d['state'],'tool_result',i)
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(join_node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',{'record':record['id'],'snapshot':'0','node':str(join_node['id']),'mode':'representation','output':'common-entry.txt'});d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('common-entry representation exports its exact retained bytes',(root/'common-entry.txt').read_bytes()==join_text.encode()and'read back exactly'in out);d=event(d['state'],'tool_result',dict({k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e},purpose='local_tool',output=out))
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(selection_result['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',{'record':record['id'],'snapshot':'0','node':str(selection_result['id']),'mode':'representation','output':'selected-value.txt'});d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('conditional-selection representation exports exact retained bytes',(root/'selected-value.txt').read_bytes()==selection_text.encode()and'read back exactly'in out);d=event(d['state'],'tool_result',dict({k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e},purpose='local_tool',output=out))
  # Independently linked pure Arm64 PE verifies that typed targets remain
  # relative RVAs, without adding its declared image base as a runtime pointer.
  coff=root/'original-coff.S';coff.write_text(asm.read_text().replace('.type entry,%function\n','').replace('.size entry,.-entry\n',''))
  obj=root/'original.obj';pe=root/'original.exe';env=dict(os.environ,TMPDIR=str(root))
  subprocess.run([str(ndk/'clang'),'--target=aarch64-pc-windows-msvc','-march=armv8.8-a','-c',str(coff),'-o',str(obj)],check=True,capture_output=True,env=env)
  subprocess.run([str(ndk/'lld-link'),'/entry:entry','/subsystem:console','/nodefaultlib','/machine:arm64','/out:'+str(pe),str(obj)],check=True,capture_output=True,env=env)
  headers=subprocess.check_output([str(ndk/'llvm-readobj'),'--file-headers',str(pe)],text=True);base=int(re.search(r'ImageBase: (0x[0-9A-Fa-f]+)',headers)[1],16)
  reference=subprocess.check_output([str(ndk/'llvm-objdump'),'-d',str(pe)],text=True);jump=re.search(r'^\s*([0-9A-Fa-f]+):.*?\bbl\s+0x([0-9A-Fa-f]+)',reference,re.M);assert jump,reference
  d=event(d['state'],'investigation');d=event(d['state'],'inspect_file',{'path':'original.exe'});d=event(d['state'],'approve');e=d['effect'];actual=op(dict(e,op='tool',root=str(root)));g=json.loads(actual)['snapshots'][0];peflow=g['control_flow'];pecall=next(r for r in peflow['relations'] if r['relation']=='encoded_call_target')
  check('real pure Arm64 PE retains relative coordinates',peflow['coordinate_kind']=='PE RVA' and pecall['target_address']==int(jump[2],16)-base and pecall['target_address']<base)
  check('PE call source matches independent disassembly',g['nodes'][pecall['source']]['encoding']['address']==int(jump[1],16)-base)
  i={k:e[k] for k in ['tool_ticket','tool_binding','editor','editor_dialog'] if k in e};i.update(purpose='local_tool',output=actual);d=event(d['state'],'tool_result',i);check('PE actual callback retains typed flow',d['state']['investigation_records']['items'][-1]['snapshots'][0]['control_flow']==peflow);check('PE classification stays unknown and declared relocation absence is explicit',g['declarations']['classification_inventory_complete'] is None and g['declarations']['relocation_inventory_complete'] is True and g['declarations']['relocations']['observed']==0 and g['declarations']['mappings']['retained']>0)
  tampered=copy.deepcopy(d['state']);bad=next(r for r in tampered['investigation_records']['items'] if r['id']==record['id']);bad['snapshots'][0]['control_flow']['relations'][0]['target_address']+=4
  rejected=event(tampered,'investigation_record:'+record['id']);rejected=event(rejected['state'],'investigation_graph');check('changed flow payload never becomes accepted native data',rejected['state']['notice_error'] and 'Malformed binary observation' in rejected['state']['notice'] and rejected['state']['dialog']!='investigation_graph')
  check('file engine still operates independently of historical payload',isinstance(op({'op':'tool','tool':'inspect','root':str(root),'args':{'path':'original.S'}}),str))
  old=subprocess.Popen([str(args.previous_flow_controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  def old_op(v):
   old.stdin.write(json.dumps(v)+'\n');old.stdin.flush();answer=json.loads(old.stdout.readline());assert'value'in answer,answer;return answer['value']
  def old_event(st,a,i=None):return old_op(dict(op='event',state=st,action=a,inputs=i or{},root=str(root)))
  try:
   legacy=old_event(None,'resume');legacy=old_event(legacy['state'],'investigation');legacy=old_event(legacy['state'],'inspect_file',{'path':'original.elf'});legacy=old_event(legacy['state'],'approve');e=legacy['effect'];data=old_op(dict(e,op='tool',root=str(root)));g=json.loads(data)['snapshots'][0];check('actual previous controller retains its original decoder meaning','decoder_version'not in g and next(n for n in g['nodes'][1:]if n['encoding']['address']==symbols['consistent_edge'])['encoding']['mnemonic']=='unclassified');i={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};i.update(purpose='local_tool',output=data);legacy=old_event(legacy['state'],'tool_result',i);old_record=legacy['state']['investigation_records']['items'][-1]
  finally:old.stdin.close();old.wait(timeout=10)
  state=copy.deepcopy(d['state']);state['investigation_records']['items'].append(old_record);legacy=event(state,'investigation_record:'+old_record['id']);legacy=event(legacy['state'],'investigation_graph');check('current native graph accepts actual legacy observation bytes',legacy['state']['dialog']=='investigation_graph'and not legacy['state'].get('notice_error'));old_node=next(n for n in old_record['snapshots'][0]['nodes'][1:]if n['encoding']['address']==symbols['consistent_edge']);legacy=event(legacy['state'],'investigation_graph_node:'+str(old_node['id']));check('legacy unclassified instruction remains historical evidence','unclassified'in'\n'.join(collect_fields(legacy['ui'])))

  # Actual immediately published code produces these bytes; the current
  # command flow reads, queries and exports that retained representation.
  old=subprocess.Popen([str(args.published_controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:
   legacy=old_event(None,'resume');legacy=old_event(legacy['state'],'investigation');legacy=old_event(legacy['state'],'inspect_file',{'path':'original.elf'});legacy=old_event(legacy['state'],'approve');e=legacy['effect'];data=old_op(dict(e,op='tool',root=str(root)));i={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};i.update(purpose='local_tool',output=data);legacy=old_event(legacy['state'],'tool_result',i);published_record=legacy['state']['investigation_records']['items'][-1];bucket=copy.deepcopy(legacy['state']['investigation_records'])
  finally:old.stdin.close();old.wait(timeout=10)
  st=event(None,'resume')['state'];st['investigation_records']=bucket;legacy=event(st,'investigation_record:'+published_record['id']);legacy=event(legacy['state'],'investigation_graph');saved_node=next(n for n in published_record['snapshots'][0]['nodes'][1:]if n['encoding']['address']==symbols['arithmetic_bits_mask']);legacy=event(legacy['state'],'investigation_graph_node:'+str(saved_node['id']));check('current native view preserves actual published representation',not legacy['state'].get('notice_error')and saved_node['content'].strip()in'\n'.join(collect_fields(legacy['ui'])))
  st=legacy['state'];st['profiles']=[{'name':'Controlled','endpoint':'http://model.test/v1/chat/completions','protocol':'chat','model':'controlled-model','plain':True,'stream':False}];legacy=event(st,'send',{'draft':'Read the actual saved published representation.'});legacy=reply(legacy,json.dumps({'tool':'observations','args':{'id':published_record['id'],'snapshot':0,'node':saved_node['id']}}));legacy=event(legacy['state'],'approve');check('current model query preserves actual published representation',legacy['effect']is not None and json.dumps(saved_node['content'],ensure_ascii=False)[1:-1]in json.loads(legacy['effect']['wire'].split('\r\n\r\n',1)[1])['messages'][-1]['content']);legacy=reply(legacy,'Saved published representation received.')
  legacy=event(legacy['state'],'investigation_record:'+published_record['id']);legacy=event(legacy['state'],'investigation_graph');legacy=event(legacy['state'],'investigation_graph_node:'+str(saved_node['id']));legacy=event(legacy['state'],'reconstruction_new');legacy=event(legacy['state'],'reconstruction_prepare',{'record':published_record['id'],'snapshot':'0','node':str(saved_node['id']),'mode':'representation','output':'published-value.txt'});legacy=event(legacy['state'],'approve');e=legacy['effect'];out=op(dict(e,op='tool',root=str(root)));check('current export preserves exact actual published representation',(root/'published-value.txt').read_bytes()==saved_node['content'].encode()and'read back exactly'in out)

  if retained_auth:
   passed.extend(retained_auth['checks'])
  else:
   old=subprocess.Popen([str(args.previous_auth_controller.resolve()),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
   def old_op(v):
    old.stdin.write(json.dumps(v)+'\n');old.stdin.flush();answer=json.loads(old.stdout.readline());assert'value'in answer,answer;return answer['value']
   def old_event(st,a,i=None):return old_op(dict(op='event',state=st,action=a,inputs=i or{},root=str(root)))
   try:
    legacy=old_event(None,'resume');legacy=old_event(legacy['state'],'investigation');legacy=old_event(legacy['state'],'inspect_file',{'path':'original.elf'});legacy=old_event(legacy['state'],'approve');e=legacy['effect'];data=old_op(dict(e,op='tool',root=str(root)));g=json.loads(data)['snapshots'][0];check('actual revision-two controller retains original authenticated-word meaning',g.get('decoder_version')==2 and next(n for n in g['nodes'][1:]if n['encoding']['address']==symbols['auth_braa'])['encoding']['mnemonic']=='unclassified');i={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};i.update(purpose='local_tool',output=data);legacy=old_event(legacy['state'],'tool_result',i);old_record=legacy['state']['investigation_records']['items'][-1]
   finally:old.stdin.close();old.wait(timeout=10)
   state=copy.deepcopy(d['state']);state['investigation_records']['items'].append(old_record);legacy=event(state,'investigation_record:'+old_record['id']);legacy=event(legacy['state'],'investigation_graph');check('current native graph accepts actual revision-two bytes',legacy['state']['dialog']=='investigation_graph'and not legacy['state'].get('notice_error'));old_node=next(n for n in old_record['snapshots'][0]['nodes'][1:]if n['encoding']['address']==symbols['auth_braa']);legacy=event(legacy['state'],'investigation_graph_node:'+str(old_node['id']));check('revision-two authenticated word remains historical evidence','unclassified'in'\n'.join(collect_fields(legacy['ui'])))

 finally:p.stdin.close();p.wait(timeout=10)
print(json.dumps({'passed':len(passed),'checks':passed,'scope':'Owner connected checks: original compiled binaries, LLVM comparisons, inspection, retained queries, native views and exact exports.'},indent=2))
(PROJECT/'output/checks/binary-flow.json').write_text(json.dumps({'passed':passed,'executed_checks':len(passed)-(3 if retained_auth else 0),'retained_compatibility':retained_auth},indent=2)+'\n')
