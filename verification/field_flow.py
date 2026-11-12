"""Assembled field/extraction values through inspection, query, UI and exact export."""
import base64,copy,json,os,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
def strings(v):
 if isinstance(v,str):yield v
 elif isinstance(v,dict):
  for x in v.values():yield from strings(x)
 elif isinstance(v,list):
  for x in v:yield from strings(x)
with tempfile.TemporaryDirectory(prefix='field-flow-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);asm=root/'fields.S';elf=root/'fields.elf';ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
 asm.write_text('''.text
.global entry
entry:
unsigned_field: ubfm x1,x0,#8,#15
unsigned_zero: and x2,x1,#0xff00
 mov x5,#0x5500
merged_field: bfm x5,x0,#0,#7
merged_constant: lsr x6,x5,#8
 mov x7,#0xaa
inserted_field: bfm x8,x7,#0,#7
inserted_constant: and x9,x8,#0xff
 and x10,x0,#0x7f
positive_field: sbfm x11,x10,#0,#7
positive_zero: lsr x12,x11,#7
 orr x13,x0,#0x80
negative_field: sbfm x14,x13,#0,#7
negative_constant: lsr x15,x14,#7
signed_wrap: sbfm x16,x0,#60,#3
signed_wrap_zero: and x17,x16,#0xf
unsigned_wrap: ubfm x18,x0,#60,#3
unsigned_wrap_zero: and x19,x18,#0xffffffffffffff0f
 mov x20,#0
extract_low: extr x21,x0,x20,#8
extract_low_zero: and x22,x21,#0x00ffffffffffffff
extract_high: extr x23,x20,x0,#8
extract_high_zero: lsr x24,x23,#56
 and x25,x0,#0xff
extract_rotation: extr x26,x25,x25,#8
rotation_zero: and x27,x26,#0x00ffffffffffffff
 ret
word_entry:
word_field: ubfm w1,w0,#4,#7
word_zero: and x2,x1,#0xfffffffffffffff0
 mov w3,#0x5500
word_merge: bfm w3,w0,#0,#7
word_constant: lsr x4,x3,#8
word_extract: extr w5,w0,wzr,#8
word_extract_zero: and x6,x5,#0x00ffffff
 ret
coordinate_entry:
 adr x10,entry
coordinate_unsigned: ubfm x11,x10,#0,#63
coordinate_signed: sbfm x12,x10,#0,#63
coordinate_merge: bfm x13,x10,#0,#63
coordinate_alias: bfm x10,x10,#0,#7
coordinate_extract: extr x14,xzr,x10,#0
 ret
''')
 subprocess.run([str(ndk/'clang'),'--target=aarch64-linux-android26','-nostdlib','-static','-fuse-ld=lld','-Wl,-e,entry','-Wl,--emit-relocs',str(asm),'-o',str(elf)],check=True,capture_output=True,env=dict(os.environ,TMPDIR=str(root)))
 nm=subprocess.check_output([str(ndk/'llvm-nm'),'-n',str(elf)],text=True);symbols={m[3]:int(m[1],16)for line in nm.splitlines()if(m:=re.match(r'([0-9a-fA-F]+)\s+(\w)\s+(\S+)$',line))};disassembly=subprocess.check_output([str(ndk/'llvm-objdump'),'-d',str(elf)],text=True)
 p=subprocess.Popen([str(PROJECT/'output/headless/RLLM'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
 def operation(process,value):process.stdin.write(json.dumps(value)+'\n');process.stdin.flush();r=json.loads(process.stdout.readline());assert'value'in r,r;return r['value']
 def op(value):return operation(p,value)
 def event(st,action,inputs=None):return op(dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
 def inspect(process):
  def event(st,action,inputs=None):return operation(process,dict(op='event',root=str(root),state=st,action=action,inputs=inputs or{}))
  d=event(None,'resume');d=event(d['state'],'investigation');d=event(d['state'],'inspect_file',{'path':elf.name});d=event(d['state'],'approve');e=d['effect'];output=operation(process,dict(e,op='tool',root=str(root)));inputs={k:e[k]for k in ['tool_ticket','tool_binding','editor','editor_dialog']if k in e};inputs.update(purpose='local_tool',output=output);return event(d['state'],'tool_result',inputs),json.loads(output)['snapshots'][0]
 def reply(d,text):
  b=json.dumps({'choices':[{'message':{'content':text},'finish_reason':'stop'}]}).encode();wire=b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(b)).encode()+b'\r\n\r\n'+b;return event(d['state'],'network_result',dict(purpose='model',model_ticket=d['state']['model_expected'],data=base64.b64encode(wire).decode()))
 try:
  d,graph=inspect(p);nodes={n['encoding']['address']:n for n in graph['nodes'][1:]};check('independent assembler emits field and extraction instructions','ubfx'in disassembly and'bfxil'in disassembly and'extr'in disassembly)
  for name,register,bits,known in [('unsigned_field','X1',0,0xffffffffffffff00),('merged_field','X5',0x5500,0xffffffffffffff00),('inserted_field','X8',0xaa,0xff),('positive_field','X11',0,0xffffffffffffff80),('negative_field','X14',0xffffffffffffff80,0xffffffffffffff80),('signed_wrap','X16',0,0xf),('unsigned_wrap','X18',0,0xffffffffffffff0f),('extract_low','X21',0,0x00ffffffffffffff),('extract_high','X23',0,0xff00000000000000),('extract_rotation','X26',0,0x00ffffffffffffff),('word_field','W1',0,0xfffffffffffffff0),('word_merge','W3',0x5500,0xffffffffffffff00),('word_extract','W5',0,0xffffffff00ffffff)]:
   check(name+' retains the selected known bits',register+' <- partial integer bits0x'+format(bits,'x')+', known mask0x'+format(known,'x')in nodes[symbols[name]]['content'])
  for name,register,value in [('unsigned_zero','X2',0),('merged_constant','X6',0x55),('inserted_constant','X9',0xaa),('positive_zero','X12',0),('negative_constant','X15',0x1ffffffffffffff),('signed_wrap_zero','X17',0),('unsigned_wrap_zero','X19',0),('extract_low_zero','X22',0),('extract_high_zero','X24',0),('rotation_zero','X27',0),('word_zero','X2',0),('word_constant','X4',0x55),('word_extract_zero','X6',0)]:check(name+' derives its constant from the preceding field',register+' <- encoded integer 0x'+format(value,'x')in nodes[symbols[name]]['content'])
  for name in ['coordinate_unsigned','coordinate_signed','coordinate_merge','coordinate_alias','coordinate_extract']:
   check(name+' preserves source-coordinate identity','encoded source coordinate declared VA 'in nodes[symbols[name]]['content'])
  record=d['state']['investigation_records']['items'][-1];node=nodes[symbols['merged_field']];d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));check('native view retains actual partial field value','X5 <- partial integer bits0x5500, known mask0xffffffffffffff00'in'\n'.join(strings(d['ui'])))
  st=d['state'];st['profiles']=[dict(name='Controlled',model='controlled',endpoint='http://controlled.test/model',plain=True,stream=False,protocol='chat')];d=event(st,'send',{'draft':'Read the retained field value.'});d=reply(d,json.dumps(dict(tool='observations',args=dict(id=record['id'],snapshot=0,node=node['id']))));d=event(d['state'],'approve');check('ordinary model query receives the same partial field value',d['effect']is not None and'X5 <- partial integer bits0x5500, known mask0xffffffffffffff00'in d['effect']['wire']);d=reply(d,'Field value received.')
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node=str(node['id']),mode='representation',output='field-result.txt'));d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('reviewed export retains the exact field representation',(root/'field-result.txt').read_bytes()==node['content'].encode()and'read back exactly'in out)
  old=subprocess.Popen([str(PROJECT/'output/published-controller'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:prior,prior_graph=inspect(old)
  finally:old.stdin.close();old.wait(timeout=10)
  prior_node=next(n for n in prior_graph['nodes'][1:]if n['encoding']['address']==symbols['merged_field']);check('actual published capture retains its encoded field facts','X5 <- partial integer bits0x5500, known mask0xffffffffffffff00'in prior_node['content']);st=event(None,'resume')['state'];st['investigation_records']=copy.deepcopy(prior['state']['investigation_records']);prior_record=st['investigation_records']['items'][-1];d=event(st,'investigation_record:'+prior_record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(prior_node['id']));check('current view preserves the actual prior field capture',prior_node['content'].strip()in'\n'.join(strings(d['ui'])))
 finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner current shared-core checks with original LLVM-assembled AArch64 field/extraction instructions, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility.');(PROJECT/'output/checks/field-flow.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
