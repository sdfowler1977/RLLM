//! Fixed-width branch encodings in declared executable mappings, never execution.
use std::{collections::BTreeSet,fmt::Write as _};
const WORDS:usize=512;
const ROWS:usize=64;
const OUTPUT:usize=8191;
#[derive(Default)]pub(super) struct Observation{pub text:String,pub statements:Vec<super::instruction::Statement>,pub coordinate:String,pub regions:Vec<Region>,pub mappings_complete:bool,pub classifications:Vec<Classification>,pub classifications_complete:Option<bool>,pub relocations:Vec<Relocation>,pub pe_relocations:Vec<PeRelocation>,pub relocations_complete:Option<bool>}
#[derive(Clone,Copy)]pub(super) struct Region{pub offset:u64,pub address:u64,pub file_size:u64,pub memory_size:u64,pub executable:bool}
#[derive(Clone,Copy)]pub(super) enum Branch{Direct{link:bool,offset:i64},Conditional{condition:u8,offset:i64,consistent:bool},Compare{nonzero:bool,wide:bool,register:u8,offset:i64},Test{nonzero:bool,bit:u8,register:u8,offset:i64},Register{kind:u8,register:u8},Authenticated{kind:u8,register:u8,modifier:Option<u8>,key_b:bool}}
#[derive(Clone)]pub(super) struct Classification{pub section:u64,pub start:u64,pub end:u64,pub code:bool,pub mapping_agrees:bool,pub contradicted:bool,pub excludes_whole_data_words:bool}
#[derive(Clone)]pub(super) struct Symbol{pub name_display:String,pub info:u8,pub section:u16,pub value:u64}
#[derive(Clone)]pub(super) struct Relocation{pub section:u64,pub record:u64,pub file_offset:u64,pub position:u64,pub section_relative:bool,pub target_section:u32,pub code:u32,pub name:&'static str,pub symbol_index:u64,pub symbol:Option<Symbol>,pub addend:Option<i64>}
#[derive(Clone)]pub(super) struct PeRelocation{pub block:u32,pub slot:u32,pub file_offset:u64,pub page:u32,pub offset:u16,pub code:u8,pub adjustment:Option<(u16,u64)>,pub target_file_offset:Option<u64>}
fn signed(value:u32,bits:u32)->i64{((value as i64)<<(64-bits))>>(64-bits)}
pub(super) fn decode(word:u32)->Option<Branch>{decode_version(word,1)}
pub(super) fn decode_version(word:u32,version:u64)->Option<Branch>{
 if version==3 {
  let class=match word&0xfeff_f800{0xd61f_0800=>Some(0),0xd63f_0800=>Some(1),_=>None};
  if let Some(kind)=class{let register=((word>>5)&31)as u8;let modifier=(word&31)as u8;let nonzero=word&(1<<24)!=0;if !nonzero&&modifier!=31{return None;}return Some(Branch::Authenticated{kind,register,modifier:if nonzero{Some(modifier)}else{None},key_b:word&(1<<10)!=0});}
  if word&0xffff_fbff==0xd65f_0bff{return Some(Branch::Authenticated{kind:2,register:30,modifier:Some(31),key_b:word&(1<<10)!=0});}
 }
 if version>=2 && word&0xff00_0010==0x5400_0010{return Some(Branch::Conditional{condition:(word&15)as u8,offset:signed((word>>5)&0x7ffff,19)*4,consistent:true});}
 if word&0x7c00_0000==0x1400_0000{return Some(Branch::Direct{link:word>>31!=0,offset:signed(word&0x03ff_ffff,26)*4});}
 if word&0xff00_0010==0x5400_0000{return Some(Branch::Conditional{condition:(word&15)as u8,offset:signed((word>>5)&0x7ffff,19)*4,consistent:false});}
 if word&0x7e00_0000==0x3400_0000{return Some(Branch::Compare{nonzero:word&(1<<24)!=0,wide:word>>31!=0,register:(word&31)as u8,offset:signed((word>>5)&0x7ffff,19)*4});}
 if word&0x7e00_0000==0x3600_0000{return Some(Branch::Test{nonzero:word&(1<<24)!=0,bit:(((word>>26)&32)|((word>>19)&31))as u8,register:(word&31)as u8,offset:signed((word>>5)&0x3fff,14)*4});}
 let kind=match word&0xffff_fc1f{0xd61f_0000=>0,0xd63f_0000=>1,0xd65f_0000=>2,_=>return None};Some(Branch::Register{kind,register:((word>>5)&31)as u8})
}
pub(super) fn authentication_name(kind:u8,key_b:bool,modifier:Option<u8>)->&'static str{
 match(kind,key_b,modifier.is_some()){(0,false,true)=>"BRAA",(0,true,true)=>"BRAB",(0,false,false)=>"BRAAZ",(0,true,false)=>"BRABZ",(1,false,true)=>"BLRAA",(1,true,true)=>"BLRAB",(1,false,false)=>"BLRAAZ",(1,true,false)=>"BLRABZ",(2,false,_)=>"RETAA",(2,true,_)=>"RETAB",_=>"unclassified"}
}
pub(super) fn authentication_operand(modifier:Option<u8>)->String{match modifier{None=>"zero modifier".into(),Some(31)=>"SP modifier".into(),Some(r)=>format!("X{r} modifier")}}
pub(super) fn displaced(pc:u64,offset:i64)->Option<u64>{if offset<0{pc.checked_sub(offset.unsigned_abs())}else{pc.checked_add(offset as u64)}}
/// Every intersecting memory mapping participates, including partial aliases
/// and virtual-only tails; unseen or conflicting mappings are never guessed.
pub(super) fn mapped(regions:&[Region],address:u64,size:u64)->Result<(u64,bool),&'static str>{
 let end=address.checked_add(size).ok_or("address overflow")?;let mut chosen=None;let mut executable=false;
 for r in regions{let re=r.address.checked_add(r.memory_size).ok_or("mapping address overflow")?;if address>=r.address&&end<=re{let delta=address-r.address;if delta.checked_add(size).is_some_and(|n|n<=r.file_size){let off=r.offset.checked_add(delta).ok_or("mapping offset overflow")?;if chosen.is_some_and(|old|old!=off){return Err("conflicting mapped bytes");}chosen=Some(off);executable|=r.executable;}}}
 let offset=chosen.ok_or("no complete file-backed mapping")?;
 for r in regions{let re=r.address.checked_add(r.memory_size).ok_or("mapping address overflow")?;let start=address.max(r.address);let stop=end.min(re);if start>=stop{continue;}let file_end=r.address.checked_add(r.file_size).ok_or("mapping address overflow")?;if stop>file_end{return Err("intersecting virtual-only tail");}let actual=r.offset.checked_add(start-r.address).ok_or("mapping offset overflow")?;let expected=offset.checked_add(start-address).ok_or("mapping offset overflow")?;if actual!=expected{return Err("conflicting intersecting mapping");}}
 Ok((offset,executable))
}
fn register_name(n:u8,wide:bool)->String{if n==31{if wide{"XZR".into()}else{"WZR".into()}}else{format!("{}{n}",if wide{"X"}else{"W"})}}
fn operand(n:u8)->&'static str{if n==31{"zero-register encoding has value0; execution unobserved"}else{"register value unobserved"}}
fn target(regions:&[Region],data:&[u8],pc:u64,delta:i64,label:&str)->String{
 let Some(address)=displaced(pc,delta)else{return "target unresolved (address arithmetic overflow)".into()};
 if label=="RVA "&&address>u32::MAX as u64{return format!("encoded relative target outside32-bit image RVA width (0x{address:x}); mapping unresolved, not an invalid-opcode assertion");}
 let mapping=match mapped(regions,address,4){Ok((off,exec))=>format!("file offset {off}; {}; {}",if exec{"executable declaration"}else{"non-executable declaration"},if off.checked_add(4).is_some_and(|end|end<=data.len()as u64){"target word captured"}else{"target word outside examined prefix"}),Err(reason)=>format!("unresolved mapping: {reason}")};
 format!("encoded target {label}0x{address:x} ({mapping})")
}
fn row(regions:&[Region],data:&[u8],off:u64,pc:u64,word:u32,branch:Branch,label:&str)->String{
 let body=match branch{
  Branch::Direct{link,offset}=>format!("{} {}; {}",if link{"BL"}else{"B"},offset,target(regions,data,pc,offset,label)),
  Branch::Conditional{condition,offset,consistent}=>format!("{} condition=0x{condition:x}, displacement {offset}; {}; condition state unobserved",if consistent{"BC.cond (FEAT_HBC)"}else{"B.cond"},target(regions,data,pc,offset,label)),
  Branch::Compare{nonzero,wide,register,offset}=>format!("{} {}, displacement {offset}; {}; {}",if nonzero{"CBNZ"}else{"CBZ"},register_name(register,wide),target(regions,data,pc,offset,label),operand(register)),
  Branch::Test{nonzero,bit,register,offset}=>format!("{} {}, bit {bit}, displacement {offset}; {}; {}",if nonzero{"TBNZ"}else{"TBZ"},register_name(register,bit>=32),target(regions,data,pc,offset,label),operand(register)),
  Branch::Authenticated{kind,register,modifier,key_b}=>format!("{} X{register}, {}, instruction key {}; FEAT_PAuth; authenticated target remains runtime-dependent",authentication_name(kind,key_b,modifier),authentication_operand(modifier),if key_b{"B"}else{"A"}),
  Branch::Register{kind,register}=>format!("{} {}; {}",match kind{0=>"BR",1=>"BLR",_=>"RET"},register_name(register,true),if register==31{"zero-register encoded absolute target0x0 (not image RVA); execution unobserved"}else{"indirect target unresolved (register state unobserved)"}),
 };
 format!("Encoding candidate at file offset {off}, {label}0x{pc:x}, word0x{word:08x}: {body}.\n")
}
/// Input bytes are borrowed from the ordinary held file inspection. Independent
/// fixed-width batches use a bounded CPU pool and retain deterministic order.
pub(super) fn analyze(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool)->Result<String,String>{
 analyze_with_data(data,regions,inventory_complete,label,cancelled,&[])
}
pub(super) fn analyze_with_data(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool,data_ranges:&[(u64,u64)])->Result<String,String>{
 observe(data,regions,inventory_complete,label,cancelled,data_ranges).map(|v|v.text)
}
pub(super) fn observe(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool,data_ranges:&[(u64,u64)])->Result<Observation,String>{
 if cancelled(){return Err("Inspection cancelled during control-flow observations".into());}
 let mut out=String::from("AArch64 control-flow encoding observations\nScope: aligned encoding candidates in declared executable file-backed mappings. Coordinates identify declared ELF virtual addresses or relative PE RVAs. Instruction words use little-endian encoding. Bounds: 512 examined words, 64 reported candidates, 8191 report bytes.\n");
 if !inventory_complete{out.push_str("Mapping inventory incomplete or invalid; branch candidates withheld because unseen mappings may conflict.\n");return Ok(Observation{text:out,coordinate:label.into(),regions:regions.to_vec(),mappings_complete:inventory_complete,..Default::default()});}
 let mut ordered=regions.to_vec();ordered.sort_by_key(|r|(r.address,r.offset));let mut words=Vec::new();let mut seen=BTreeSet::new();let mut bounded=false;let mut invalid=0;let mut unavailable=0;let mut examined=0;let mut declared_data=0;
 'regions:for r in &ordered{
  if cancelled(){return Err("Inspection cancelled during control-flow observations".into());}if !r.executable||r.file_size==0{continue;}
  if r.file_size>r.memory_size||r.offset.checked_add(r.file_size).is_none()||r.address.checked_add(r.memory_size).is_none(){invalid+=1;continue;}
  let leading=(4-r.address%4)%4;let available=r.file_size.min((data.len()as u64).saturating_sub(r.offset));if available<r.file_size{unavailable+=1;}let mut delta=leading;
  while delta.checked_add(4).is_some_and(|n|n<=available){if examined%16==0&&cancelled(){return Err("Inspection cancelled during control-flow observations".into());}if examined>=WORDS{bounded=true;break 'regions;}examined+=1;let pc=r.address+delta;let off=r.offset+delta;delta+=4;if !seen.insert((pc,off)){continue;}match mapped(&ordered,pc,4){Ok((at,true))if at==off=>{if data_ranges.iter().any(|&(start,end)|start<=pc&&pc.checked_add(4).is_some_and(|n|n<=end)){declared_data+=1;continue;}words.push((off,pc,u32::from_le_bytes(data[off as usize..off as usize+4].try_into().unwrap())));},_=>invalid+=1,}}
 }
 let jobs:Vec<_>=words.chunks(16).collect();let workers=std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(4).min(jobs.len()).max(1);
 let observe=|(off,pc,word)|{let branch=decode_version(word,3);(off,pc,word,branch,super::instruction::decode(word,branch))};
 let (decoded,workers)=if workers==1{(words.iter().copied().map(observe).collect::<Vec<_>>(),usize::from(!words.is_empty()))}else{std::thread::scope(|scope|{
  let work=|worker:usize|{let mut result=Vec::new();for i in (worker..jobs.len()).step_by(workers){for &word in jobs[i]{result.push(observe(word));}}result};
  let mut handles=Vec::new();let mut all=Vec::new();let mut inline=false;
  for worker in 0..workers{match std::thread::Builder::new().spawn_scoped(scope,move||work(worker)){Ok(handle)=>handles.push(handle),Err(_)=>{all.extend(work(worker));inline=true;}}}
  let actual=handles.len()+usize::from(inline);for handle in handles{all.extend(handle.join().map_err(|_|"Control-flow observation worker did not complete")?);}Ok::<_,String>((all,actual))
 })?};
 if cancelled(){return Err("Inspection cancelled during control-flow observations".into());}
 let mut decoded=decoded;decoded.sort_by_key(|r|(r.1,r.0));let matched=decoded.iter().filter(|r|r.3.is_some()).count();let mut reported=0;
 for &(off,pc,word,branch,_)in &decoded{if let Some(branch)=branch{let line=row(&ordered,data,off,pc,word,branch,label);if reported>=ROWS||out.len()+line.len()>OUTPUT-1024{continue;}out.push_str(&line);reported+=1;}}
 let _=writeln!(out,"Examined {examined} aligned words, {} unique mapped words using {workers} CPU workers; recognized {matched} branch encodings, reported {reported}, omitted {} recognized rows. Other words retain their unclassified state. Word work bound reached: {bounded}; unavailable executable prefixes: {unavailable}; invalid/conflicting word or range mappings: {invalid}.",words.len(),matched-reported);
 let _=writeln!(out,"Excluded {declared_data} whole words declared as data by the complete available ELF mapping-symbol inventory; no runtime code/data classification inferred.");
 out.push_str("This branch pass interprets B/BL, B.cond, BC.cond (FEAT_HBC encoding), CBZ/CBNZ, TBZ/TBNZ, BR/BLR/RET and FEAT_PAuth authenticated branch/call/return encodings. \n");
 let ir=super::instruction::observe(&decoded,label,cancelled)?;out.push('\n');out.push_str(&ir.text);Ok(Observation{text:out,statements:ir.statements,coordinate:label.into(),regions:regions.to_vec(),mappings_complete:inventory_complete,..Default::default()})
}
