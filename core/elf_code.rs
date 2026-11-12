//! ELF code/data declarations and relocation records from the held source only.
use super::*;
const RECORDS:u64=2048;
const RELOCATIONS:u64=128;
#[derive(Default)]pub(super) struct Context{pub data:Vec<(u64,u64)>,pub report:String,pub classifications:Vec<super::super::control_flow::Classification>,pub classifications_complete:bool,pub relocations:Vec<super::super::control_flow::Relocation>,pub relocations_complete:bool,pub symbols:Vec<super::super::control_flow::CodeSymbol>}
fn mapping_name(name:&str)->Option<bool>{if name=="$x"||name.starts_with("$x."){Some(true)}else if name=="$d"||name.starts_with("$d."){Some(false)}else{None}}
fn relocation_name(kind:u32)->&'static str{match kind{0=>"NONE",257=>"ABS64",258=>"ABS32",259=>"ABS16",260=>"PREL64",261=>"PREL32",262=>"PREL16",273=>"LD_PREL_LO19",274=>"ADR_PREL_LO21",275=>"ADR_PREL_PG_HI21",276=>"ADR_PREL_PG_HI21_NC",277=>"ADD_ABS_LO12_NC",278=>"LDST8_ABS_LO12_NC",279=>"TSTBR14",280=>"CONDBR19",282=>"JUMP26",283=>"CALL26",284=>"LDST16_ABS_LO12_NC",285=>"LDST32_ABS_LO12_NC",286=>"LDST64_ABS_LO12_NC",299=>"LDST128_ABS_LO12_NC",1024=>"COPY",1025=>"GLOB_DAT",1026=>"JUMP_SLOT",1027=>"RELATIVE",1032=>"IRELATIVE",_=>"uninterpreted relocation code"}}
fn symbol(reader:&Reader<'_>,header:&Header,table:Section,index:u64)->Option<(String,u8,u16,u64)>{
 if !matches!(table.kind,2|11)||table.entry<24||table.size%table.entry!=0||index>=table.size/table.entry||table.flags&0x800!=0{return None;}
 reader.extent(table.offset,table.size).ok()?;let at=table.offset.checked_add(index.checked_mul(table.entry)?)?;reader.slice(at,24)?;
 let name=named(string_section(reader,header,table.link).ok()?,reader.u32(at)?as u64).ok()?;
 Some((name,reader.slice(at+4,1)?[0],reader.u16(at+6)?,reader.u64(at+8)?))
}
fn line(out:&mut String,row:String){if out.len()+row.len()+1<10800{out.push_str(&row);out.push('\n');}}
fn publish_code_symbols(data:&[u8],out:&mut String)->Vec<super::super::control_flow::CodeSymbol>{
 let read=super::super::control_flow::elf_function_symbols(data);
 for symbol in &read.symbols{line(out,format!("Code symbol \"{}\" at declared value 0x{:x}, size {}. Outgoing calls, incoming references and unknown counts for this address are in the decoded-word cross-reference section.",symbol.name_display,symbol.value,symbol.size));}
 if let Some(reason)=read.refusal{line(out,format!("Refusal: {reason}."));}
 read.symbols
}
pub(super) fn inspect(reader:&Reader<'_>,header:&Header,kind:u16,sh_valid:bool,regions:&[super::super::control_flow::Region],cancelled:&impl Fn()->bool)->Result<Context,String>{
 let mut out=String::from("ELF AArch64 code/data and relocation declarations\nInput: original held source bytes. Mapping classifications require a complete bounded symbol inventory; missing/stripped metadata keeps encoding candidates available, with code/data unknown. Bounds:64 section headers,2048 total mapping-symbol records,128 relocation records,256-byte names, independent12KiB report.\n");
 if !matches!(kind,1|2|3){return Ok(Context{report:out+"ELF type is outside relocatable/executable/shared-object metadata scope; mapping-symbol and relocation semantics unresolved.\n",..Default::default()});}
 if !sh_valid||header.shnum>TABLE_LIMIT{return Ok(Context{report:out+"Section inventory unavailable or bounded; code/data classification and relocation records unresolved.\n",..Default::default()});}
 let mut sections=Vec::new();for i in 0..header.shnum{if cancelled(){return Err("Inspection cancelled during ELF code metadata".into());}let Some(s)=section(reader,header,i)else{return Ok(Context{report:out+"Section inventory incomplete; code/data classification and relocation records unresolved.\n",..Default::default()});};sections.push(s);}
 let mut markers=Vec::new();let mut records=0u64;let mut complete=true;let mut tables=0;
 for s in &sections{
  if !matches!(s.kind,2|11){continue;}tables+=1;
  if s.flags&0x800!=0||s.entry<24||s.size%s.entry!=0||reader.extent(s.offset,s.size).is_err()||reader.slice(s.offset,s.size).is_none(){complete=false;continue;}
  let strings=match string_section(reader,header,s.link){Ok(t)=>t,Err(_)=>{complete=false;continue;}};
  let count=s.size/s.entry;
  for i in 0..count{
   if records>=RECORDS{complete=false;break;}if records%16==0&&cancelled(){return Err("Inspection cancelled during ELF mapping symbols".into());}records+=1;
   let at=s.offset+i*s.entry;let name=match named(strings,reader.u32(at).unwrap()as u64){Ok(v)=>v,Err(_)=>{complete=false;continue;}};
   let Some(code)=mapping_name(&name)else{continue;};let info=reader.slice(at+4,1).unwrap()[0];let index=reader.u16(at+6).unwrap()as usize;let value=reader.u64(at+8).unwrap();let size=reader.u64(at+16).unwrap();
   if info!=0||size!=0||index==0||index>=sections.len(){complete=false;continue;}let target=sections[index];
   if target.kind!=1||target.flags&0x800!=0||target.flags&6!=6||!pow2_or_small(target.align)||reader.extent(target.offset,target.size).is_err(){complete=false;continue;}
   let Some(end)=target.address.checked_add(target.size)else{complete=false;continue;};
   if !matches!(kind,2|3)||value<target.address||value>=end{complete=false;continue;}
   markers.push((index,value,code));
  }
 }
 markers.sort();markers.dedup();
 if markers.windows(2).any(|p|p[0].0==p[1].0&&p[0].1==p[1].1&&p[0].2!=p[1].2){complete=false;}
 let mut spans=Vec::new();if complete{for(i,&(section,start,code))in markers.iter().enumerate(){let end=markers.get(i+1).filter(|next|next.0==section).map(|n|n.1).unwrap_or(sections[section].address+sections[section].size);if start<end{spans.push((start,end,code,section));}}}
 // Section aliases may contradict only part of a word. A data exclusion must
 // agree with every intersecting classified interval and file mapping.
 let mut data=Vec::new();let mut classifications=Vec::new();for &(start,end,code,index)in &spans{
  line(&mut out,format!("Mapping declaration section{index}: [0x{start:x},0x{end:x}) {}.",if code{"A64 code"}else{"data/literal pool"}));
  let s=sections[index];let expected=s.offset.checked_add(start-s.address);
  let agrees=super::super::control_flow::mapped(regions,start,end-start).ok().is_some_and(|(off,exec)|exec&&Some(off)==expected);
  let contradicted=spans.iter().any(|&(a,b,c,_)|c!=code&&a<end&&start<b);let excludes=!code&&agrees&&!contradicted;
  classifications.push(super::super::control_flow::Classification{section:index as u64,start,end,code,mapping_agrees:agrees,contradicted,excludes_whole_data_words:excludes});if excludes{data.push((start,end));}
 }
 line(&mut out,format!("Mapping symbols: inspected{records} records across{tables} tables, {} markers; complete inventory{complete}; {} uncontradicted data intervals. Only whole words in declared data intervals can be excluded; all other bytes remain candidates, not proven code.",markers.len(),data.len()));
 let mut relocations=0;let mut incomplete=false;let mut facts=Vec::new();
 for(index,s)in sections.iter().enumerate(){
  if !matches!(s.kind,4|9){continue;}let minimum=if s.kind==4{24}else{16};
  if s.flags&0x800!=0||s.entry<minimum||s.size%s.entry!=0||reader.extent(s.offset,s.size).is_err()||reader.slice(s.offset,s.size).is_none(){incomplete=true;line(&mut out,format!("Relocation section{index} incomplete, compressed or malformed; records withheld."));continue;}
  for i in 0..s.size/s.entry{
   if relocations>=RELOCATIONS{incomplete=true;break;}if relocations%16==0&&cancelled(){return Err("Inspection cancelled during ELF relocations".into());}relocations+=1;
   let at=s.offset+i*s.entry;let address=reader.u64(at).unwrap();let info=reader.u64(at+8).unwrap();let code=info as u32;let si=info>>32;let addend=if s.kind==4{Some(reader.u64(at+16).unwrap()as i64)}else{None};
   let sym=sections.get(s.link as usize).and_then(|t|symbol(reader,header,*t,si));
   facts.push(super::super::control_flow::Relocation{section:index as u64,record:i,file_offset:at,position:address,section_relative:kind==1,target_section:s.info,code,name:relocation_name(code),symbol_index:si,symbol:sym.as_ref().map(|(name,info,section,value)|super::super::control_flow::Symbol{name_display:name.clone(),info:*info,section:*section,value:*value}),addend});
   let position=if kind==1{format!("section{} relative offset0x{address:x}; link placement unresolved",s.info)}else{format!("declared VA0x{address:x}; no runtime image loaded")};
   let symbol_text=match &sym{Some((name,info,section,value))=>format!("symbol{si} \"{name}\", binding{}, type{}, section{section}, declared value0x{value:x}",info>>4,info&15),None=>format!("symbol{si} unresolved (linked table/index/name unavailable)")};
   line(&mut out,format!("Relocation section{index} record{i}: {}({code}) at {position}; {}; {symbol_text}.",relocation_name(code),match addend{Some(a)=>format!("explicit signed addend{a}"),None=>"implicit addend not decoded".into()}));
   if matches!(code,282|283)&&matches!(kind,2|3){if let(Some((_,info,section,value)),Some(addend))=(&sym,addend){
    if *section!=0&&(*section as u64)<header.shnum&&matches!(info&15,0|1|2){
     let target=if addend<0{value.checked_sub(addend.unsigned_abs())}else{value.checked_add(addend as u64)};
     if let Some(target)=target{let displacement=target as i128-address as i128;
      line(&mut out,format!("  Declared S+A candidate0x{target:x}; S+A-P={displacement}; aligned26-bit branch displacement fits{}. This is relocation arithmetic, not symbol binding, a patched instruction, or a resolved runtime call.",displacement%4==0&&(-(1i128<<27)..(1i128<<27)).contains(&displacement)));
     }
    }
   }}
  }
 }
 let _=writeln!(out,"Observed {relocations} relocation records; unavailable/bounded tables{incomplete}. Report rows may be omitted under the12KiB bound. Unobserved/stripped sections do not establish absence. Dynamic relocation streams, implicit addend extraction, linker relaxation, symbol interposition, IFUNC/TLS, runtime fixups and dependency resolution remain unresolved. All graph/IR facts describe stored encodings before any relocation.");
 if cancelled(){return Err("Inspection cancelled during ELF code symbols".into());}
 let symbols=publish_code_symbols(reader.bytes,&mut out);
 Ok(Context{data,report:out,classifications,classifications_complete:complete,relocations:facts,relocations_complete:!incomplete,symbols})
}
#[cfg(test)]mod tests{
 use super::*;
 fn function_image()->Vec<u8>{
  let shoff=64u64;let sym_off=shoff+3*64;let str_off=sym_off+24;let mut data=vec![0u8;str_off as usize+6];
  data[..4].copy_from_slice(b"\x7fELF");data[4]=2;data[5]=1;data[6]=1;data[18..20].copy_from_slice(&183u16.to_le_bytes());
  data[40..48].copy_from_slice(&shoff.to_le_bytes());data[58..60].copy_from_slice(&64u16.to_le_bytes());data[60..62].copy_from_slice(&3u16.to_le_bytes());
  let put=|data:&mut [u8],at:usize,kind:u32,offset:u64,size:u64,link:u32,entry:u64|{data[at+4..at+8].copy_from_slice(&kind.to_le_bytes());data[at+24..at+32].copy_from_slice(&offset.to_le_bytes());data[at+32..at+40].copy_from_slice(&size.to_le_bytes());data[at+40..at+44].copy_from_slice(&link.to_le_bytes());data[at+56..at+64].copy_from_slice(&entry.to_le_bytes());};
  put(&mut data,shoff as usize+64,2,sym_off,24,2,24);put(&mut data,shoff as usize+128,3,str_off,6,0,0);
  data[sym_off as usize..sym_off as usize+4].copy_from_slice(&1u32.to_le_bytes());data[sym_off as usize+4]=2;data[sym_off as usize+6..sym_off as usize+8].copy_from_slice(&1u16.to_le_bytes());
  data[sym_off as usize+8..sym_off as usize+16].copy_from_slice(&0x1008u64.to_le_bytes());data[sym_off as usize+16..sym_off as usize+24].copy_from_slice(&4u64.to_le_bytes());
  data[str_off as usize]=0;data[str_off as usize+1..str_off as usize+5].copy_from_slice(b"main");data[str_off as usize+5]=0;data
 }
 #[test]fn code_symbol_line_names_the_address_used_by_cross_references(){
  let mut out=String::new();let symbols=publish_code_symbols(&function_image(),&mut out);
  assert_eq!(symbols.len(),1);assert_eq!(symbols[0].name_display,"main");assert_eq!(symbols[0].value,0x1008);
  assert!(out.contains("Code symbol \"main\" at declared value 0x1008"));
  assert!(out.contains("decoded-word cross-reference section"));
  let mut empty=String::new();assert!(publish_code_symbols(b"\x7fELF", &mut empty).is_empty());assert!(empty.is_empty());
 }
}
