//! Original bounded DEX code-unit/reference observer. Never executes bytecode.
//! Encoding and target observations are not a VM verifier or a reachable CFG.
pub(super) struct Tables<'a>{pub strings:usize,pub types:usize,pub fields:usize,pub method_words:&'a[usize],pub proto_words:&'a[usize]}
pub(super) struct Signatures<'a>{pub methods:&'a[(usize,usize)],pub protos:&'a[(usize,Vec<u16>)],pub wide:&'a dyn Fn(usize)->bool}
pub(super) struct Site{pub handle:usize,pub name:usize,pub proto:usize}
pub(super) struct Handle{pub kind:u16,pub index:usize}
pub(super) struct Ref {pub pc:usize,pub kind:&'static str,pub index:usize,pub invocation:bool}
pub(super) struct Code {pub instructions:usize,pub payloads:usize,pub references:usize,pub invocations:usize,pub branches:usize,pub rows:Vec<String>,pub refs:Vec<Ref>,pub abbreviated:bool,boundaries:Vec<u8>}
type Result<T>=std::result::Result<T,String>;
fn charge(work:&mut usize,n:usize)->Result<()>{*work=work.checked_sub(n).ok_or("DEX shared instruction work bound exhausted")?;Ok(())}
fn target(pc:usize,delta:i32,n:usize)->Result<usize>{let t=pc as i64+delta as i64;if t<0||t>=n as i64{return Err(format!("DEX branch/payload from unit {pc} is outside code units"));}Ok(t as usize)}
fn reg(v:usize,wide:bool,registers:usize)->Result<()>{if v>=registers||wide&&v.checked_add(1).map(|n|n>=registers).unwrap_or(true){return Err(format!("DEX register v{v}{} exceeds frame of {registers}",if wide{" wide pair"}else{""}));}Ok(())}
// Only declared parameter widths and encoded register identities are checked.
// No register value type, assignability, receiver class or execution is inferred.
fn arguments(op:u8,w:&[u16],pc:usize,count:usize,proto:usize,receiver:bool,sig:&Signatures<'_>,work:&mut usize,cancelled:&dyn Fn()->bool)->Result<()>{
 let args=&sig.protos.get(proto).ok_or("DEX invocation signature context lacks prototype")?.1;
 let range=matches!(op,0x74..=0x78|0xfb|0xfd);let b=w[pc+2];let list=[(b&15)as usize,((b>>4)&15)as usize,((b>>8)&15)as usize,(b>>12)as usize,((w[pc]>>8)&15)as usize];
 let register=|at:usize|->Result<usize>{if at>=count{return Err("DEX invocation parameter width exceeds encoded argument words".into());}if range{Ok(b as usize+at)}else{list.get(at).copied().ok_or_else(||"DEX invocation argument list is truncated".into())}};
 let mut at=usize::from(receiver);
 for &ty in args{if cancelled(){return Err("DEX instruction observation cancelled".into());}charge(work,1)?;let low=register(at)?;if (sig.wide)(ty as usize){let high=register(at+1)?;if low.checked_add(1)!=Some(high){return Err(format!("DEX invocation wide argument at unit {pc} uses nonconsecutive registers v{low}, v{high}"));}at+=2;}else{at+=1;}}
 if at!=count{return Err("DEX invocation parameter widths disagree with encoded argument words".into());}Ok(())
}
fn width(op:u8)->Option<usize>{Some(match op{
 0x00|0x01|0x04|0x07|0x0a..=0x12|0x1d|0x1e|0x21|0x27|0x28|0x7b..=0x8f|0xb0..=0xcf=>1,
 0x02|0x05|0x08|0x13|0x15|0x16|0x19|0x1a|0x1c|0x1f|0x20|0x22|0x23|0x29|0x2d..=0x3d|0x44..=0x6d|0x90..=0xaf|0xd0..=0xe2|0xfe|0xff=>2,
 0x03|0x06|0x09|0x14|0x17|0x1b|0x24..=0x26|0x2a..=0x2c|0x6e..=0x72|0x74..=0x78|0xfc|0xfd=>3,
 0xfa|0xfb=>4,0x18=>5,_=>return None})}
// Format mnemonics preserve the encoded operation and variant, not inferred types.
fn name(op:u8)->&'static str{match op{
 0x00=>"nop",0x01=>"move",0x02=>"move/from16",0x03=>"move/16",0x04=>"move-wide",0x05=>"move-wide/from16",0x06=>"move-wide/16",0x07=>"move-object",0x08=>"move-object/from16",0x09=>"move-object/16",
 0x0a=>"move-result",0x0b=>"move-result-wide",0x0c=>"move-result-object",0x0d=>"move-exception",0x0e=>"return-void",0x0f=>"return",0x10=>"return-wide",0x11=>"return-object",
 0x12=>"const/4",0x13=>"const/16",0x14=>"const",0x15=>"const/high16",0x16=>"const-wide/16",0x17=>"const-wide/32",0x18=>"const-wide",0x19=>"const-wide/high16",0x1a=>"const-string",0x1b=>"const-string/jumbo",0x1c=>"const-class",0x1d=>"monitor-enter",0x1e=>"monitor-exit",0x1f=>"check-cast",0x20=>"instance-of",0x21=>"array-length",0x22=>"new-instance",0x23=>"new-array",0x24=>"filled-new-array",0x25=>"filled-new-array/range",0x26=>"fill-array-data",0x27=>"throw",0x28=>"goto",0x29=>"goto/16",0x2a=>"goto/32",0x2b=>"packed-switch",0x2c=>"sparse-switch",
 0x2d..=0x31=>["cmpl-float","cmpg-float","cmpl-double","cmpg-double","cmp-long"][(op-0x2d)as usize],
 0x32..=0x3d=>["if-eq","if-ne","if-lt","if-ge","if-gt","if-le","if-eqz","if-nez","if-ltz","if-gez","if-gtz","if-lez"][(op-0x32)as usize],
 0x44..=0x51=>["aget","aget-wide","aget-object","aget-boolean","aget-byte","aget-char","aget-short","aput","aput-wide","aput-object","aput-boolean","aput-byte","aput-char","aput-short"][(op-0x44)as usize],
 0x52..=0x5f=>["iget","iget-wide","iget-object","iget-boolean","iget-byte","iget-char","iget-short","iput","iput-wide","iput-object","iput-boolean","iput-byte","iput-char","iput-short"][(op-0x52)as usize],
 0x60..=0x6d=>["sget","sget-wide","sget-object","sget-boolean","sget-byte","sget-char","sget-short","sput","sput-wide","sput-object","sput-boolean","sput-byte","sput-char","sput-short"][(op-0x60)as usize],
 0x6e..=0x72=>["invoke-virtual","invoke-super","invoke-direct","invoke-static","invoke-interface"][(op-0x6e)as usize],
 0x74..=0x78=>["invoke-virtual/range","invoke-super/range","invoke-direct/range","invoke-static/range","invoke-interface/range"][(op-0x74)as usize],
 0x7b..=0x8f=>["neg-int","not-int","neg-long","not-long","neg-float","neg-double","int-to-long","int-to-float","int-to-double","long-to-int","long-to-float","long-to-double","float-to-int","float-to-long","float-to-double","double-to-int","double-to-long","double-to-float","int-to-byte","int-to-char","int-to-short"][(op-0x7b)as usize],
 0x90..=0xaf=>["add-int","sub-int","mul-int","div-int","rem-int","and-int","or-int","xor-int","shl-int","shr-int","ushr-int","add-long","sub-long","mul-long","div-long","rem-long","and-long","or-long","xor-long","shl-long","shr-long","ushr-long","add-float","sub-float","mul-float","div-float","rem-float","add-double","sub-double","mul-double","div-double","rem-double"][(op-0x90)as usize],
 0xb0..=0xcf=>["add-int/2addr","sub-int/2addr","mul-int/2addr","div-int/2addr","rem-int/2addr","and-int/2addr","or-int/2addr","xor-int/2addr","shl-int/2addr","shr-int/2addr","ushr-int/2addr","add-long/2addr","sub-long/2addr","mul-long/2addr","div-long/2addr","rem-long/2addr","and-long/2addr","or-long/2addr","xor-long/2addr","shl-long/2addr","shr-long/2addr","ushr-long/2addr","add-float/2addr","sub-float/2addr","mul-float/2addr","div-float/2addr","rem-float/2addr","add-double/2addr","sub-double/2addr","mul-double/2addr","div-double/2addr","rem-double/2addr"][(op-0xb0)as usize],
 0xd0..=0xd7=>["add-int/lit16","rsub-int","mul-int/lit16","div-int/lit16","rem-int/lit16","and-int/lit16","or-int/lit16","xor-int/lit16"][(op-0xd0)as usize],
 0xd8..=0xe2=>["add-int/lit8","rsub-int/lit8","mul-int/lit8","div-int/lit8","rem-int/lit8","and-int/lit8","or-int/lit8","xor-int/lit8","shl-int/lit8","shr-int/lit8","ushr-int/lit8"][(op-0xd8)as usize],
 0xfa=>"invoke-polymorphic",0xfb=>"invoke-polymorphic/range",0xfc=>"invoke-custom",0xfd=>"invoke-custom/range",0xfe=>"const-method-handle",0xff=>"const-method-type",_=>"unsupported"}}
