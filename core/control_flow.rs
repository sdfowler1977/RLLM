//! Fixed-width branch encodings in declared executable mappings, never execution.
use std::{collections::{BTreeMap,BTreeSet},fmt::Write as _};
const WORDS:usize=512;
const ROWS:usize=64;
const OUTPUT:usize=8191;
#[derive(Default)]pub(super) struct Observation{pub text:String,pub statements:Vec<super::instruction::Statement>,pub coordinate:String,pub regions:Vec<Region>,pub mappings_complete:bool,pub classifications:Vec<Classification>,pub classifications_complete:Option<bool>,pub relocations:Vec<Relocation>,pub pe_relocations:Vec<PeRelocation>,pub relocations_complete:Option<bool>,pub xrefs:CrossReferences}
#[derive(Clone,Copy)]pub(super) struct Region{pub offset:u64,pub address:u64,pub file_size:u64,pub memory_size:u64,pub executable:bool}
#[derive(Clone,Copy)]pub(super) enum Branch{Direct{link:bool,offset:i64},Conditional{condition:u8,offset:i64,consistent:bool},Compare{nonzero:bool,wide:bool,register:u8,offset:i64},Test{nonzero:bool,bit:u8,register:u8,offset:i64},Register{kind:u8,register:u8},Authenticated{kind:u8,register:u8,modifier:Option<u8>,key_b:bool}}
#[derive(Clone)]pub(super) struct Classification{pub section:u64,pub start:u64,pub end:u64,pub code:bool,pub mapping_agrees:bool,pub contradicted:bool,pub excludes_whole_data_words:bool}
#[derive(Clone)]pub(super) struct Symbol{pub name_display:String,pub info:u8,pub section:u16,pub value:u64}
#[derive(Clone)]pub(super) struct Relocation{pub section:u64,pub record:u64,pub file_offset:u64,pub position:u64,pub section_relative:bool,pub target_section:u32,pub code:u32,pub name:&'static str,pub symbol_index:u64,pub symbol:Option<Symbol>,pub addend:Option<i64>}
#[derive(Clone)]pub(super) struct PeRelocation{pub block:u32,pub slot:u32,pub file_offset:u64,pub page:u32,pub offset:u16,pub code:u8,pub adjustment:Option<(u16,u64)>,pub target_file_offset:Option<u64>}
fn signed(value:u32,bits:u32)->i64{((value as i64)<<(64-bits))>>(64-bits)}
/// Base fixed-width encodings. Feature-gated forms stay in `decode_version`.
pub(super) fn decode(word:u32)->Option<Branch>{
 if word&0x7c00_0000==0x1400_0000{return Some(Branch::Direct{link:word>>31!=0,offset:signed(word&0x03ff_ffff,26)*4});}
 if word&0xff00_0010==0x5400_0000{return Some(Branch::Conditional{condition:(word&15)as u8,offset:signed((word>>5)&0x7ffff,19)*4,consistent:false});}
 if word&0x7e00_0000==0x3400_0000{return Some(Branch::Compare{nonzero:word&(1<<24)!=0,wide:word>>31!=0,register:(word&31)as u8,offset:signed((word>>5)&0x7ffff,19)*4});}
 if word&0x7e00_0000==0x3600_0000{return Some(Branch::Test{nonzero:word&(1<<24)!=0,bit:(((word>>26)&32)|((word>>19)&31))as u8,register:(word&31)as u8,offset:signed((word>>5)&0x3fff,14)*4});}
 let kind=match word&0xffff_fc1f{0xd61f_0000=>0,0xd63f_0000=>1,0xd65f_0000=>2,_=>return None};Some(Branch::Register{kind,register:((word>>5)&31)as u8})
}
pub(super) fn decode_version(word:u32,version:u64)->Option<Branch>{
 if version==3 {
  let class=match word&0xfeff_f800{0xd61f_0800=>Some(0),0xd63f_0800=>Some(1),_=>None};
  if let Some(kind)=class{let register=((word>>5)&31)as u8;let modifier=(word&31)as u8;let nonzero=word&(1<<24)!=0;if !nonzero&&modifier!=31{return None;}return Some(Branch::Authenticated{kind,register,modifier:if nonzero{Some(modifier)}else{None},key_b:word&(1<<10)!=0});}
  if word&0xffff_fbff==0xd65f_0bff{return Some(Branch::Authenticated{kind:2,register:30,modifier:Some(31),key_b:word&(1<<10)!=0});}
 }
 if version>=2 && word&0xff00_0010==0x5400_0010{return Some(Branch::Conditional{condition:(word&15)as u8,offset:signed((word>>5)&0x7ffff,19)*4,consistent:true});}
 decode(word)
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
/// Retained direct-edge bound. One past this count is a named refusal, not another edge.
pub(super) const XREF_MAX:usize=64;
const XREF_SYMBOL_LIST:usize=8;
const XREF_OUTPUT:usize=4096;
pub(super) const XREF_EDGE_CAP:&str="cross-reference edge cap";
const XREF_LIST_CAP:&str="cross-reference symbol list cap";
const XREF_OUTPUT_CAP:&str="cross-reference output cap";
const XREF_UNKNOWN_CAP:&str="cross-reference unknown cap";
pub(super) const CODE_SYMBOL_MAX:usize=128;
const CODE_SYMBOL_SCAN:usize=2048;
pub(super) const CODE_SYMBOL_CAP:&str="code-symbol record cap";
const CODE_SYMBOL_SCAN_CAP:&str="code-symbol scan cap";
const CODE_SYMBOL_SECTION_CAP:&str="code-symbol section cap";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub(super) enum XrefKind{Branch,Call,ConditionalBranch,Return}
impl XrefKind{pub(super) fn name(self)->&'static str{match self{Self::Branch=>"branch",Self::Call=>"call",Self::ConditionalBranch=>"conditional_branch",Self::Return=>"return"}}}
#[derive(Clone,Debug,PartialEq,Eq)]pub(super) struct XrefEdge{pub from_pc:u64,pub to_pc:u64,pub kind:XrefKind,pub region:String}
#[derive(Clone,Debug,PartialEq,Eq)]pub(super) struct XrefUnknown{pub from_pc:u64,pub kind:XrefKind,pub reason:&'static str}
#[derive(Clone,Debug,PartialEq,Eq)]pub(super) struct SymbolView{pub address:u64,pub name:Option<String>,pub outgoing_calls:Vec<u64>,pub incoming:Vec<(u64,XrefKind)>,pub unknown:u64}
#[derive(Clone,Debug,Default,PartialEq,Eq)]pub(super) struct CrossReferences{pub edges:Vec<XrefEdge>,pub unknowns:Vec<XrefUnknown>,pub symbols:Vec<SymbolView>,pub refusal:Option<&'static str>}
#[derive(Clone,Debug,PartialEq,Eq)]pub(super) struct CodeSymbol{pub name_display:String,pub value:u64,pub size:u64}
#[derive(Clone,Debug,Default,PartialEq,Eq)]pub(super) struct SymbolRead{pub symbols:Vec<CodeSymbol>,pub refusal:Option<&'static str>}
enum Target{Edge{kind:XrefKind,to:u64},Unknown{kind:XrefKind,reason:&'static str}}
fn indirect_kind(kind:u8)->XrefKind{match kind{1=>XrefKind::Call,2=>XrefKind::Return,_=>XrefKind::Branch}}
fn indirect_reason(kind:u8,authenticated:bool)->&'static str{
 if authenticated{return "authenticated indirect target unresolved (runtime authentication context unobserved)";}
 match kind{0=>"indirect branch unresolved (register state unobserved)",1=>"indirect call unresolved (register state unobserved)",_=>"return target unresolved (register state unobserved)"}
}
fn resolved_target(pc:u64,offset:i64,kind:XrefKind)->Target{
 match (displaced(pc,offset),super::instruction::coordinate_target(pc,offset)){
  (Some(to),Some(same)) if to==same=>Target::Edge{kind,to},
  (Some(_),Some(_))=>Target::Unknown{kind,reason:"target unresolved (displacement and coordinate arithmetic disagree)"},
  _=>Target::Unknown{kind,reason:"target unresolved (address arithmetic overflow)"},
 }
}
fn direct_target(pc:u64,branch:Branch)->Target{
 match branch{
  Branch::Direct{link,offset}=>resolved_target(pc,offset,if link{XrefKind::Call}else{XrefKind::Branch}),
  Branch::Conditional{offset,..}|Branch::Compare{offset,..}|Branch::Test{offset,..}=>resolved_target(pc,offset,XrefKind::ConditionalBranch),
  Branch::Register{kind,..}=>Target::Unknown{kind:indirect_kind(kind),reason:indirect_reason(kind,false)},
  Branch::Authenticated{kind,..}=>Target::Unknown{kind:indirect_kind(kind),reason:indirect_reason(kind,true)},
 }
}
fn landing(regions:&[Region],address:u64)->String{
 match mapped(regions,address,4){
  Ok((off,exec))=>{
   let found=regions.iter().enumerate().find(|(_,r)|address>=r.address&&r.file_size>address-r.address&&r.offset.checked_add(address-r.address)==Some(off));
   match found{
    Some((index,r))=>format!("region {index} address 0x{:x} file offset {off} {}",r.address,if exec{"executable declaration"}else{"non-executable declaration"}),
    None=>format!("file offset {off} {}",if exec{"executable declaration"}else{"non-executable declaration"}),
   }
  }
  Err(reason)=>{
   let inside=regions.iter().any(|r|r.memory_size>0&&address>=r.address&&address-r.address<r.memory_size);
   if inside{format!("unresolved mapping: {reason}")}else{"outside every mapped region".into()}
  }
 }
}
fn symbol_name<'a>(symbols:&'a [CodeSymbol],address:u64)->Option<&'a str>{
 let mut exact=None;let mut range=None;let mut exact_count=0usize;let mut range_count=0usize;
 for symbol in symbols{
  if symbol.value==address{exact_count+=1;exact=Some(symbol.name_display.as_str());}
  else if symbol.size>0{if let Some(delta)=address.checked_sub(symbol.value){if delta>0&&delta<symbol.size{range_count+=1;range=Some(symbol.name_display.as_str());}}}
 }
 if exact_count==1{return exact;}
 if exact_count==0&&range_count==1{return range;}
 None
}
fn symbol_views(edges:&[XrefEdge],unknown_counts:&BTreeMap<u64,u64>,names:&[CodeSymbol],refusal:&mut Option<&'static str>)->Vec<SymbolView>{
 let mut addresses=BTreeSet::new();
 for edge in edges{addresses.insert(edge.from_pc);addresses.insert(edge.to_pc);}
 for &pc in unknown_counts.keys(){addresses.insert(pc);}
 let mut symbols=Vec::new();
 for address in addresses{
  let mut outgoing=Vec::new();let mut incoming=Vec::new();
  for edge in edges{
   if edge.kind==XrefKind::Call&&edge.from_pc==address{
    if outgoing.len()>=XREF_SYMBOL_LIST{if refusal.is_none(){*refusal=Some(XREF_LIST_CAP);}}else{outgoing.push(edge.to_pc);}
   }
   if edge.to_pc==address{
    if incoming.len()>=XREF_SYMBOL_LIST{if refusal.is_none(){*refusal=Some(XREF_LIST_CAP);}}else{incoming.push((edge.from_pc,edge.kind));}
   }
  }
  let unknown=match unknown_counts.get(&address){Some(n)=>*n,None=>0};
  symbols.push(SymbolView{address,name:symbol_name(names,address).map(|name|name.to_string()),outgoing_calls:outgoing,incoming,unknown});
 }
 symbols
}
/// Direct targets become edges only when `displaced` and coordinate arithmetic agree.
/// `BR`, `BLR`, `RET` and authenticated forms stay unknown; their register values are not guessed.
pub(super) fn cross_references(sites:&[(u64,Branch)],regions:&[Region],symbols:&[CodeSymbol])->CrossReferences{
 let mut edges=Vec::new();let mut unknowns=Vec::new();let mut counts:BTreeMap<u64,u64>=BTreeMap::new();let mut refusal=None;
 for &(pc,branch) in sites{
  match direct_target(pc,branch){
   Target::Edge{kind,to}=>{
    if edges.len()>=XREF_MAX{if refusal.is_none(){refusal=Some(XREF_EDGE_CAP);}continue;}
    edges.push(XrefEdge{from_pc:pc,to_pc:to,kind,region:landing(regions,to)});
   }
   Target::Unknown{kind,reason}=>{
    counts.entry(pc).and_modify(|n|*n=n.saturating_add(1)).or_insert(1);
    if unknowns.len()>=XREF_MAX{if refusal.is_none(){refusal=Some(XREF_UNKNOWN_CAP);}continue;}
    unknowns.push(XrefUnknown{from_pc:pc,kind,reason});
   }
  }
 }
 let symbols=symbol_views(&edges,&counts,symbols,&mut refusal);
 CrossReferences{edges,unknowns,symbols,refusal}
}
fn xref_line(out:&mut String,line:&str)->bool{
 if out.contains(XREF_OUTPUT_CAP){return false;}
 let reserve=XREF_OUTPUT_CAP.len()+1;
 if out.len().saturating_add(line.len()).saturating_add(1).saturating_add(reserve)>XREF_OUTPUT{out.push_str(XREF_OUTPUT_CAP);out.push('\n');return false;}
 out.push_str(line);out.push('\n');true
}
fn xref_text(set:&CrossReferences)->String{
 let mut out=String::new();
 let _=xref_line(&mut out,"AArch64 calls and cross-references");
 let _=xref_line(&mut out,"Scope: direct targets that displaced() and coordinate arithmetic resolve to the same address. Indirect and authenticated targets are unknown and are not guessed. Each resolved target is passed through mapped().");
 if let Some(reason)=set.refusal{let _=xref_line(&mut out,&format!("Refusal: {reason}."));}
 // The refusal field keeps the first cap. A later cap still names itself in the report.
 let unknown_total=set.symbols.iter().fold(0u64,|total,symbol|total.saturating_add(symbol.unknown));
 if unknown_total>set.unknowns.len() as u64&&!out.contains(XREF_UNKNOWN_CAP){let _=xref_line(&mut out,&format!("Refusal: {XREF_UNKNOWN_CAP}."));}
 let lists_cut=set.symbols.iter().any(|symbol|{let incoming=set.edges.iter().filter(|edge|edge.to_pc==symbol.address).count();let outgoing=set.edges.iter().filter(|edge|edge.kind==XrefKind::Call&&edge.from_pc==symbol.address).count();incoming>symbol.incoming.len()||outgoing>symbol.outgoing_calls.len()});
 if lists_cut&&!out.contains(XREF_LIST_CAP){let _=xref_line(&mut out,&format!("Refusal: {XREF_LIST_CAP}."));}
 let _=xref_line(&mut out,&format!("Edges {}; stored unknowns {}; symbols {}.",set.edges.len(),set.unknowns.len(),set.symbols.len()));
 for edge in &set.edges{if !xref_line(&mut out,&format!("Edge {} from 0x{:x} to 0x{:x}; {}.",edge.kind.name(),edge.from_pc,edge.to_pc,edge.region)){break;}}
 for unknown in &set.unknowns{if !xref_line(&mut out,&format!("Unknown {} at 0x{:x}: {}.",unknown.kind.name(),unknown.from_pc,unknown.reason)){break;}}
 for symbol in &set.symbols{
  let calls=symbol.outgoing_calls.iter().map(|address|format!("0x{address:x}")).collect::<Vec<_>>().join(", ");
  let incoming=symbol.incoming.iter().map(|(address,kind)|format!("{} from 0x{address:x}",kind.name())).collect::<Vec<_>>().join(", ");
  let name=match &symbol.name{Some(name)=>format!(" \"{name}\""),None=>String::new()};
  if !xref_line(&mut out,&format!("Symbol 0x{:x}{name}: outgoing calls {} ({calls}); incoming references {} ({incoming}); unknown {}.",symbol.address,symbol.outgoing_calls.len(),symbol.incoming.len(),symbol.unknown)){break;}
 }
 out
}
fn le_u16(data:&[u8],at:usize)->Option<u16>{Some(u16::from_le_bytes(data.get(at..at.checked_add(2)?)?.try_into().ok()?))}
fn le_u32(data:&[u8],at:usize)->Option<u32>{Some(u32::from_le_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))}
fn le_u64(data:&[u8],at:usize)->Option<u64>{Some(u64::from_le_bytes(data.get(at..at.checked_add(8)?)?.try_into().ok()?))}
fn display_symbol(bytes:&[u8])->Option<String>{
 let end=bytes.iter().position(|b|*b==0)?;if end==0{return None;}let mut out=String::new();
 for &byte in &bytes[..end]{
  let part=if (b' '..=b'~').contains(&byte)&&byte!=b'"'&&byte!=b'\\'{(byte as char).to_string()}else{format!("\\x{byte:02x}")};
  if out.len()+part.len()>128{out.push('…');break;}out.push_str(&part);
 }
 Some(out)
}
fn elf_bytes<'a>(data:&'a [u8],offset:u64,size:u64)->Option<&'a [u8]>{
 let start=usize::try_from(offset).ok()?;let len=usize::try_from(size).ok()?;data.get(start..start.checked_add(len)?)
}
/// ELF64 little-endian AArch64 function symbols, used only to name an address that matches one symbol.
/// Any other image, or a table this pass cannot prove, yields no names.
pub(super) fn elf_function_symbols(data:&[u8])->SymbolRead{
 if data.len()<64||data.get(..4)!=Some(b"\x7fELF")||data.get(4)!=Some(&2)||data.get(5)!=Some(&1)||data.get(6)!=Some(&1){return SymbolRead::default();}
 if le_u16(data,18)!=Some(183){return SymbolRead::default();}
 let (Some(shoff),Some(shentsize),Some(shnum))=(le_u64(data,40),le_u16(data,58),le_u16(data,60)) else{return SymbolRead::default();};
 if shentsize!=64||shoff==0||shnum==0{return SymbolRead::default();}
 if shnum as usize>64{return SymbolRead{refusal:Some(CODE_SYMBOL_SECTION_CAP),..SymbolRead::default()};}
 let Some(table_end)=shoff.checked_add(u64::from(shnum).saturating_mul(64)) else{return SymbolRead::default();};
 if table_end>data.len() as u64{return SymbolRead::default();}
 let mut sections=Vec::new();
 for index in 0..u64::from(shnum){
  let Some(at)=shoff.checked_add(index.saturating_mul(64)).and_then(|at|usize::try_from(at).ok()) else{return SymbolRead::default();};
  let Some(kind)=le_u32(data,at.saturating_add(4)) else{return SymbolRead::default();};
  let Some(flags)=le_u64(data,at.saturating_add(8)) else{return SymbolRead::default();};
  let Some(offset)=le_u64(data,at.saturating_add(24)) else{return SymbolRead::default();};
  let Some(size)=le_u64(data,at.saturating_add(32)) else{return SymbolRead::default();};
  let Some(link)=le_u32(data,at.saturating_add(40)) else{return SymbolRead::default();};
  let Some(entry)=le_u64(data,at.saturating_add(56)) else{return SymbolRead::default();};
  sections.push((kind,flags,offset,size,link,entry));
 }
 let mut read=SymbolRead::default();let mut scanned=0usize;
 for &(kind,flags,offset,size,link,entry) in &sections{
  if !matches!(kind,2|11)||flags&0x800!=0||entry!=24||size%24!=0{continue;}
  let Some(&(3,string_flags,string_offset,string_size,_,_))=sections.get(link as usize) else{continue;};
  if string_flags&0x800!=0{continue;}
  let Some(strings)=elf_bytes(data,string_offset,string_size) else{continue;};
  if strings.first()!=Some(&0)||strings.last()!=Some(&0){continue;}
  let count=(size/24) as usize;
  for i in 0..count{
   scanned+=1;
   if scanned>CODE_SYMBOL_SCAN{read.refusal=Some(CODE_SYMBOL_SCAN_CAP);return read;}
   let Some(at)=offset.checked_add((i as u64).saturating_mul(24)).and_then(|at|usize::try_from(at).ok()) else{continue;};
   let (Some(name_off),Some(info),Some(section),Some(value),Some(sym_size))=(le_u32(data,at),data.get(at.saturating_add(4)).copied(),le_u16(data,at.saturating_add(6)),le_u64(data,at.saturating_add(8)),le_u64(data,at.saturating_add(16))) else{continue;};
   if info&15!=2||section==0||section>=0xff00||u64::from(section)>=u64::from(shnum){continue;}
   let Some(name)=usize::try_from(name_off).ok().and_then(|start|strings.get(start..)).and_then(display_symbol) else{continue;};
   if read.symbols.len()>=CODE_SYMBOL_MAX{read.refusal=Some(CODE_SYMBOL_CAP);return read;}
   read.symbols.push(CodeSymbol{name_display:name,value,size:sym_size});
  }
 }
 read
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
/// PE images declare no mapping-symbol data inventory, so no whole word is
/// excluded as declared data and every mapped word stays an encoding candidate.
pub(super) fn analyze(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool)->Result<Observation,String>{
 analyze_with_data(data,regions,inventory_complete,label,cancelled,&[])
}
/// Analyze the declared executable mappings and return the complete observation.
/// Whole words inside the supplied declared data intervals are excluded from the
/// candidate set; this is the implementation of the mapping observation.
pub(super) fn analyze_with_data(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool,data_ranges:&[(u64,u64)])->Result<Observation,String>{
 if cancelled(){return Err("Inspection cancelled during control-flow observations".into());}
 let mut out=String::from("AArch64 control-flow encoding observations\nScope: aligned encoding candidates in declared executable file-backed mappings. Coordinates identify declared ELF virtual addresses or relative PE RVAs. Instruction words use little-endian encoding. Bounds: 512 examined words, 64 reported candidates, 8191 report bytes.\n");
 if !inventory_complete{out.push_str("Mapping inventory incomplete or invalid; branch candidates withheld because unseen mappings may conflict.\nCross-references withheld with the branch candidates.\n");return Ok(Observation{text:out,coordinate:label.into(),regions:regions.to_vec(),mappings_complete:inventory_complete,..Default::default()});}
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
 let ir=super::instruction::observe(&decoded,label,cancelled)?;out.push('\n');out.push_str(&ir.text);
 if cancelled(){return Err("Inspection cancelled during control-flow observations".into());}
 let sites:Vec<(u64,Branch)>=decoded.iter().filter_map(|row|row.3.map(|branch|(row.1,branch))).collect();
 let declared=elf_function_symbols(data);
 let xrefs=cross_references(&sites,&ordered,&declared.symbols);
 out.push('\n');out.push_str(&xref_text(&xrefs));
 if let Some(reason)=declared.refusal{if !out.contains(reason){out.push_str(&format!("Refusal: {reason}.\n"));}}
 Ok(Observation{text:out,statements:ir.statements,coordinate:label.into(),regions:regions.to_vec(),mappings_complete:inventory_complete,xrefs,..Default::default()})
}
/// Observe the declared executable mappings and return the complete observation.
/// The observation is the mapping analysis itself; this named entry keeps the
/// binary observers readable.
pub(super) fn observe(data:&[u8],regions:&[Region],inventory_complete:bool,label:&str,cancelled:&dyn Fn()->bool,data_ranges:&[(u64,u64)])->Result<Observation,String>{
 analyze_with_data(data,regions,inventory_complete,label,cancelled,data_ranges)
}
#[cfg(test)]mod tests{
 use super::*;
 fn region()->Region{Region{offset:0,address:0x1000,file_size:0x100,memory_size:0x100,executable:true}}
 fn must(word:u32)->Branch{decode(word).unwrap_or_else(||panic!("test encoding {word:08x} was not a branch"))}
 #[test]fn bl_call_edge_names_resolved_target_and_region(){
  let set=cross_references(&[(0x1000,must(0x94000002))],&[region()],&[]);
  assert!(set.refusal.is_none());assert!(set.unknowns.is_empty());assert_eq!(set.edges.len(),1);
  let edge=&set.edges[0];
  assert_eq!((edge.from_pc,edge.to_pc,edge.kind),(0x1000,0x1008,XrefKind::Call));
  assert_eq!(edge.region,"region 0 address 0x1000 file offset 8 executable declaration");
  let caller=set.symbols.iter().find(|s|s.address==0x1000).unwrap();
  assert_eq!(caller.outgoing_calls,vec![0x1008]);assert!(caller.incoming.is_empty());assert_eq!(caller.unknown,0);
  let callee=set.symbols.iter().find(|s|s.address==0x1008).unwrap();
  assert!(callee.outgoing_calls.is_empty());assert_eq!(callee.incoming,vec![(0x1000,XrefKind::Call)]);
  assert!(xref_text(&set).contains("Edge call from 0x1000 to 0x1008"));
 }
 #[test]fn conditional_branch_is_not_counted_as_a_call(){
  let set=cross_references(&[(0x1000,must(0x54000040)),(0x1004,must(0x14000002)),(0x1008,must(0x34000040))],&[region()],&[]);
  assert!(set.refusal.is_none());assert!(set.edges.iter().all(|e|e.kind!=XrefKind::Call));
  assert_eq!(set.edges.iter().filter(|e|e.kind==XrefKind::ConditionalBranch).count(),2);
  assert_eq!(set.edges.iter().filter(|e|e.kind==XrefKind::Branch).count(),1);
  assert!(set.symbols.iter().all(|s|s.outgoing_calls.is_empty()));
 }
 #[test]fn indirect_br_blr_ret_are_unknown_with_a_reason(){
  let sites=[(0x1000,must(0xd61f0020)),(0x1004,must(0xd63f0040)),(0x1008,must(0xd65f03c0)),(0x100c,decode_version(0xd65f0bff,3).unwrap()),(0x1010,decode_version(0xd71f0864,3).unwrap()),(0x1014,decode_version(0xd73f0909,3).unwrap())];
  let set=cross_references(&sites,&[region()],&[]);
  assert!(set.edges.is_empty());assert_eq!(set.unknowns.len(),6);assert!(set.refusal.is_none());
  assert_eq!(set.unknowns[0].kind,XrefKind::Branch);assert!(set.unknowns[0].reason.contains("indirect branch"));
  assert_eq!(set.unknowns[1].kind,XrefKind::Call);assert!(set.unknowns[1].reason.contains("indirect call"));
  assert_eq!(set.unknowns[2].kind,XrefKind::Return);assert!(set.unknowns[2].reason.contains("return"));
  assert_eq!(set.unknowns[3].kind,XrefKind::Return);assert!(set.unknowns[3].reason.contains("authenticated"));
  assert!(set.unknowns[4].reason.contains("authenticated"));assert!(set.unknowns[5].reason.contains("authenticated"));
  assert!(set.symbols.iter().all(|s|s.outgoing_calls.is_empty()&&s.incoming.is_empty()));
  assert_eq!(set.symbols.iter().map(|s|s.unknown).sum::<u64>(),6);
 }
 #[test]fn edge_cap_refuses_at_xref_max_plus_one(){
  let sites=(0..=XREF_MAX).map(|i|(0x1000+i as u64*4,must(0x94000000|(i as u32+1)))).collect::<Vec<_>>();
  let set=cross_references(&sites,&[],&[]);
  assert_eq!(set.edges.len(),XREF_MAX);assert_eq!(set.refusal,Some(XREF_EDGE_CAP));
  assert!(set.edges.iter().all(|e|e.kind==XrefKind::Call&&e.region=="outside every mapped region"));
  let text=xref_text(&set);assert!(text.contains(XREF_EDGE_CAP));assert!(text.len()<=XREF_OUTPUT);
 }
 #[test]fn unresolved_direct_target_is_unknown_not_an_edge(){
  let set=cross_references(&[(u64::MAX-1,must(0x94000002))],&[region()],&[]);
  assert!(set.edges.is_empty());assert_eq!(set.unknowns.len(),1);
  assert!(set.unknowns[0].reason.contains("overflow"));assert_eq!(set.symbols[0].unknown,1);assert!(set.symbols[0].outgoing_calls.is_empty());
 }
 #[test]fn symbol_list_cap_and_ambiguous_names_refuse_to_guess(){
  let sites=(0..XREF_SYMBOL_LIST+1).map(|i|{let imm=0x400-i as u32;(0x1000+i as u64*4,must(0x94000000|imm))}).collect::<Vec<_>>();
  let set=cross_references(&sites,&[region()],&[]);
  assert_eq!(set.refusal,Some(XREF_LIST_CAP));
  let callee=set.symbols.iter().find(|s|s.address==0x2000).unwrap();
  assert_eq!(callee.incoming.len(),XREF_SYMBOL_LIST);assert_eq!(set.edges.len(),XREF_SYMBOL_LIST+1);
  let ambiguous=[CodeSymbol{name_display:"one".into(),value:0x1008,size:4},CodeSymbol{name_display:"two".into(),value:0x1008,size:4}];
  let named=cross_references(&[(0x1000,must(0x94000002))],&[region()],&ambiguous);
  assert!(named.symbols.iter().find(|s|s.address==0x1008).unwrap().name.is_none());
  let unique=[CodeSymbol{name_display:"callee".into(),value:0x1008,size:16}];
  let named=cross_references(&[(0x1000,must(0x94000002))],&[region()],&unique);
  assert_eq!(named.symbols.iter().find(|s|s.address==0x1008).unwrap().name.as_deref(),Some("callee"));
 }
 #[test]fn observe_records_the_call_edge_on_the_native_observation(){
  let mut data=vec![0u8;16];data[..4].copy_from_slice(&0x94000002u32.to_le_bytes());
  let obs=observe(&data,&[region()],true,"declared VA ",&||false,&[]).unwrap();
  let edge=obs.xrefs.edges.iter().find(|e|e.kind==XrefKind::Call).unwrap();
  assert_eq!((edge.from_pc,edge.to_pc),(0x1000,0x1008));
  assert!(edge.region.contains("executable declaration"));
  assert!(obs.text.contains("Edge call from 0x1000 to 0x1008"));
  assert!(obs.text.contains("outgoing calls 1"));
 }
 #[test]fn elf_function_symbol_is_named_and_the_record_cap_refuses(){
  fn image(count:usize)->Vec<u8>{
   let shoff=64u64;let sym_off=shoff+3*64;let str_off=sym_off+count as u64*24;let mut data=vec![0u8;str_off as usize+6];
   data[..4].copy_from_slice(b"\x7fELF");data[4]=2;data[5]=1;data[6]=1;data[18..20].copy_from_slice(&183u16.to_le_bytes());
   data[40..48].copy_from_slice(&shoff.to_le_bytes());data[58..60].copy_from_slice(&64u16.to_le_bytes());data[60..62].copy_from_slice(&3u16.to_le_bytes());
   let put=|data:&mut [u8],at:usize,kind:u32,offset:u64,size:u64,link:u32,entry:u64|{data[at+4..at+8].copy_from_slice(&kind.to_le_bytes());data[at+24..at+32].copy_from_slice(&offset.to_le_bytes());data[at+32..at+40].copy_from_slice(&size.to_le_bytes());data[at+40..at+44].copy_from_slice(&link.to_le_bytes());data[at+56..at+64].copy_from_slice(&entry.to_le_bytes());};
   put(&mut data,shoff as usize+64,2,sym_off,count as u64*24,2,24);put(&mut data,shoff as usize+128,3,str_off,6,0,0);
   data[str_off as usize]=0;data[str_off as usize+1..str_off as usize+5].copy_from_slice(b"main");data[str_off as usize+5]=0;
   for i in 0..count{let at=sym_off as usize+i*24;data[at..at+4].copy_from_slice(&1u32.to_le_bytes());data[at+4]=2;data[at+6..at+8].copy_from_slice(&1u16.to_le_bytes());data[at+8..at+16].copy_from_slice(&(0x1008+i as u64*4).to_le_bytes());data[at+16..at+24].copy_from_slice(&4u64.to_le_bytes());}
   data
  }
  let read=elf_function_symbols(&image(1));assert_eq!(read.refusal,None);assert_eq!(read.symbols.len(),1);assert_eq!(read.symbols[0].name_display,"main");assert_eq!(read.symbols[0].value,0x1008);
  let set=cross_references(&[(0x1000,must(0x94000002))],&[region()],&read.symbols);
  assert_eq!(set.symbols.iter().find(|s|s.address==0x1008).unwrap().name.as_deref(),Some("main"));
  let capped=elf_function_symbols(&image(CODE_SYMBOL_MAX+1));assert_eq!(capped.symbols.len(),CODE_SYMBOL_MAX);assert_eq!(capped.refusal,Some(CODE_SYMBOL_CAP));
  assert!(elf_function_symbols(&[0x94,0,0,0]).symbols.is_empty());
 }
 #[test]fn encoded_bl_signs_agree_and_record_the_same_address(){
  let offset_of=|branch:Branch|->i64{match branch{Branch::Direct{offset,..}=>offset,_=>panic!("test encoding was not direct")}};
  let positive=must(0x94000001);let negative=must(0x97ff_ffff);
  let (pos,neg)=(offset_of(positive),offset_of(negative));
  assert_eq!((pos,neg),(4,-4));
  let sites=[(0x1000u64,positive),(0x1004,negative)];
  for &(pc,branch) in &sites{
   let offset=offset_of(branch);let agreed=displaced(pc,offset);
   assert_eq!(agreed,super::super::instruction::coordinate_target(pc,offset));
   assert!(agreed.is_some());
  }
  let set=cross_references(&sites,&[region()],&[]);
  assert!(set.refusal.is_none()&&set.unknowns.is_empty()&&set.edges.len()==2);
  assert_eq!((set.edges[0].from_pc,set.edges[0].to_pc,set.edges[0].kind),(0x1000,0x1004,XrefKind::Call));
  assert_eq!((set.edges[1].from_pc,set.edges[1].to_pc,set.edges[1].kind),(0x1004,0x1000,XrefKind::Call));
  let text=xref_text(&set);
  assert!(text.contains("Edge call from 0x1000 to 0x1004")&&text.contains("Edge call from 0x1004 to 0x1000"));
  assert!(!text.contains("disagree"));
  for word in [0x9400_0000u32,0x95ff_ffff,0x9600_0000,0x97ff_ffff]{
   let offset=offset_of(must(word));
   for pc in [0u64,0x1000,u64::MAX]{assert_eq!(displaced(pc,offset),super::super::instruction::coordinate_target(pc,offset));}
  }
  let far_offset=offset_of(must(0x95ff_ffff));
  let far_to=displaced(0x1000,far_offset).unwrap();
  assert_eq!(far_to,super::super::instruction::coordinate_target(0x1000,far_offset).unwrap());
  let far=cross_references(&[(0x1000,must(0x95ff_ffff))],&[region()],&[]);
  assert!(far.unknowns.is_empty()&&far.edges.len()==1&&far.edges[0].to_pc==far_to&&!far.edges[0].region.is_empty());
  let overflow=cross_references(&[(0x1000,must(0x9600_0000))],&[region()],&[]);
  assert!(overflow.edges.is_empty()&&overflow.unknowns.len()==1);
  assert!(overflow.unknowns[0].reason.contains("overflow")&&!overflow.unknowns[0].reason.contains("disagree"));
 }
 #[test]fn unknown_cap_names_the_refusal_in_the_report(){
  let branch=must(0xd61f0020);
  let under=(0..XREF_MAX).map(|i|(0x1000+i as u64*4,branch)).collect::<Vec<_>>();
  let under_set=cross_references(&under,&[region()],&[]);
  assert_eq!(under_set.unknowns.len(),XREF_MAX);assert!(under_set.refusal.is_none());
  assert!(!xref_text(&under_set).contains(XREF_UNKNOWN_CAP));
  let over=(0..=XREF_MAX).map(|i|(0x1000+i as u64*4,branch)).collect::<Vec<_>>();
  let set=cross_references(&over,&[region()],&[]);
  assert!(set.edges.is_empty());assert_eq!(set.unknowns.len(),XREF_MAX);assert_eq!(set.refusal,Some(XREF_UNKNOWN_CAP));
  let text=xref_text(&set);
  assert!(text.contains("Refusal: cross-reference unknown cap."));
  assert!(text.contains("stored unknowns 64"));
  assert!(text.len()<=XREF_OUTPUT);
 }
 #[test]fn symbol_list_cap_names_the_refusal_in_the_report(){
  let sites=(0..XREF_SYMBOL_LIST+1).map(|i|{let imm=0x400-i as u32;(0x1000+i as u64*4,must(0x94000000|imm))}).collect::<Vec<_>>();
  let set=cross_references(&sites,&[region()],&[]);
  assert_eq!(set.refusal,Some(XREF_LIST_CAP));assert_eq!(set.edges.len(),XREF_SYMBOL_LIST+1);
  let callee=set.symbols.iter().find(|s|s.address==0x2000).unwrap();
  assert_eq!(callee.incoming.len(),XREF_SYMBOL_LIST);
  let text=xref_text(&set);
  assert!(text.contains("Refusal: cross-reference symbol list cap."));
  assert!(text.contains("incoming references 8 ("));
  assert!(!text.contains(XREF_OUTPUT_CAP));
  assert!(text.len()<=XREF_OUTPUT);
 }
 #[test]fn output_cap_names_itself_when_the_report_is_cut(){
  let name="callee-name-".repeat(400);
  let symbols=[CodeSymbol{name_display:name.clone(),value:0x1004,size:4}];
  let set=cross_references(&[(0x1000,must(0x94000001))],&[region()],&symbols);
  assert!(set.refusal.is_none());assert_eq!(set.edges.len(),1);assert_eq!(set.symbols.len(),2);
  let text=xref_text(&set);
  assert!(text.contains("\ncross-reference output cap\n"));
  assert!(text.len()<=XREF_OUTPUT);
  assert!(!text.contains(&name));
  assert_eq!(text.lines().filter(|line|line.starts_with("Symbol ")).count(),1);
  assert!(text.contains("symbols 2."));
 }
 #[test]fn later_caps_are_named_beside_the_first_refusal(){
  let base=0x1_0000_0000_0000u64;let target=base+(XREF_MAX as u64+2)*4;let mut sites=Vec::new();
  for i in 0..=XREF_MAX{let pc=base+i as u64*4;let imm=((target-pc)/4) as u32;sites.push((pc,must(0x9400_0000|imm)));}
  for i in 0..=XREF_MAX{sites.push((0x2_0000_0000_0000+i as u64*4,must(0xd61f0020)));}
  let set=cross_references(&sites,&[],&[]);
  assert_eq!(set.refusal,Some(XREF_EDGE_CAP));
  assert_eq!(set.edges.len(),XREF_MAX);assert_eq!(set.unknowns.len(),XREF_MAX);
  let callee=set.symbols.iter().find(|s|s.address==target).unwrap();
  assert_eq!(callee.incoming.len(),XREF_SYMBOL_LIST);
  let text=xref_text(&set);
  assert!(text.contains("Refusal: cross-reference edge cap."));
  assert!(text.contains("Refusal: cross-reference unknown cap."));
  assert!(text.contains("Refusal: cross-reference symbol list cap."));
  assert!(text.contains("\ncross-reference output cap\n"));
  assert!(text.len()<=XREF_OUTPUT);
 }
}
