//! Original bounded ELF envelope and declared-linkage observations, never a loader.
use std::fmt::Write as _;
const INPUT_LIMIT:usize=128*1024;
const TABLE_LIMIT:u64=64;
const SYMBOL_TABLE_LIMIT:usize=4;
const SYMBOL_LIMIT:u64=128;
const DYNAMIC_TABLE_LIMIT:usize=4;
const DYNAMIC_LIMIT:usize=128;
const NAME_SCAN:usize=256;
const OUTPUT_LIMIT:usize=8191;

#[derive(Clone,Copy)]enum Endian{Little,Big}
struct Reader<'a>{bytes:&'a [u8],endian:Endian,total:Option<u64>}
impl Reader<'_>{
 fn slice(&self,offset:u64,length:u64)->Option<&[u8]>{let end=offset.checked_add(length)?;if end>self.bytes.len() as u64{return None;}self.bytes.get(offset as usize..end as usize)}
 fn u16(&self,at:u64)->Option<u16>{let b:[u8;2]=self.slice(at,2)?.try_into().ok()?;Some(match self.endian{Endian::Little=>u16::from_le_bytes(b),Endian::Big=>u16::from_be_bytes(b)})}
 fn u32(&self,at:u64)->Option<u32>{let b:[u8;4]=self.slice(at,4)?.try_into().ok()?;Some(match self.endian{Endian::Little=>u32::from_le_bytes(b),Endian::Big=>u32::from_be_bytes(b)})}
 fn u64(&self,at:u64)->Option<u64>{let b:[u8;8]=self.slice(at,8)?.try_into().ok()?;Some(match self.endian{Endian::Little=>u64::from_le_bytes(b),Endian::Big=>u64::from_be_bytes(b)})}
 fn extent(&self,offset:u64,length:u64)->Result<u64,&'static str>{let end=offset.checked_add(length).ok_or("offset/length overflow")?;if self.total.map(|n|end>n).unwrap_or(false){return Err("extent exceeds reported file length (truncated source or malformed fields)");}Ok(end)}
}
#[derive(Clone,Copy)]struct Header{wide:bool,phoff:u64,phsize:u64,phnum:u64,shoff:u64,shsize:u64,shnum:u64,names:u64}
#[derive(Clone,Copy)]struct Section{name:u32,kind:u32,flags:u64,address:u64,offset:u64,size:u64,link:u32,info:u32,align:u64,entry:u64}
#[derive(Clone,Copy)]struct Load{offset:u64,address:u64,file_size:u64,memory_size:u64}
#[derive(Clone,Copy)]struct Dynamic{offset:u64,size:u64,string_section:Option<u32>}
fn section(reader:&Reader<'_>,header:&Header,index:u64)->Option<Section>{
 if index>=header.shnum{return None;}let at=header.shoff.checked_add(header.shsize.checked_mul(index)?)?;reader.slice(at,header.shsize)?;
 Some(if header.wide{Section{name:reader.u32(at)?,kind:reader.u32(at+4)?,flags:reader.u64(at+8)?,address:reader.u64(at+16)?,offset:reader.u64(at+24)?,size:reader.u64(at+32)?,link:reader.u32(at+40)?,info:reader.u32(at+44)?,align:reader.u64(at+48)?,entry:reader.u64(at+56)?}}
 else{Section{name:reader.u32(at)?,kind:reader.u32(at+4)?,flags:reader.u32(at+8)? as u64,address:reader.u32(at+12)? as u64,offset:reader.u32(at+16)? as u64,size:reader.u32(at+20)? as u64,link:reader.u32(at+24)?,info:reader.u32(at+28)?,align:reader.u32(at+32)? as u64,entry:reader.u32(at+36)? as u64}})
}
fn table_extent(reader:&Reader<'_>,offset:u64,stride:u64,count:u64,minimum:u64,label:&str,out:&mut String)->bool{
 if count==0{return true;}if offset==0{let _=writeln!(out,"{label}: nonzero count with zero table offset; entries withheld.");return false;}
 if stride<minimum{let _=writeln!(out,"{label}: entry size {stride} is below the class minimum {minimum}; entries withheld.");return false;}
 let size=match stride.checked_mul(count){Some(v)=>v,None=>{let _=writeln!(out,"{label}: table-size multiplication overflow; entries withheld.");return false;}};
 match reader.extent(offset,size){Ok(end)=>{let _=writeln!(out,"{label}: declared {count} entries, stride {stride}, byte extent [{offset},{end}); at most {TABLE_LIMIT} headers inspected. Extra entry bytes, if any, are not interpreted.");true},Err(reason)=>{let _=writeln!(out,"{label}: {reason}; entries withheld.");false}}
}
fn escaped(bytes:&[u8])->String{let mut out=String::new();for b in bytes{let part=if (b' '..=b'~').contains(b)&&!matches!(*b,b'"'|b'\\'){(*b as char).to_string()}else{format!("\\x{b:02x}")};if out.len()+part.len()>128{out.push_str("…");break;}out.push_str(&part);}out}
fn string_table<'a>(reader:&'a Reader<'_>,s:Section)->Result<&'a [u8],&'static str>{
 if s.kind!=3{return Err("linked section is not a string table");}if s.flags&0x800!=0{return Err("compressed string table is not interpreted");}reader.extent(s.offset,s.size)?;
 let bytes=reader.slice(s.offset,s.size).ok_or("string table is not fully available in the examined prefix")?;
 if bytes.is_empty()||bytes.first()!=Some(&0)||bytes.last()!=Some(&0){return Err("string table lacks required initial/final NUL bytes");}Ok(bytes)
}
fn named(table:&[u8],offset:u64)->Result<String,&'static str>{
 if offset>=table.len() as u64{return Err("name offset is outside its string table");}
 let start=offset as usize;let sample=&table[start..table.len().min(start.saturating_add(NAME_SCAN))];
 let end=sample.iter().position(|b|*b==0).ok_or("name has no NUL within the 256-byte observation bound")?;
 Ok(escaped(&sample[..end]))
}
fn label(table:Option<&[u8]>,offset:u32)->String{match table{Some(t)=>match named(t,offset as u64){Ok(name)=>format!("\"{name}\""),Err(reason)=>format!("name unavailable ({reason}; string offset {offset})")},None=>format!("name unavailable (string offset {offset})")}}
fn address_end(address:u64,size:u64,wide:bool)->Option<u64>{let end=address.checked_add(size)?;if !wide&&end>0x1_0000_0000{return None;}Some(end)}
fn pow2_or_small(n:u64)->bool{n==0||n.is_power_of_two()}
fn machine(n:u16)->&'static str{match n{3=>"x86",8=>"MIPS",20=>"PowerPC",21=>"PowerPC64",40=>"Arm",62=>"x86-64",183=>"AArch64",243=>"RISC-V",_=>"unrecognized machine identifier"}}
fn program_type(n:u32)->&'static str{match n{0=>"NULL",1=>"LOAD",2=>"DYNAMIC",3=>"INTERP",4=>"NOTE",6=>"PHDR",7=>"TLS",_=>"other/extension"}}
fn section_type(n:u32)->&'static str{match n{0=>"NULL",1=>"PROGBITS",2=>"SYMTAB",3=>"STRTAB",4=>"RELA",5=>"HASH",6=>"DYNAMIC",7=>"NOTE",8=>"NOBITS",9=>"REL",11=>"DYNSYM",14=>"INIT_ARRAY",15=>"FINI_ARRAY",17=>"GROUP",18=>"SYMTAB_SHNDX",_=>"other/extension"}}
fn string_section<'a>(reader:&'a Reader<'_>,header:&Header,index:u32)->Result<&'a [u8],&'static str>{let target=section(reader,header,index as u64).ok_or("linked string-section header is outside the table or sampled prefix")?;string_table(reader,target)}
fn symbols(reader:&Reader<'_>,header:&Header,index:u64,s:Section,out:&mut String)->usize{
 let minimum=if header.wide{24}else{16};let count=if s.entry>=minimum&&s.entry!=0&&s.size%s.entry==0{s.size/s.entry}else{let _=writeln!(out,"Symbol table section {index}: invalid entry size or non-integral table size; candidates withheld.");return 0;};
 if reader.extent(s.offset,s.size).is_err()||reader.slice(s.offset,s.size).is_none(){let _=writeln!(out,"Symbol table section {index}: full declared table is outside the source or examined prefix; candidates withheld.");return 0;}
 let strings=match string_section(reader,header,s.link){Ok(v)=>v,Err(reason)=>{let _=writeln!(out,"Symbol table section {index}: {reason}; named candidates withheld.");return 0;}};
 let _=writeln!(out,"Symbol table section {index}: {count} declared records; inspecting at most {SYMBOL_LIMIT}; binding/visibility are declarations, not proof of linked or callable code.");
 let mut found=0;
 for i in 0..count.min(SYMBOL_LIMIT){
  let at=match s.entry.checked_mul(i).and_then(|d|s.offset.checked_add(d)){Some(v)=>v,None=>{out.push_str("Symbol offset overflow; stopped.\n");break;}};
  let name=reader.u32(at).unwrap();let(info,other,section_index,value,size)=if header.wide{(reader.slice(at+4,1).unwrap()[0],reader.slice(at+5,1).unwrap()[0],reader.u16(at+6).unwrap(),reader.u64(at+8).unwrap(),reader.u64(at+16).unwrap())}else{(reader.slice(at+12,1).unwrap()[0],reader.slice(at+13,1).unwrap()[0],reader.u16(at+14).unwrap(),reader.u32(at+4).unwrap() as u64,reader.u32(at+8).unwrap() as u64)};
  let binding=info>>4;let kind=info&15;let visibility=other&3;
  if !matches!(binding,1|2|10)||matches!(kind,3|4)||name==0{continue;}
  if section_index==0xffff{let _=writeln!(out,"Symbol {i}: extended section index requires a companion index table; classification withheld.");continue;}
  let defined=section_index!=0;
  if defined&&!((section_index as u64)<header.shnum||matches!(section_index,0xfff1|0xfff2)){let _=writeln!(out,"Symbol {i}: section index {section_index} is outside declared/recognized ranges; classification withheld.");continue;}
  if address_end(value,size,header.wide).is_none(){let _=writeln!(out,"Symbol {i}: declared value/size overflow; classification withheld.");continue;}
  let name=match named(strings,name as u64){Ok(name) if !name.is_empty()=>name,_=>{let _=writeln!(out,"Symbol {i}: name unavailable under string bounds; named classification withheld.");continue;}};
  let class=if section_index==0xfff2{"common symbol candidate (tentative allocation, not established export)"}else if !defined{if s.kind==11{"dynamic import candidate (undefined symbol)"}else{"undefined global/weak symbol candidate (linkage unresolved)"}}else if s.kind==11&&matches!(visibility,0|3){"dynamic export candidate (declared definition)"}else{"defined global/weak symbol candidate (not established as a dynamic export)"};
  let _=writeln!(out,"Symbol {i}: {class}; name \"{name}\"; binding {binding}; type {kind}; visibility {visibility}; section {section_index}; declared value 0x{value:x}; size {size}.");found+=1;
 }
 if count>SYMBOL_LIMIT{out.push_str("Remaining symbol records were not classified under the symbol work bound.\n");}found
}
fn mapped(loads:&[Load],address:u64,size:u64)->Result<u64,&'static str>{
 let mut match_offset=None;
 for load in loads{if address<load.address{continue;}let delta=address-load.address;if delta>load.file_size||size>load.file_size-delta{continue;}let offset=load.offset.checked_add(delta).ok_or("load mapping overflow")?;if match_offset.map(|old|old!=offset).unwrap_or(false){return Err("ambiguous overlapping load mappings");}match_offset=Some(offset);}
 match_offset.ok_or("virtual string-table extent has no observed file-backed LOAD mapping")
}
fn dynamic(reader:&Reader<'_>,header:&Header,entry:Dynamic,loads:&[Load],out:&mut String)->usize{
 let stride=if header.wide{16u64}else{8};
 if entry.size%stride!=0||reader.extent(entry.offset,entry.size).is_err()||reader.slice(entry.offset,entry.size).is_none(){out.push_str("Dynamic table is incomplete, out of file bounds, or not an integral entry count; named dependencies withheld.\n");return 0;}
 let count=entry.size/stride;let mut string_address=None;let mut string_size=None;let mut ambiguous=false;let mut names=Vec::new();let mut terminated=false;let mut observed=0;let mut names_omitted=0;
 for i in 0..count.min(DYNAMIC_LIMIT as u64){let at=entry.offset+i*stride;let(tag,value)=if header.wide{(reader.u64(at).unwrap() as i64,reader.u64(at+8).unwrap())}else{(reader.u32(at).unwrap() as i32 as i64,reader.u32(at+4).unwrap() as u64)};observed+=1;if tag==0{terminated=true;break;}match tag{5=>{if string_address.replace(value).is_some(){ambiguous=true;}},10=>{if string_size.replace(value).is_some(){ambiguous=true;}},1|14|15|29=>{if names.len()<32{names.push((tag,value));}else{names_omitted+=1;}},_=>{}}}
 let _=writeln!(out,"Dynamic table at file offset {}: inspected {observed} entries (cap {DYNAMIC_LIMIT}); DT_NULL terminator {}. Unreported tags and payload semantics are not validated.",entry.offset,if terminated{"observed"}else{"not observed"});
 if names_omitted>0{let _=writeln!(out,"Omitted {names_omitted} additional named dynamic references under the 32-name bound.");}
 if !terminated||ambiguous{out.push_str("A bounded terminator and unambiguous string-table metadata are required for named dynamic observations; names withheld.\n");return 0;}
 let strings=if let Some(index)=entry.string_section{
  if let (Some(address),Some(size))=(string_address,string_size){
   let target=section(reader,header,index as u64);
   if target.map(|s|s.size!=size).unwrap_or(true)||(!loads.is_empty()&&mapped(loads,address,size).ok().zip(target).map(|(offset,s)|offset!=s.offset).unwrap_or(true)){out.push_str("Dynamic linked string table conflicts with address/size metadata; named observations withheld.\n");return 0;}
  }
  match string_section(reader,header,index){Ok(v)=>v,Err(reason)=>{let _=writeln!(out,"Dynamic linked string table unavailable: {reason}.");return 0;}}}
 else{let(address,size)=match(string_address,string_size){(Some(a),Some(s))=>(a,s),_=>{out.push_str("Dynamic string-table address/size not both observed; dependency names unresolved.\n");return 0;}};let offset=match mapped(loads,address,size){Ok(v)=>v,Err(reason)=>{let _=writeln!(out,"Dynamic string table: {reason}; names unresolved.");return 0;}};match string_table(reader,Section{name:0,kind:3,flags:0,address,offset,size,link:0,info:0,align:1,entry:0}){Ok(v)=>v,Err(reason)=>{let _=writeln!(out,"Dynamic string table: {reason}; names unresolved.");return 0;}}};
 let mut found=0;
 for(tag,offset)in names{let label=match tag{1=>"DT_NEEDED dependency candidate",14=>"DT_SONAME declaration",15=>"DT_RPATH declaration (not used)",_=>"DT_RUNPATH declaration (not used)"};match named(strings,offset){Ok(name)=>{let _=writeln!(out,"{label}: \"{name}\". No dependency was located, fetched or loaded.");found+=1;},Err(reason)=>{let _=writeln!(out,"{label}: name unavailable ({reason}).");}}}
 found
}
fn contents(reader:&Reader<'_>,wide:bool,out:&mut String){
 let minimum=if wide{64}else{52};if reader.bytes.len()<minimum{out.push_str("Class-specific ELF header is truncated in examined bytes; table fields withheld.\n");return;}
 let(kind,architecture,version)=(reader.u16(16).unwrap(),reader.u16(18).unwrap(),reader.u32(20).unwrap());
 let(entry,flags,ehsize,mut header)=if wide{(reader.u64(24).unwrap(),reader.u32(48).unwrap(),reader.u16(52).unwrap(),Header{wide,phoff:reader.u64(32).unwrap(),shoff:reader.u64(40).unwrap(),phsize:reader.u16(54).unwrap() as u64,phnum:reader.u16(56).unwrap() as u64,shsize:reader.u16(58).unwrap() as u64,shnum:reader.u16(60).unwrap() as u64,names:reader.u16(62).unwrap() as u64})}
 else{(reader.u32(24).unwrap() as u64,reader.u32(36).unwrap(),reader.u16(40).unwrap(),Header{wide,phoff:reader.u32(28).unwrap() as u64,shoff:reader.u32(32).unwrap() as u64,phsize:reader.u16(42).unwrap() as u64,phnum:reader.u16(44).unwrap() as u64,shsize:reader.u16(46).unwrap() as u64,shnum:reader.u16(48).unwrap() as u64,names:reader.u16(50).unwrap() as u64})};
 let _=writeln!(out,"ELF{} {}-endian header: type {kind}; machine {architecture} ({}); version {version}; entry field 0x{entry:x}; flags 0x{flags:x}; declared header size {ehsize}. Architecture is declared metadata, not measured execution support.",if wide{64}else{32},match reader.endian{Endian::Little=>"little",Endian::Big=>"big"},machine(architecture));
 if version!=1||ehsize as usize!=minimum{out.push_str("Unsupported header version or inconsistent class header size; tables withheld rather than guessed.\n");return;}
 let shminimum=if wide{64}else{40};let phminimum=if wide{56}else{32};
 if header.phnum==65535||(header.shnum==0&&header.shoff!=0)||header.names==65535{
  let mut one=header;one.shnum=1;
  if !table_extent(reader,one.shoff,one.shsize,1,shminimum,"Extended-count section zero",out){return;}
  let zero=match section(reader,&one,0){Some(v) if v.kind==0=>v,_=>{out.push_str("Extended counts require a fully available NULL section-zero header; unresolved table counts withheld.\n");return;}};
  if header.phnum==65535{header.phnum=zero.info as u64;}if header.shnum==0&&header.shoff!=0{header.shnum=zero.size;}if header.names==65535{header.names=zero.link as u64;}
  let _=writeln!(out,"Resolved section-zero extended fields: program count {}, section count {}, section-name index {}.",header.phnum,header.shnum,header.names);
 }
 let ph_valid=table_extent(reader,header.phoff,header.phsize,header.phnum,phminimum,"Program table",out);
 let sh_valid=table_extent(reader,header.shoff,header.shsize,header.shnum,shminimum,"Section table",out);
 let mut loads=Vec::new();let mut dynamics:Vec<Dynamic>=Vec::new();let mut ph_observed=0;
 if ph_valid{for i in 0..header.phnum.min(TABLE_LIMIT){
  let at=match header.phsize.checked_mul(i).and_then(|v|header.phoff.checked_add(v)){Some(v)=>v,None=>break};if reader.slice(at,header.phsize).is_none(){let _=writeln!(out,"Program header {i} is not fully available in the examined prefix; later headers unexamined.");break;}
  let ptype=reader.u32(at).unwrap();let(offset,address,file_size,memory_size,flags,align)=if wide{(reader.u64(at+8).unwrap(),reader.u64(at+16).unwrap(),reader.u64(at+32).unwrap(),reader.u64(at+40).unwrap(),reader.u32(at+4).unwrap(),reader.u64(at+48).unwrap())}else{(reader.u32(at+4).unwrap() as u64,reader.u32(at+8).unwrap() as u64,reader.u32(at+16).unwrap() as u64,reader.u32(at+20).unwrap() as u64,reader.u32(at+24).unwrap(),reader.u32(at+28).unwrap() as u64)};ph_observed+=1;
  let _=writeln!(out,"Program {i}: type {ptype} ({}); file offset {offset}; file size {file_size}; virtual address 0x{address:x}; memory size {memory_size}; flags 0x{flags:x}; alignment {align}.",program_type(ptype));
  let end=match reader.extent(offset,file_size){Ok(v)=>v,Err(reason)=>{let _=writeln!(out,"Program {i}: {reason}; mapping/candidates withheld.");continue;}};
  let memory_end=match address_end(address,memory_size,wide){Some(v)=>v,None=>{let _=writeln!(out,"Program {i}: virtual-address extent overflow; mapping withheld.");continue;}};
  if ptype==1{
   if file_size>memory_size||!pow2_or_small(align)||(align>1&&offset%align!=address%align){let _=writeln!(out,"LOAD {i}: inconsistent file/memory sizes or alignment/congruence; mapping withheld.");continue;}
   let file_address_end=address+file_size;
   let _=writeln!(out,"Declared LOAD mapping: file [{offset},{end}) -> virtual [0x{address:x},0x{file_address_end:x}); declared zero-fill tail {} bytes to 0x{memory_end:x}. This is an ELF address mapping, not an actual process address or loaded-memory observation.",memory_size-file_size);
   loads.push(Load{offset,address,file_size,memory_size});
  }
  if ptype==2&&dynamics.len()<DYNAMIC_TABLE_LIMIT{dynamics.push(Dynamic{offset,size:file_size,string_section:None});}
 }}
 let names=if sh_valid&&header.names>0&&header.names<header.shnum{section(reader,&header,header.names).and_then(|s|match string_table(reader,s){Ok(v)=>Some(v),Err(reason)=>{let _=writeln!(out,"Section-name table unavailable: {reason}.");None}})}else{if header.names>=header.shnum&&header.names!=0{out.push_str("Section-name index exceeds the declared section count; names withheld.\n");}None};
 let mut sh_observed=0;let mut symbol_tables=0;let mut symbol_candidates=0;
 if sh_valid{for i in 0..header.shnum.min(TABLE_LIMIT){
  let s=match section(reader,&header,i){Some(v)=>v,None=>{let _=writeln!(out,"Section header {i} is not fully available in the examined prefix; later headers unexamined.");break;}};sh_observed+=1;
  let _=writeln!(out,"Section {i}: {}; type {} ({}); file offset {}; size {}; address 0x{:x}; flags 0x{:x}; link {}; entry size {}.",label(names,s.name),s.kind,section_type(s.kind),s.offset,s.size,s.address,s.flags,s.link,s.entry);
  if !pow2_or_small(s.align){let _=writeln!(out,"Section {i}: non-power-of-two alignment {}; semantic candidates withheld.",s.align);continue;}
  if address_end(s.address,s.size,wide).is_none(){let _=writeln!(out,"Section {i}: address/size overflow; candidates withheld.");continue;}
  if s.kind==0{continue;}
  if s.kind==8{out.push_str("NOBITS section declares memory extent only; no file payload was read or required.\n");continue;}
  if let Err(reason)=reader.extent(s.offset,s.size){let _=writeln!(out,"Section {i}: {reason}; payload candidates withheld.");continue;}
  if s.flags&0x800!=0{if s.kind==6{dynamics.retain(|d|d.offset!=s.offset||d.size!=s.size);}out.push_str("Compressed section payload is not interpreted; semantic candidates withheld.\n");continue;}
  if matches!(s.kind,2|11){if symbol_tables<SYMBOL_TABLE_LIMIT{symbol_tables+=1;symbol_candidates+=symbols(reader,&header,i,s,out);}else{out.push_str("Additional symbol table omitted under table-count bound.\n");}}
  if s.kind==6{
   if let Some(existing)=dynamics.iter_mut().find(|d|d.offset==s.offset&&d.size==s.size){existing.string_section=Some(s.link);}
   else if dynamics.len()<DYNAMIC_TABLE_LIMIT{dynamics.push(Dynamic{offset:s.offset,size:s.size,string_section:Some(s.link)});}else{out.push_str("Additional dynamic table omitted under table-count bound.\n");}
  }
 }}
 if header.phnum>TABLE_LIMIT||header.shnum>TABLE_LIMIT{out.push_str("Some declared table headers were omitted under the 64-header bound; unseen mappings, symbols and relationships remain unknown.\n");}
 let mut dynamic_names=0;for d in dynamics{dynamic_names+=dynamic(reader,&header,d,&loads,out);}
 let _=writeln!(out,"Observed {ph_observed} program headers and {sh_observed} section headers; classified {symbol_candidates} named symbol candidates and {dynamic_names} dynamic-name declarations. Symbol section-index extensions and executable semantics are not resolved.");
 let observed_memory=loads.iter().fold(0u64,|n,l|n.saturating_add(l.memory_size));let _=writeln!(out,"Observed LOAD memory-size fields sum to {observed_memory} bytes before overlap reconciliation; this is not memory usage, allocation, or a unique-address total.");
}