// Called only after width/frame/reference validation. Untyped constants retain bits.
fn operands(op:u8,w:&[u16],pc:usize)->String{
 let first=w[pc];let signed32=|at:usize|((w[at]as u32)|((w[at+1]as u32)<<16))as i32;
 let literal=match op{
  0x12=>Some((((first>>12)as i8)<<4>>4)as i64),
  0x13|0x16|0xd0..=0xd7=>Some(w[pc+1]as i16 as i64),
  0x14|0x17=>Some(signed32(pc+1)as i64),
  0x15=>Some(((w[pc+1]as u32)<<16)as i32 as i64),
  0x18=>{let mut bits=0u64;for i in 0..4{bits|=(w[pc+1+i]as u64)<<(i*16);}Some(bits as i64)},
  0x19=>Some(((w[pc+1]as u64)<<48)as i64),
  0xd8..=0xe2=>Some((w[pc+1]>>8)as u8 as i8 as i64),_=>None};
 if let Some(v)=literal{let wide=matches!(op,0x16..=0x19);return if wide{format!("; literal signed64 {v}, bits64 0x{:016x} (untyped)",v as u64)}else if matches!(op,0xd0..=0xe2){format!("; literal signed32 {v}, bits32 0x{:08x}",v as u32)}else{format!("; literal signed32 {v}, bits32 0x{:08x} (untyped)",v as u32)};}
 let delta=match op{0x28=>Some((first>>8)as u8 as i8 as i32),0x29|0x32..=0x3d=>Some(w[pc+1]as i16 as i32),0x26|0x2a..=0x2c=>Some(signed32(pc+1)),_=>None};
 if let Some(d)=delta{return format!("; signed displacement {d} code units -> unit {}",pc as i64+d as i64);}
 let mut text=String::new();
 if matches!(op,0x25|0x74..=0x78|0xfb|0xfd){let count=(first>>8)as usize;let start=w[pc+2]as usize;text=if count==0{"; argument words 0, range empty".into()}else{format!("; argument words {count}, range v{start}..v{}",start+count-1)};}
 else if matches!(op,0x24|0x6e..=0x72|0xfa|0xfc){text=format!("; argument words {}",first>>12);}
 if matches!(op,0xfa|0xfb){text.push_str(&format!("; prototype#{}",w[pc+3]));}text
}

