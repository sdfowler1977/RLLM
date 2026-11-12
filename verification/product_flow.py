"""Product identities and known bits through assembled flow, queries, UI and exact export."""
import base64,copy,json,os,pathlib,re,subprocess,tempfile
PROJECT=pathlib.Path(__file__).resolve().parents[1];passed=[]
def check(name,value):assert value,name;passed.append(name)
def strings(v):
 if isinstance(v,str):yield v
 elif isinstance(v,dict):
  for x in v.values():yield from strings(x)
 elif isinstance(v,list):
  for x in v:yield from strings(x)
with tempfile.TemporaryDirectory(prefix='product-flow-',dir=PROJECT/'output')as tmp:
 root=pathlib.Path(tmp);asm=root/'product.S';elf=root/'product.elf';ndk=pathlib.Path(os.environ['CLIENT_NDK'])/'toolchains/llvm/prebuilt/linux-x86_64/bin'
 asm.write_text('''.text
.global entry
entry:
zero_product: madd x1,x0,xzr,xzr
 mov x2,#42
zero_addend: madd x3,x0,xzr,x2
zero_subtract: msub x4,x0,xzr,x2
 movk x0,#0x1234
 mov x6,#1
unit_product: madd x7,x0,x6,xzr
same_register_cancel: msub x8,x0,x6,x0
 mov x9,#2
shift_product: madd x10,x0,x9,xzr
commuted_product: madd x11,x9,x0,xzr
 movk x12,#0xabcd
low_product: madd x13,x0,x12,xzr
masked_product: and x14,x13,#0xffff
 and x15,x0,#0xffffffff00000000
forced_zero: madd x16,x15,x15,xzr
forced_accumulator: madd x17,x15,x15,x2
 mov x18,#3
 mov x19,#2
 movk x20,#0x1000
partial_addend: madd x21,x18,x19,x20
partial_subtract: msub x22,x18,x19,x20
masked_addend: and x23,x21,#0xffff
 ands xzr,xzr,xzr
flag_product: madd x24,x0,xzr,x2
preserved_flags: csel x25,x2,xzr,eq
 ret
word_entry:
 mov w2,#42
word_zero: madd w1,w0,wzr,wzr
word_accumulator: msub w3,w0,wzr,w2
 movk w0,#0x1234
 mov w6,#2
word_shift: madd w7,w0,w6,wzr
word_mask: and x8,x7,#0xffff
 ret
coordinate_entry:
 adr x1,entry
 mov x2,#1
coordinate_unit: madd x3,x1,x2,xzr
coordinate_accumulator: madd x4,x0,xzr,x1
coordinate_cancel: msub x5,x1,x2,x1
 mov x6,#2
coordinate_shift: madd x7,x1,x6,xzr
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
  d,graph=inspect(p);nodes={n['encoding']['address']:n for n in graph['nodes'][1:]};check('independent assembler retains actual product instruction encodings','madd'in disassembly and'msub'in disassembly and'mul'in disassembly)
  for name,register,value in [('zero_product','X1',0),('zero_addend','X3',42),('zero_subtract','X4',42),('same_register_cancel','X8',0),('masked_product','X14',0x1234*0xabcd&0xffff),('forced_zero','X16',0),('forced_accumulator','X17',42),('masked_addend','X23',0x1006),('flag_product','X24',42),('preserved_flags','X25',42),('word_zero','W1',0),('word_accumulator','W3',42),('word_mask','X8',0x2468),('coordinate_cancel','X5',0)]:
   check(name+' retains its determined product or accumulator',register+' <- encoded integer 0x'+format(value,'x')in nodes[symbols[name]]['content'])
  for name,register,bits,known in [('unit_product','X7',0x1234,0xffff),('shift_product','X10',0x2468,0x1ffff),('commuted_product','X11',0x2468,0x1ffff),('low_product','X13',0x1234*0xabcd&0xffff,0xffff),('partial_addend','X21',0x1006,0xffff),('partial_subtract','X22',0xffa,0xffff),('word_shift','W7',0x2468,0xffffffff0001ffff),('coordinate_shift','X7',0,1)]:check(name+' retains each proved product bit',register+' <- partial integer bits0x'+format(bits,'x')+', known mask0x'+format(known,'x')in nodes[symbols[name]]['content'])
  for name in ['coordinate_unit','coordinate_accumulator']:check(name+' preserves source-coordinate identity','encoded source coordinate declared VA 'in nodes[symbols[name]]['content'])
  record=d['state']['investigation_records']['items'][-1];node=nodes[symbols['shift_product']];d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));check('native view retains actual partial product bits','X10 <- partial integer bits0x2468, known mask0x1ffff'in'\n'.join(strings(d['ui'])))
  st=d['state'];st['profiles']=[dict(name='Controlled',model='controlled',endpoint='http://controlled.test/model',plain=True,stream=False,protocol='chat')];d=event(st,'send',{'draft':'Read the retained product bits.'});d=reply(d,json.dumps(dict(tool='observations',args=dict(id=record['id'],snapshot=0,node=node['id']))));d=event(d['state'],'approve');check('ordinary model query receives the actual product bits',d['effect']is not None and'X10 <- partial integer bits0x2468, known mask0x1ffff'in d['effect']['wire']);d=reply(d,'Product bits received.')
  d=event(d['state'],'investigation_record:'+record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(node['id']));d=event(d['state'],'reconstruction_new');d=event(d['state'],'reconstruction_prepare',dict(record=record['id'],snapshot='0',node=str(node['id']),mode='representation',output='product-result.txt'));d=event(d['state'],'approve');e=d['effect'];out=op(dict(e,op='tool',root=str(root)));check('reviewed export retains the exact product representation',(root/'product-result.txt').read_bytes()==node['content'].encode()and'read back exactly'in out)
  old=subprocess.Popen([str(PROJECT/'output/published-controller'),'--jsonl'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:prior,prior_graph=inspect(old)
  finally:old.stdin.close();old.wait(timeout=10)
  prior_node=next(n for n in prior_graph['nodes'][1:]if n['encoding']['address']==symbols['shift_product']);check('actual published capture retains the same product encoding',prior_node['encoding']['bits']==node['encoding']['bits']and prior_node['encoding']['mnemonic']=='MADD');st=event(None,'resume')['state'];st['investigation_records']=copy.deepcopy(prior['state']['investigation_records']);prior_record=st['investigation_records']['items'][-1];d=event(st,'investigation_record:'+prior_record['id']);d=event(d['state'],'investigation_graph');d=event(d['state'],'investigation_graph_node:'+str(prior_node['id']));check('current view preserves the actual prior product capture',prior_node['content'].strip()in'\n'.join(strings(d['ui'])))
 finally:p.stdin.close();p.wait(timeout=10)
result=dict(passed=passed,scope='Owner current shared-core checks with original LLVM-assembled AArch64 product instruction encodings, independent disassembly, retained observations, native UI descriptions, controlled provider query, exact export and genuine immediately-published core capture compatibility.');(PROJECT/'output/checks/product-flow.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(result,count=len(passed)),indent=2))