/// Inspect original held bytes only. Total size is caller metadata, not proof
/// that the file or process remained unchanged outside this read.
pub(super) fn analyze(data:&[u8],complete:bool,total:u64)->String{
 let count=data.len().min(INPUT_LIMIT);let consistent=data.len() as u64<=total&&(!complete||data.len() as u64==total);let whole=consistent&&complete&&count==data.len();
 let mut out=format!("ELF structural observations\nReported source {total} bytes; supplied {}; examined {count} (cap {INPUT_LIMIT}). Scope: {}. Bounds: 64 headers/table, 4 symbol tables ×128 records, 4 dynamic tables ×128 records, 256-byte name scans.\n",data.len(),if whole{"complete supplied input"}else{"prefix only; unseen bytes unknown"});
 if !consistent{out.push_str("Supplied bytes/completion flag/total are inconsistent; whole-file extent comparisons withheld.\n");}
 let bytes=&data[..count];
 if bytes.len()<4{out.push_str("Insufficient bytes for an ELF signature.\n");}
 else if &bytes[..4]!=b"\x7fELF"{out.push_str("No ELF signature observed; other formats are not classified here.\n");}
 else if bytes.len()<16{out.push_str("ELF signature observed, but identification bytes are incomplete; no class/endian fields inferred.\n");}
 else if !matches!(bytes[4],1|2)||!matches!(bytes[5],1|2)||bytes[6]!=1{let _=writeln!(out,"Unsupported identification: class {}, data encoding {}, identification version {}; tables not guessed.",bytes[4],bytes[5],bytes[6]);}
 else{let _=writeln!(out,"Identification OS ABI byte {}; ABI version byte {}. No OS compatibility assertion.",bytes[7],bytes[8]);let reader=Reader{bytes,endian:if bytes[5]==1{Endian::Little}else{Endian::Big},total:if consistent{Some(total)}else{None}};contents(&reader,bytes[4]==2,&mut out);}
 out.push_str("Relocations, dependency locations, disassembly and resolved call relationships remain unobserved by this structural pass.\n");
 if out.len()>OUTPUT_LIMIT{let mut n=(OUTPUT_LIMIT-1400).min(out.len());while !out.is_char_boundary(n){n-=1;}let mut result=out[..n].to_string();result.push_str("\nReport abbreviated; middle inventory rows omitted. Final observations follow:\n");let mut tail=out.len().saturating_sub(1200);while !out.is_char_boundary(tail){tail+=1;}if let Some(line)=out[tail..].find('\n'){tail+=line+1;}result.push_str(&out[tail..]);result}else{out}
}