impl Code{
 pub(super) fn is_instruction(&self,at:usize)->bool{self.boundaries.get(at)==Some(&1)}
 pub(super) fn is_boundary(&self,at:usize)->bool{at==self.boundaries.len()||self.boundaries.get(at).map(|b|*b!=0).unwrap_or(false)}
 fn row(&mut self,s:String){if self.rows.len()<24{self.rows.push(s);}else{self.abbreviated=true;}}
 fn reference(&mut self,pc:usize,kind:&'static str,id:usize,count:usize,invocation:bool)->Result<()>{if id>=count{return Err(format!("DEX {kind} reference#{id} at unit {pc} exceeds table"));}self.references+=1;if invocation{self.invocations+=1;}if self.refs.len()<64{self.refs.push(Ref{pc,kind,index:id,invocation});}else{self.abbreviated=true;}Ok(())}
}
/// Code offsets and all signed displacements are measured in 16-bit units.
pub(super) fn observe(bytes:&[u8],be:bool,version:u32,registers:usize,outs:usize,t:&Tables<'_>,work:&mut usize)->Result<Code>{
 observe_with_cancel(bytes,be,version,registers,outs,t,work,&||false)
}
pub(super) fn observe_with_cancel(bytes:&[u8],be:bool,version:u32,registers:usize,outs:usize,t:&Tables<'_>,work:&mut usize,cancelled:&dyn Fn()->bool)->Result<Code>{
 observe_handles_with_cancel(bytes,be,version,registers,outs,t,work,cancelled,None)
}
pub(super) fn observe_handles_with_cancel(bytes:&[u8],be:bool,version:u32,registers:usize,outs:usize,t:&Tables<'_>,work:&mut usize,cancelled:&dyn Fn()->bool,handles:Option<&[Handle]>)->Result<Code>{
 observe_context_with_cancel(bytes,be,version,registers,outs,t,work,cancelled,handles,None)
}
pub(super) fn observe_context_with_cancel(bytes:&[u8],be:bool,version:u32,registers:usize,outs:usize,t:&Tables<'_>,work:&mut usize,cancelled:&dyn Fn()->bool,handles:Option<&[Handle]>,sites:Option<&[Site]>)->Result<Code>{
 observe_signatures_with_cancel(bytes,be,version,registers,outs,t,work,cancelled,handles,sites,None)
}
pub(super) fn observe_signatures_with_cancel(bytes:&[u8],be:bool,version:u32,registers:usize,outs:usize,t:&Tables<'_>,work:&mut usize,cancelled:&dyn Fn()->bool,handles:Option<&[Handle]>,sites:Option<&[Site]>,signatures:Option<&Signatures<'_>>)->Result<Code>{
 if cancelled(){return Err("DEX instruction observation cancelled".into());}
 if bytes.len()%2!=0{return Err("DEX instruction bytes have an odd length".into());}let n=bytes.len()/2;if n>65536{return Err("DEX method instruction inspection exceeds 65536 code-unit bound".into());}charge(work,n*8)?;
 let w:Vec<u16>=bytes.chunks_exact(2).map(|b|if be{u16::from_be_bytes([b[0],b[1]])}else{u16::from_le_bytes([b[0],b[1]])}).collect();
 let signed=|at:usize|->i32{((w[at]as u32)|((w[at+1]as u32)<<16))as i32};
 let mut out=Code{instructions:0,payloads:0,references:0,invocations:0,branches:0,rows:vec![],refs:vec![],abbreviated:false,boundaries:vec![]};let mut starts=vec![0u8;n];let mut branches=Vec::new();let mut pc=0;
 while pc<n{if cancelled(){return Err("DEX instruction observation cancelled".into());}let first=w[pc];let op=first as u8;
  if op==0&&first!=0{
   if pc%2!=0{return Err(format!("DEX payload at unit {pc} is not four-byte aligned"));}if pc+2>n{return Err("DEX payload header is truncated".into());}let size=w[pc+1]as usize;
   let (kind,len)=match first{
    0x0100=>{if pc+4>n{return Err("DEX packed-switch header is truncated".into());}let key=signed(pc+2)as i64;if size>0&&key+size as i64-1>i32::MAX as i64{return Err("DEX packed-switch key range overflows signed32".into());}(2,4+size*2)},
    0x0200=>(3,2+size*4),
    0x0300=>{if pc+4>n{return Err("DEX array payload header is truncated".into());}if ![1,2,4,8].contains(&size){return Err("DEX array payload element width is unsupported/invalid".into());}let count=signed(pc+2)as u32 as usize;let len=count.checked_mul(size).and_then(|v|v.checked_add(1)).and_then(|v|v.checked_div(2)).and_then(|v|v.checked_add(4)).ok_or("DEX array payload length overflows")?;(4,len)},
    _=>return Err(format!("DEX unknown payload identifier 0x{first:04x} at unit {pc}"))};
   if len>n-pc{return Err(format!("DEX payload at unit {pc} exceeds code-unit envelope"));}
   if kind==3{let mut previous=None;for i in 0..size{if cancelled(){return Err("DEX instruction observation cancelled".into());}let key=signed(pc+2+i*2);if previous.map(|p|p>=key).unwrap_or(false){return Err("DEX sparse-switch keys are duplicate or unordered".into());}previous=Some(key);}}
   starts[pc]=kind;out.payloads+=1;out.row(format!("Payload unit {pc}: identifier 0x{first:04x}, {len} units (data, not instructions)"));pc+=len;continue;
  }
  let len=(if op==0xfe&&handles.is_none()||matches!(op,0xfc|0xfd)&&sites.is_none(){None}else{width(op)}).ok_or_else(||format!("DEX unsupported/reserved opcode 0x{op:02x} at unit {pc}; method instruction references not accepted"))?;
  if len>n-pc{return Err(format!("DEX opcode 0x{op:02x} at unit {pc} is truncated"));}
  if (op==0x0e||matches!(op,0x03|0x06|0x09|0x29|0x2a))&&first>>8!=0{return Err(format!("DEX reserved instruction bits nonzero at unit {pc}"));}
  if matches!(op,0xfa|0xfb|0xfc|0xfd)&&version<38||matches!(op,0xfe|0xff)&&version<39{return Err("DEX instruction is unavailable in the declared revision".into());}
  starts[pc]=1;out.instructions+=1;let a=(first>>8)as usize;let lo=a&15;let hi=a>>4;let mut regs=Vec::new();let mut add=|v:usize,wide:bool|->Result<()>{reg(v,wide,registers)?;regs.push(v);Ok(())};
  match op{
   0x01|0x04|0x07|0x21|0x7b..=0x8f|0xb0..=0xcf=>{let wa=matches!(op,0x04|0x7d|0x7e|0x80|0x81|0x83|0x86|0x88|0x89|0x8b|0xbb..=0xc5|0xcb..=0xcf);let wb=matches!(op,0x04|0x7d|0x7e|0x80|0x84..=0x86|0x8a..=0x8c|0xbb..=0xc2|0xcb..=0xcf);add(lo,wa)?;add(hi,wb)?;},
   0x02|0x05|0x08=>{add(a,op==5)?;add(w[pc+1]as usize,op==5)?;},
   0x03|0x06|0x09=>{add(w[pc+1]as usize,op==6)?;add(w[pc+2]as usize,op==6)?;},
   0x0a..=0x0d|0x0f..=0x11|0x13..=0x1f|0x22|0x26|0x27|0x2b|0x2c|0x38..=0x3d|0x60..=0x6d|0xfe|0xff=>{add(a,matches!(op,0x0b|0x10|0x16..=0x19|0x61|0x68))?;},
   0x12=>{add(lo,false)?;},
   0x20|0x23|0x32..=0x37|0x52..=0x5f|0xd0..=0xd7=>{add(lo,matches!(op,0x53|0x5a))?;add(hi,false)?;},
   0x2d..=0x31|0x44..=0x51|0x90..=0xaf=>{let b=w[pc+1]as usize;add(a,matches!(op,0x45|0x4c|0x9b..=0xa5|0xab..=0xaf))?;add(b&255,matches!(op,0x2f..=0x31|0x9b..=0xa5|0xab..=0xaf))?;add(b>>8,matches!(op,0x2f..=0x31|0x9b..=0xa2|0xab..=0xaf))?;},
   0xd8..=0xe2=>{add(a,false)?;add((w[pc+1]&255)as usize,false)?;},
   0x24|0x6e..=0x72|0xfa|0xfc=>{if hi>5{return Err("DEX argument register list exceeds five words".into());}let b=w[pc+2];let list=[(b&15)as usize,((b>>4)&15)as usize,((b>>8)&15)as usize,(b>>12)as usize,lo];for &v in &list[..hi]{add(v,false)?;}if op!=0x24&&hi>outs{return Err("DEX invocation argument words exceed outs_size".into());}},
   0x25|0x74..=0x78|0xfb|0xfd=>{let start=w[pc+2]as usize;if a>0&&start.checked_add(a).map(|v|v>registers).unwrap_or(true){return Err("DEX argument register range exceeds frame".into());}if a>0{add(start,false)?;if a>1{add(start+a-1,false)?;}}if op!=0x25&&a>outs{return Err("DEX invocation argument range exceeds outs_size".into());}},
   _=>{},
  }
  if matches!(op,0xfc|0xfd){let list=sites.unwrap_or(&[]);let id=w[pc+1]as usize;out.reference(pc,"call-site",id,list.len(),true)?;let site=&list[id];out.reference(pc,"prototype",site.proto,t.proto_words.len(),false)?;let count=if op==0xfd{a}else{hi};if count!=t.proto_words[site.proto]{return Err("DEX invoke-custom argument word count differs from call-site prototype".into());}if let Some(sig)=signatures{arguments(op,&w,pc,count,site.proto,false,sig,work,cancelled)?;}}
  if op==0xfe{let h=handles.unwrap_or(&[]);let id=w[pc+1]as usize;out.reference(pc,"method-handle",id,h.len(),false)?;let handle=&h[id];let(kind,count)=if handle.kind<=3{("field",t.fields)}else if handle.kind<=8{("method",t.method_words.len())}else{return Err("DEX method-handle kind is unknown".into());};out.reference(pc,kind,handle.index,count,false)?;}
  let reference=match op{0x1a|0x1b=>Some(("string",if op==0x1b{signed(pc+1)as u32 as usize}else{w[pc+1]as usize},t.strings,false)),0x1c|0x1f|0x20|0x22..=0x25=>Some(("type",w[pc+1]as usize,t.types,false)),0x52..=0x6d=>Some(("field",w[pc+1]as usize,t.fields,false)),0x6e..=0x72|0x74..=0x78|0xfa|0xfb=>Some(("method",w[pc+1]as usize,t.method_words.len(),true)),0xff=>Some(("prototype",w[pc+1]as usize,t.proto_words.len(),false)),_=>None};
  if let Some((kind,id,count,call))=reference{out.reference(pc,kind,id,count,call)?;if call{let count=if matches!(op,0x74..=0x78|0xfb){a}else{hi};let wanted=if matches!(op,0xfa|0xfb){let proto=w[pc+3]as usize;out.reference(pc,"prototype",proto,t.proto_words.len(),false)?;t.proto_words[proto]+1}else{t.method_words[id]+usize::from(!matches!(op,0x71|0x77))};if count!=wanted{return Err(format!("DEX invocation argument word count {count} differs from declared signature {wanted}"));}if let Some(sig)=signatures{let proto=if matches!(op,0xfa|0xfb){w[pc+3]as usize}else{sig.methods.get(id).ok_or("DEX invocation signature context lacks method")?.1};arguments(op,&w,pc,count,proto,!matches!(op,0x71|0x77),sig,work,cancelled)?;}}}
  let jump=match op{0x28=>Some(((first>>8)as u8 as i8 as i32,0)),0x29=>Some((w[pc+1]as i16 as i32,0)),0x2a=>Some((signed(pc+1),0)),0x32..=0x3d=>Some((w[pc+1]as i16 as i32,0)),0x26=>Some((signed(pc+1),4)),0x2b=>Some((signed(pc+1),2)),0x2c=>Some((signed(pc+1),3)),_=>None};
  if let Some((delta,kind))=jump{if kind==0&&delta==0&&op!=0x2a{return Err("DEX zero branch displacement is forbidden for this encoding".into());}branches.push((pc,target(pc,delta,n)?,kind));}
  let ref_text=if matches!(op,0xfc|0xfd){format!("; call-site#{}",w[pc+1])}else if op==0xfe{format!("; method-handle#{}",w[pc+1])}else{reference.map(|(k,i,_,_)|format!("; {k}#{i}")).unwrap_or_default()};out.row(format!("Instruction unit {pc}: {} opcode 0x{op:02x}, width {len}, registers {regs:?}{ref_text}{}",name(op),operands(op,&w,pc)));pc+=len;
 }
 for(from,to,kind)in branches{if cancelled(){return Err("DEX instruction observation cancelled".into());}if kind==0{if starts[to]!=1{return Err("DEX control branch targets payload/interior instead of instruction boundary".into());}out.branches+=1;out.row(format!("Encoded control edge unit {from} -> unit {to}"));}else{
  if starts[to]!=kind{return Err("DEX payload reference targets wrong payload kind/interior".into());}out.row(format!("Payload reference unit {from} -> unit {to}"));if kind==4{continue;}let size=w[to+1]as usize;charge(work,size*2)?;let offset=if kind==2{to+4}else{to+2+size*2};for i in 0..size{if cancelled(){return Err("DEX instruction observation cancelled".into());}let dest=target(from,signed(offset+i*2),n)?;if starts[dest]!=1{return Err("DEX switch case targets payload/interior instead of instruction boundary".into());}out.branches+=1;out.row(format!("Encoded switch edge unit {from} case#{i} -> unit {dest}"));}
 }}
 out.boundaries=starts;Ok(out)
}