#[cfg(test)]mod checks{
 use super::*;
 fn put16(b:&mut[u8],at:usize,v:u16,big:bool){b[at..at+2].copy_from_slice(&if big{v.to_be_bytes()}else{v.to_le_bytes()});}
 fn put32(b:&mut[u8],at:usize,v:u32,big:bool){b[at..at+4].copy_from_slice(&if big{v.to_be_bytes()}else{v.to_le_bytes()});}
 fn put64(b:&mut[u8],at:usize,v:u64){b[at..at+8].copy_from_slice(&v.to_le_bytes());}
 fn elf64()->Vec<u8>{
  let mut b=vec![0;1024];b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");put16(&mut b,16,3,false);put16(&mut b,18,62,false);put32(&mut b,20,1,false);put64(&mut b,24,0x4001c0);put64(&mut b,32,64);put64(&mut b,40,512);put16(&mut b,52,64,false);put16(&mut b,54,56,false);put16(&mut b,56,2,false);put16(&mut b,58,64,false);put16(&mut b,60,6,false);put16(&mut b,62,1,false);
  put32(&mut b,64,1,false);put32(&mut b,68,5,false);put64(&mut b,80,0x400000);put64(&mut b,96,1024);put64(&mut b,104,1040);put64(&mut b,112,4096);
  put32(&mut b,120,2,false);put64(&mut b,128,272);put64(&mut b,136,0x400110);put64(&mut b,152,64);put64(&mut b,160,64);put64(&mut b,168,8);
  let names=b"\0.shstrtab\0.dynstr\0.dynsym\0.dynamic\0.text\0";b[352..352+names.len()].copy_from_slice(names);let strings=b"\0puts\0exported\0libc.so\0";b[400..400+strings.len()].copy_from_slice(strings);
  for(i,name,kind,offset,size,link,entry)in[(1,1,3,352,names.len() as u64,0,0),(2,11,3,400,strings.len() as u64,0,0),(3,19,11,192,72,2,24),(4,27,6,272,64,2,16),(5,36,1,448,16,0,0)]{let at=512+i*64;put32(&mut b,at,name,false);put32(&mut b,at+4,kind,false);put64(&mut b,at+24,offset);put64(&mut b,at+32,size);put32(&mut b,at+40,link,false);put64(&mut b,at+48,1);put64(&mut b,at+56,entry);}
  put32(&mut b,216,1,false);b[220]=0x12;put32(&mut b,240,6,false);b[244]=0x12;put16(&mut b,246,5,false);put64(&mut b,248,0x4001c0);put64(&mut b,256,4);
  for(i,tag,value)in[(0,5,0x400190),(1,10,strings.len() as u64),(2,1,15),(3,0,0)]{put64(&mut b,272+i*16,tag);put64(&mut b,280+i*16,value);}b
 }
 #[test]fn elf64_headers_mapping_names_and_candidates(){let b=elf64();let out=analyze(&b,true,b.len() as u64);assert!(out.contains("ELF64 little-endian"));assert!(out.contains("file [0,1024) -> virtual [0x400000,0x400400)"));assert!(out.contains("dynamic import candidate (undefined symbol); name \"puts\""));assert!(out.contains("dynamic export candidate (declared definition); name \"exported\""));assert!(out.contains("DT_NEEDED dependency candidate: \"libc.so\""));assert!(out.contains("semantic"));}
 #[test]fn elf32_big_endian_load_fields(){let mut b=vec![0;128];b[..7].copy_from_slice(b"\x7fELF\x01\x02\x01");put16(&mut b,16,2,true);put16(&mut b,18,3,true);put32(&mut b,20,1,true);put32(&mut b,24,0x1000,true);put32(&mut b,28,52,true);put16(&mut b,40,52,true);put16(&mut b,42,32,true);put16(&mut b,44,1,true);put16(&mut b,46,40,true);put32(&mut b,52,1,true);put32(&mut b,60,0x1000,true);put32(&mut b,68,128,true);put32(&mut b,72,160,true);put32(&mut b,76,5,true);put32(&mut b,80,4096,true);let out=analyze(&b,true,128);assert!(out.contains("ELF32 big-endian"));assert!(out.contains("file [0,128) -> virtual [0x1000,0x1080)"));assert!(out.contains("zero-fill tail 32"));}
 #[test]fn extended_counts_and_sectionless_dynamic(){let mut b=elf64();put16(&mut b,56,65535,false);put16(&mut b,60,0,false);put16(&mut b,62,65535,false);put64(&mut b,544,6);put32(&mut b,552,1,false);put32(&mut b,556,2,false);assert!(analyze(&b,true,1024).contains("Resolved section-zero extended fields: program count 2, section count 6, section-name index 1"));let mut b=elf64();put64(&mut b,40,0);put16(&mut b,60,0,false);put16(&mut b,62,0,false);assert!(analyze(&b,true,1024).contains("DT_NEEDED dependency candidate: \"libc.so\""));}
 #[test]fn sampled_and_malformed_extents_never_infer_names(){let b=elf64();let out=analyze(&b[..176],false,1024);assert!(out.contains("prefix only"));assert!(out.contains("Section header 0 is not fully available"));assert!(!out.contains("name \"puts\""));let mut b=elf64();put64(&mut b,32,u64::MAX-8);assert!(analyze(&b,true,1024).contains("overflow"));let mut b=elf64();put16(&mut b,54,8,false);assert!(analyze(&b,true,1024).contains("below the class minimum"));let mut b=elf64();put64(&mut b,104,1);assert!(analyze(&b,true,1024).contains("inconsistent file/memory sizes"));}
 #[test]fn string_symbol_and_dynamic_bounds_refuse_candidates(){let mut b=elf64();b[422]=b'x';let out=analyze(&b,true,1024);assert!(out.contains("required initial/final NUL"));assert!(!out.contains("dynamic import candidate (undefined symbol); name"));let mut b=elf64();put64(&mut b,512+3*64+56,8);assert!(analyze(&b,true,1024).contains("invalid entry size"));let mut b=elf64();put64(&mut b,272+3*16,1);assert!(analyze(&b,true,1024).contains("DT_NULL terminator not observed"));}
 #[test]fn input_output_and_unknown_bounds(){assert!(analyze(b"",true,0).contains("Insufficient bytes"));assert!(analyze(b"not elf",true,7).contains("No ELF signature"));let mut b=elf64();b[4]=3;assert!(analyze(&b,true,1024).contains("Unsupported identification"));let out=analyze(&elf64(),true,999);assert!(out.contains("inconsistent"));assert!(out.len()<8192);let mut b=vec![0;INPUT_LIMIT+10];b[..1024].copy_from_slice(&elf64());let out=analyze(&b,true,b.len() as u64);assert!(out.contains("prefix only"));assert!(out.len()<8192);}
 #[test]fn compressed_conflicting_and_class_width_metadata(){
  let mut b=elf64();put64(&mut b,512+2*64+8,0x800);let out=analyze(&b,true,1024);assert!(out.contains("compressed string table"));assert!(!out.contains("dynamic import candidate (undefined symbol); name"));
  let mut b=elf64();put64(&mut b,512+3*64+8,0x800);let out=analyze(&b,true,1024);assert!(out.contains("Compressed section payload"));assert!(!out.contains("dynamic import candidate (undefined symbol); name"));
  let mut b=elf64();put64(&mut b,280,0x400191);let out=analyze(&b,true,1024);assert!(out.contains("conflicts with address/size metadata"));assert!(!out.contains("DT_NEEDED dependency candidate:"));
  assert_eq!(address_end(0xfffffff0,32,false),None);assert_eq!(address_end(0xfffffff0,16,false),Some(0x1_0000_0000));assert_eq!(address_end(u64::MAX,1,true),None);
  let mut b=elf64();put16(&mut b,246,0xfff2,false);assert!(analyze(&b,true,1024).contains("tentative allocation, not established export"));
 }

}
