//! Original read-only observations of declared PE image layout and imports.
use std::fmt::{self,Write as _};
const INPUT_LIMIT:usize=4*1024*1024;
const OUTPUT_LIMIT:usize=16384;
const SECTION_LIMIT:usize=96;
const DLL_LIMIT:usize=64;
const IMPORT_LIMIT:usize=128;
const NAME_LIMIT:usize=256;
const RELOCATION_BLOCKS:usize=32;
const RELOCATION_SLOTS:usize=128;
#[derive(Debug)]enum Issue{Cancelled,Work,Operation(String),Missing(&'static str),Invalid(&'static str),Padding(u64)}
impl fmt::Display for Issue{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{match self{Self::Cancelled=>f.write_str("inspection cancelled"),Self::Operation(s)=>f.write_str(s),Self::Work=>f.write_str("logical work bound reached; remaining data unexamined"),Self::Missing(s)|Self::Invalid(s)=>f.write_str(s),Self::Padding(p)=>write!(f,"raw-padding candidate file offset {p}; outside declared virtual size, loaded RVA unresolved")}}}
type PResult<T>=Result<T,Issue>;
struct Budget<'a>{left:usize,cancelled:&'a dyn Fn()->bool}
impl Budget<'_>{fn step(&mut self,n:usize)->PResult<()>{if(self.cancelled)(){return Err(Issue::Cancelled);}self.left=self.left.checked_sub(n).ok_or(Issue::Work)?;Ok(())}}
struct Report{value:String,full:bool}
impl Report{fn new()->Self{Self{value:String::with_capacity(1024),full:false}}fn line(&mut self,args:fmt::Arguments<'_>){if self.full{return;}let line=args.to_string();if self.value.len()+line.len()+1>OUTPUT_LIMIT-256{self.full=true;return;}self.value.push_str(&line);self.value.push('\n');}fn finish(mut self)->String{if self.full{self.value.push_str("Further report rows omitted under the16KiB output bound; omitted tables, terminators and names remain unreported.\n");}self.value}}
macro_rules! row{($out:expr,$($arg:tt)*)=>{$out.line(format_args!($($arg)*))}}
#[derive(Clone,Copy)]struct Reader<'a>{bytes:&'a[u8],total:Option<u64>}
impl<'a> Reader<'a>{fn extent(&self,at:u64,n:u64)->PResult<u64>{let end=at.checked_add(n).ok_or(Issue::Invalid("file extent overflows64-bit offsets"))?;if self.total.is_some_and(|v|end>v){return Err(Issue::Invalid("file extent exceeds reported source length"));}Ok(end)}fn slice(&self,at:u64,n:u64)->PResult<&'a[u8]>{let end=self.extent(at,n)?;if end>self.bytes.len()as u64{return Err(Issue::Missing("referenced bytes are unavailable in examined prefix"));}Ok(&self.bytes[at as usize..end as usize])}fn u16(&self,at:u64)->PResult<u16>{Ok(u16::from_le_bytes(self.slice(at,2)?.try_into().unwrap()))}fn u32(&self,at:u64)->PResult<u32>{Ok(u32::from_le_bytes(self.slice(at,4)?.try_into().unwrap()))}fn u64(&self,at:u64)->PResult<u64>{Ok(u64::from_le_bytes(self.slice(at,8)?.try_into().unwrap()))}}
struct Optional<'a>{r:Reader<'a>,at:u64,size:u64}
impl Optional<'_>{fn field(&self,offset:u64,n:u64)->PResult<u64>{if offset.checked_add(n).filter(|n|*n<=self.size).is_none(){return Err(Issue::Missing("field is outside declared optional-header size"));}self.at.checked_add(offset).ok_or(Issue::Invalid("optional-header offset overflow"))}fn u16(&self,o:u64)->PResult<u16>{self.r.u16(self.field(o,2)?)}fn u32(&self,o:u64)->PResult<u32>{self.r.u32(self.field(o,4)?)}fn u64(&self,o:u64)->PResult<u64>{self.r.u64(self.field(o,8)?)}}
#[derive(Clone,Copy)]struct Section{rva:u64,virtual_size:u64,raw:u64,raw_size:u64,raw_valid:bool,executable:bool}
struct Image<'a>{r:Reader<'a>,sections:Vec<Section>,headers:u64,complete_table:bool,wide:bool}
impl Image<'_>{
 fn mapped(&self,rva:u64,n:u64,b:&mut Budget<'_>)->PResult<(u64,u64)>{
  if !self.complete_table{return Err(Issue::Missing("section table not fully observed; unseen sections could add ambiguous mappings"));}
  let end=rva.checked_add(n).filter(|v|*v<=1u64<<32).ok_or(Issue::Invalid("reference exceeds32-bit RVA extent"))?;
  let mut selected=None;let mut candidates=0usize;
  if rva<self.headers&&end>0{selected=Some(None);candidates+=1;}
  for(i,s)in self.sections.iter().enumerate(){b.step(1)?;let ve=s.rva+s.virtual_size;if s.virtual_size>0&&rva<ve&&end>s.rva{selected=Some(Some(i));candidates+=1;}}
  if candidates>1{return Err(Issue::Invalid("ambiguous overlapping virtual/header mappings"));}
  let(off,mut available)=match selected{
   Some(None)=>{if end>self.headers{return Err(Issue::Invalid("reference crosses declared header extent"));}self.r.extent(0,self.headers)?;(rva,self.headers-rva)},
   Some(Some(i))=>{let s=self.sections[i];let ve=s.rva+s.virtual_size;if ve>1u64<<32{return Err(Issue::Invalid("section virtual extent exceeds32-bit RVA width"));}if rva<s.rva||end>ve{return Err(Issue::Invalid("reference crosses declared section virtual extent"));}let delta=rva-s.rva;if !s.raw_valid{return Err(Issue::Invalid("section raw extent exceeds reported source length"));}if delta.checked_add(n).is_none_or(|v|v>s.raw_size){return Err(Issue::Missing("reference is in a virtual-only tail; no source bytes synthesized"));}(s.raw+delta,(s.virtual_size-delta).min(s.raw_size-delta))},
   None=>{for s in &self.sections{b.step(1)?;if rva>=s.rva{let d=rva-s.rva;if d.checked_add(n).is_some_and(|v|v<=s.raw_size)&&end>s.rva+s.virtual_size{return Err(Issue::Padding(s.raw+d));}}}return Err(Issue::Missing("RVA has no declared virtual/header mapping"));}
  };
  self.r.slice(off,n)?;available=available.min((self.r.bytes.len()as u64).saturating_sub(off));if let Some(total)=self.r.total{available=available.min(total.saturating_sub(off));}
  // A following overlap must be reconsidered before any name scan crosses it.
  for s in &self.sections{b.step(1)?;if s.virtual_size>0&&s.rva>rva{available=available.min(s.rva-rva);}}
  Ok((off,available))
 }
 fn name(&self,mut rva:u64,b:&mut Budget<'_>)->PResult<String>{let mut bytes=[0u8;NAME_LIMIT];let mut used=0;
  while used<NAME_LIMIT{let(off,available)=self.mapped(rva,1,b)?;let take=(available as usize).min(NAME_LIMIT-used);if take==0{return Err(Issue::Missing("name has no captured contiguous bytes"));}let part=self.r.slice(off,take as u64)?;
   for v in part{b.step(1)?;if *v==0{if used==0{return Err(Issue::Invalid("empty declared name; no usable name candidate"));}return Ok(escaped(&bytes[..used]));}if *v>127{return Err(Issue::Invalid("declared name contains non-ASCII bytes"));}bytes[used]=*v;used+=1;rva=rva.checked_add(1).ok_or(Issue::Invalid("name RVA overflow"))?;}
  }Err(Issue::Missing("no name terminator observed within256-byte scan bound"))
 }
}
fn escaped(bytes:&[u8])->String{let mut s=String::with_capacity(bytes.len());for c in bytes{if(0x20..=0x7e).contains(c)&&!matches!(*c,b'"'|b'\\'){s.push(*c as char);}else{let _=write!(s,"\\x{c:02x}");}}s}
fn machine(n:u16)->&'static str{match n{0x14c=>"x86",0x8664=>"x86-64",0x1c0=>"Arm",0x1c4=>"Arm Thumb-2",0xaa64=>"Arm64",0xa641=>"Arm64EC",0xa64e=>"Arm64X",0x5032=>"RISC-V32",0x5064=>"RISC-V64",0x6264=>"LoongArch64",_=>"unclassified machine value"}}
pub(super) fn relocation_type(code:u8)->(&'static str,Option<u8>){match code{0=>("ABSOLUTE padding",None),1=>("HIGH",Some(2)),2=>("LOW",Some(2)),3=>("HIGHLOW",Some(4)),4=>("HIGHADJ",Some(2)),10=>("DIR64",Some(8)),5|7|8|9=>("machine-dependent; not decoded",None),_=>("reserved/unclassified; not decoded",None)}}
fn relocations(image:&Image<'_>,rva:u64,size:u64,out:&mut Report,b:&mut Budget<'_>,facts:&mut Vec<super::control_flow::PeRelocation>)->PResult<bool>{
 row!(out,"PE base-relocation declarations: directoryRVA0x{rva:x}, bytes{size}; at most32blocks/128slots. Observation mode: held-source declarations.");
 if rva==0&&size==0{row!(out,"Observed base-relocation directory entry: empty.");return Ok(true);}
 if rva==0||size<8||rva.checked_add(size).is_none_or(|end|end>1u64<<32){return Err(Issue::Invalid("base-relocation directory extent is inconsistent"));}
 let mut cursor=0u64;let mut slots=0usize;let mut coherent=true;
 for block in 0..RELOCATION_BLOCKS{
  if cursor==size{return Ok(coherent);}b.step(1)?;
  let start=rva.checked_add(cursor).ok_or(Issue::Invalid("base-relocation block RVA overflows"))?;
  if start%4!=0||size-cursor<8{return Err(Issue::Invalid("base-relocation block header is unaligned or incomplete"));}
  let(at,_)=image.mapped(start,8,b)?;let page=image.r.u32(at)?;let block_size=image.r.u32(at+4)?as u64;
  if block_size<8||block_size%2!=0||block_size>size-cursor{return Err(Issue::Invalid("base-relocation block size is invalid or exceeds the directory"));}
  if page%4096!=0{coherent=false;row!(out,"Block{block} pageRVA0x{page:x} is not4KiB-aligned; loader interpretation unresolved.");}
  let mut slot=0u64;
  while slot<(block_size-8)/2{
   b.step(1)?;if slots>=RELOCATION_SLOTS{row!(out,"Base-relocation slot bound reached; remaining directory unexamined.");return Ok(false);}
   let entry=start+8+slot*2;let(file_offset,_)=image.mapped(entry,2,b)?;let word=image.r.u16(file_offset)?;let code=(word>>12)as u8;let offset=word&4095;let(_,width)=relocation_type(code);
   let target=(page as u64).checked_add(offset as u64).filter(|v|*v<=u32::MAX as u64);
   let mut fact=super::control_flow::PeRelocation{block:block as u32,slot:slot as u32,file_offset,page,offset,code,adjustment:None,target_file_offset:None};
   if page%4096==0{if let(Some(target),Some(width))=(target,width){match image.mapped(target,width as u64,b){Ok((off,_))=>fact.target_file_offset=Some(off),Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(_)=>{}}}}
   slots+=1;slot+=1;
   if code==4{
    if slot>=(block_size-8)/2||slots>=RELOCATION_SLOTS{facts.push(fact);row!(out,"HIGHADJ adjustment word unavailable or beyond slot allowance; record incomplete.");return Ok(false);}
    let adjustment=match image.mapped(entry+2,2,b).and_then(|(off,_)|image.r.u16(off).map(|word|(word,off))){Ok(word)=>word,Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(error)=>{facts.push(fact);return Err(error);}};
    fact.adjustment=Some(adjustment);slots+=1;slot+=1;
   }
   let(name,_)=relocation_type(code);row!(out,"Base relocation block{block} slot{} file offset{file_offset}: {name}({code}), pageRVA0x{page:x}+offset0x{offset:x}; target field mapping{}. Adjustment word{:?}; declaration only.",fact.slot,if fact.target_file_offset.is_some(){"captured"}else{"unresolved/not applicable"},fact.adjustment);facts.push(fact);
  }
  cursor+=block_size;
 }
 if cursor==size{Ok(coherent)}else{row!(out,"Base-relocation block bound reached; later blocks unexamined.");Ok(false)}
}
fn imports(image:&Image<'_>,rva:u64,size:u64,out:&mut Report,b:&mut Budget<'_>)->PResult<()>{
 if rva==0&&size==0{row!(out,"Import directory absent in observed optional-header declaration; no imported behavior inferred.");return Ok(());}if rva==0||size<20{row!(out,"Import directory metadata inconsistent or too short for a descriptor; imports unresolved.");return Ok(());}
 if rva.checked_add(size).filter(|v|*v<=1u64<<32).is_none(){row!(out,"Import directory extent exceeds32-bit RVA width; declared references unresolved.");return Ok(());}
 row!(out,"Declared import directory RVA0x{rva:x}, size{size}; bounds64DLL descriptors/128total lookup records/256bytes per name.");
 if !image.complete_table{row!(out,"Import references unresolved because section table is incomplete or observation-bounded.");return Ok(());}
 let mut dlls=0usize;let mut records=0usize;let mut candidates=0usize;let mut terminated=false;let mut descriptor_partial=false;
 for index in 0..(size/20).min(DLL_LIMIT as u64){b.step(1)?;if out.full{break;}let at=rva.checked_add(index.checked_mul(20).ok_or(Issue::Invalid("descriptor offset overflow"))?).ok_or(Issue::Invalid("descriptor RVA overflow"))?;
  let offset=match image.mapped(at,20,b){Ok((off,_))=>off,Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(e)=>{row!(out,"Import descriptor{index} unavailable: {e}; later descriptors unexamined.");descriptor_partial=true;break;}};
  let bytes=image.r.slice(offset,20)?;let get=|o|u32::from_le_bytes(bytes[o..o+4].try_into().unwrap())as u64;let(lookup,stamp,forward,name,iat)=(get(0),get(4),get(8),get(12),get(16));if lookup==0&&stamp==0&&forward==0&&name==0&&iat==0{terminated=true;break;}dlls+=1;
  row!(out,"DLL descriptor{index}: lookupRVA0x{lookup:x}, nameRVA0x{name:x}, IATRVA0x{iat:x}, timestamp0x{stamp:x}, forwarder0x{forward:x}.");
  match if name==0{Err(Issue::Invalid("DLL name RVA is null"))}else{image.name(name,b)}{Ok(n)=>row!(out,"Declared DLL name candidate: \"{n}\" (not located, fetched or loaded)."),Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(e)=>row!(out,"DLL name unresolved: {e}.")}
  if lookup==0{row!(out,"Import lookup table absent; IAT can contain bound addresses and is not reinterpreted as names or ordinals.");continue;}
  let stride=if image.wide{8}else{4};let flag=if image.wide{1u64<<63}else{1u64<<31};let mut entries=0usize;let mut lookup_end=false;let mut partial=false;
  while records<IMPORT_LIMIT&&!out.full{b.step(1)?;let slot=lookup.checked_add((entries as u64).checked_mul(stride).ok_or(Issue::Invalid("lookup index overflow"))?).ok_or(Issue::Invalid("lookup RVA overflow"))?;let offset=match image.mapped(slot,stride,b){Ok((off,_))=>off,Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(e)=>{row!(out,"Lookup record{entries} unresolved: {e}.");partial=true;break;}};let value=if image.wide{image.r.u64(offset)?}else{image.r.u32(offset)?as u64};if value==0{lookup_end=true;break;}records+=1;entries+=1;
   if value&flag!=0{if value&!(flag|0xffff)!=0{row!(out,"Lookup record{} has reserved ordinal bits set; ordinal unresolved.",entries-1);}else{candidates+=1;row!(out,"Declared import ordinal candidate{} (not resolved or executed).",value&0xffff);}}
   else if value&!0x7fff_ffff!=0{row!(out,"Lookup record{} has reserved named-RVA bits set; name unresolved.",entries-1);}
   else{let hint=match image.mapped(value,2,b).and_then(|(off,_)|image.r.u16(off)){Ok(v)=>v,Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(e)=>{row!(out,"Lookup record{} hint/name unavailable: {e}.",entries-1);continue;}};match image.name(value+2,b){Ok(n)=>{candidates+=1;row!(out,"Declared import name candidate \"{n}\", hint{hint} (not resolved or executed).");},Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(Issue::Work)=>return Err(Issue::Work),Err(e)=>row!(out,"Lookup record{} name unresolved: {e}.",entries-1)}}
  }
  row!(out,"Lookup table observed{entries} nonzero records; null terminator{}. {}",if lookup_end{"observed"}else{"not observed"},if records>=IMPORT_LIMIT&&!lookup_end{"Total lookup-record bound reached; remaining entries unknown."}else if partial{"Unavailable records remain unknown."}else{""});
 }
 row!(out,"Observed DLL descriptors{dlls}; lookup records{records}; name/ordinal candidates{candidates}; descriptor null terminator{}. {}",if terminated{"observed"}else{"not observed"},if !terminated||descriptor_partial{"Import directory observation incomplete; no complete-empty inference."}else{"Declared descriptor sequence ended; lookup/name limitations above remain."});Ok(())
}
fn observe(r:Reader<'_>,out:&mut Report,b:&mut Budget<'_>,flow:&mut super::control_flow::Observation)->PResult<()>{
 b.step(1)?;if r.slice(0,2)?!=b"MZ"{row!(out,"No MZ DOS signature observed; this observer does not classify other formats.");return Ok(());}let pe=r.u32(0x3c)?as u64;row!(out,"DOS MZ signature; declared PE signature file offset{pe}.");if r.slice(pe,4)?!=b"PE\0\0"{row!(out,"No PE signature at the declared offset; MZ alone is not a PE classification.");return Ok(());}
 let coff=pe+4;r.slice(coff,20)?;let kind=r.u16(coff)?;let count=r.u16(coff+2)?as usize;let optional_size=r.u16(coff+16)?as u64;let flags=r.u16(coff+18)?;row!(out,"PE signature and COFF fields: machine0x{kind:04x}({}), sections{count}, optional-header bytes{optional_size}, characteristics0x{flags:04x}.",machine(kind));
 let optional=Optional{r,at:coff+20,size:optional_size};let magic=optional.u16(0)?;let wide=match magic{0x10b=>false,0x20b=>true,_=>{row!(out,"Unsupported optional-header magic0x{magic:04x}; format-specific fields and imports unexamined.");return Ok(());}};
 let entry=optional.u32(16)?;let image_base=if wide{optional.u64(24)?}else{optional.u32(28)?as u64};let section_alignment=optional.u32(32)?;let file_alignment=optional.u32(36)?;let image_size=optional.u32(56)?;let headers=optional.u32(60)?as u64;let subsystem=optional.u16(68)?;let dll_flags=optional.u16(70)?;let directories=optional.u32(if wide{108}else{92})?as u64;let directory_start=if wide{112}else{96};
 row!(out,"{} optional fields: entryRVA0x{entry:x}, preferred image base0x{image_base:x}, image bytes{image_size}, header bytes{headers}, section/file alignment{section_alignment}/{file_alignment}, subsystem{subsystem}, DLLflags0x{dll_flags:04x}, directory count{directories}.",if wide{"PE32+"}else{"PE32"});
 if section_alignment==0||file_alignment==0||!file_alignment.is_power_of_two(){row!(out,"Alignment declarations are zero or non-power-of-two; no loader validity claim.");}
 let available=optional_size.saturating_sub(directory_start)/8;if directories>available{row!(out,"Directory count exceeds declared optional-header capacity{available}; only individually available entries can be observed.");}
 if directories>4{match(optional.u32(directory_start+4*8),optional.u32(directory_start+4*8+4)){(Ok(file),Ok(size))=>row!(out,"Certificate-directory declaration: FILE offset{file}, size{size}; not an RVA. Certificate bytes/signature/trust unexamined."),_=>row!(out,"Certificate-directory fields unavailable; no signature or trust inference.")}}
 let table=optional.at.checked_add(optional_size).ok_or(Issue::Invalid("section-table offset overflow"))?;let table_bytes=(count as u64).checked_mul(40).ok_or(Issue::Invalid("section-table size overflow"))?;let mut complete_table=count<=SECTION_LIMIT&&r.slice(table,table_bytes).is_ok();let mut sections=Vec::with_capacity(count.min(SECTION_LIMIT));
 for i in 0..count.min(SECTION_LIMIT){b.step(1)?;let at=table+i as u64*40;let s=match r.slice(at,40){Ok(s)=>s,Err(e)=>{row!(out,"Section header{i} unavailable: {e}; later headers unexamined.");complete_table=false;break;}};let get=|o|u32::from_le_bytes(s[o..o+4].try_into().unwrap())as u64;let section=Section{virtual_size:get(8),rva:get(12),raw_size:get(16),raw:get(20),raw_valid:r.extent(get(20),get(16)).is_ok(),executable:get(36)&0x2000_0000!=0};let end_name=s[..8].iter().position(|v|*v==0).unwrap_or(8);row!(out,"Section{i} \"{}\": RVA0x{:x}, virtual bytes{}, raw file[{},{}), flags0x{:08x}.",escaped(&s[..end_name]),section.rva,section.virtual_size,section.raw,section.raw+section.raw_size,get(36));
  if !section.raw_valid{row!(out,"Section{i} raw extent exceeds reported source; mapping unresolved.");}if section.rva+section.virtual_size>1u64<<32{row!(out,"Section{i} virtual extent exceeds32-bit RVA width; mapping unresolved.");}if section.raw_size>section.virtual_size{row!(out,"Section{i} includes{} raw bytes beyond declared virtual size; padding is not assumed loaded.",section.raw_size-section.virtual_size);}if section.virtual_size>section.raw_size{row!(out,"Section{i} declares{} virtual-only bytes; no source zero bytes synthesized.",section.virtual_size-section.raw_size);}sections.push(section);
 }
 row!(out,"Section headers declared{count}, observed{}; complete table{}. Header-table file extent[{},{}).",sections.len(),complete_table,table,table+table_bytes);
 if count>SECTION_LIMIT{row!(out,"Section observation bound96 reached; unseen section mappings make import references unresolved.");}
 for(i,s)in sections.iter().enumerate(){if s.raw_size>0&&s.raw<headers{row!(out,"Section{i} raw file extent aliases declared header bytes; this is layout data, not unique-RVA ambiguity by itself.");}for(j,t)in sections[..i].iter().enumerate(){b.step(1)?;if s.raw_size>0&&t.raw_size>0&&s.raw<t.raw+t.raw_size&&t.raw<s.raw+s.raw_size{row!(out,"Sections{j}/{i} raw file extents alias; no loader or memory behavior inferred.");}}}
 let image=Image{r,sections,headers,complete_table,wide};
 let mut regions=vec![super::control_flow::Region{offset:0,address:0,file_size:headers,memory_size:headers,executable:false}];
 for s in &image.sections{regions.push(super::control_flow::Region{offset:s.raw,address:s.rva,file_size:s.raw_size.min(s.virtual_size),memory_size:s.virtual_size,executable:s.executable});}
 let inventory=complete_table&&r.total.is_some()&&r.extent(0,headers).is_ok()&&image.sections.iter().all(|s|s.raw_valid&&s.rva+s.virtual_size<=1u64<<32);
 if kind==0xaa64&&wide{
 *flow=super::control_flow::observe(r.bytes,&regions,inventory,"RVA ",b.cancelled,&[]).map_err(|e|if(b.cancelled)(){Issue::Cancelled}else{Issue::Operation(e)})?;
 }else{flow.coordinate="RVA ".into();flow.regions=regions;flow.mappings_complete=inventory;flow.text=format!("Control-flow instruction interpretation unavailable for PE machine0x{kind:04x}/class; mixed Arm64EC/Arm64X is not interpreted as uniform AArch64. Existing structural observations remain available.\n");}
 if directories<=5{flow.relocations_complete=Some(true);row!(out,"Observed directory count places the base-relocation entry outside the declared directory list.");}
 else{let result=optional.u32(directory_start+5*8).and_then(|rva|optional.u32(directory_start+5*8+4).map(|size|(rva,size))).and_then(|(rva,size)|relocations(&image,rva as u64,size as u64,out,b,&mut flow.pe_relocations));match result{Ok(complete)=>flow.relocations_complete=Some(complete),Err(Issue::Cancelled)=>return Err(Issue::Cancelled),Err(error)=>{flow.relocations_complete=Some(false);row!(out,"Base-relocation inventory incomplete: {error}; previously observed declarations retained, remaining records unknown.");}}}
 if out.full{return Ok(());}
 if directories<2{row!(out,"Observed directory count declares no import-directory entry; unavailable declarations are not inferred.");return Ok(());}
 let import_rva=optional.u32(directory_start+8)?as u64;let import_size=optional.u32(directory_start+12)?as u64;imports(&image,import_rva,import_size,out,b)
}
/// Analyze the already held inspection bytes; no binary, dependency or table is
/// loaded, executed, fetched, or treated as trusted instructions.
pub(super) fn analyze(data:&[u8],complete:bool,total:u64,cancelled:&impl Fn()->bool)->Result<String,String>{
 observe_with_cancel(data,complete,total,cancelled).map(|v|v.text)
}
pub(super) fn observe_with_cancel(data:&[u8],complete:bool,total:u64,cancelled:&impl Fn()->bool)->Result<super::control_flow::Observation,String>{
 let count=data.len().min(INPUT_LIMIT);let consistent=data.len()as u64<=total&&(!complete||data.len()as u64==total);let whole=consistent&&complete&&count==data.len();let mut out=Report::new();row!(out,"PE image observations (untrusted declared metadata, not executable behavior)");row!(out,"Reported source{total}bytes; supplied{}; examined{count} (existing4MiB bound). Scope:{}.",data.len(),if whole{"complete supplied input"}else{"prefix only; unseen bytes unknown"});
 if !consistent{row!(out,"Supplied length/completion/total inconsistent; whole-source extent comparisons withheld.");}let reader=Reader{bytes:&data[..count],total:if consistent{Some(total)}else{None}};let mut budget=Budget{left:1_000_000,cancelled};
 let mut flow=super::control_flow::Observation::default();match observe(reader,&mut out,&mut budget,&mut flow){Ok(())=>{},Err(Issue::Cancelled)=>return Err("Inspection cancelled during PE observations".into()),Err(e)=>row!(out,"PE observation incomplete: {e}. No unavailable table/name is treated as an observed empty result.")}
 if cancelled(){return Err("Inspection cancelled during PE observations".into());}row!(out,"Held-source PE metadata observation complete. Instruction encoding observations follow.");flow.text=format!("{}\n{}",out.finish(),flow.text);Ok(flow)
}
#[cfg(test)]mod relocation_checks{
 use super::*;
 fn w16(v:&mut[u8],at:usize,n:u16){v[at..at+2].copy_from_slice(&n.to_le_bytes());}
 fn w32(v:&mut[u8],at:usize,n:u32){v[at..at+4].copy_from_slice(&n.to_le_bytes());}
 fn image()->Vec<u8>{
  let mut v=vec![0u8;0x800];v[..2].copy_from_slice(b"MZ");w32(&mut v,0x3c,0x80);v[0x80..0x84].copy_from_slice(b"PE\0\0");w16(&mut v,0x84,0xaa64);w16(&mut v,0x86,3);w16(&mut v,0x94,240);
  let o=0x98;w16(&mut v,o,0x20b);w32(&mut v,o+16,0x1000);v[o+24..o+32].copy_from_slice(&0x140000000u64.to_le_bytes());w32(&mut v,o+32,4096);w32(&mut v,o+36,512);w32(&mut v,o+56,0x4000);w32(&mut v,o+60,512);w16(&mut v,o+68,3);w32(&mut v,o+108,16);w32(&mut v,o+112+40,0x3000);w32(&mut v,o+112+44,12);
  for(i,name)in[b".text\0\0\0",b".data\0\0\0",b".reloc\0\0"].iter().enumerate(){let at=o+240+i*40;v[at..at+8].copy_from_slice(*name);w32(&mut v,at+8,512);w32(&mut v,at+12,(i as u32+1)*4096);w32(&mut v,at+16,512);w32(&mut v,at+20,(i as u32+1)*512);w32(&mut v,at+36,if i==0{0x60000020}else{0x40000040});}
  w32(&mut v,0x200,0xd65f03c0);w32(&mut v,0x600,0x2000);w32(&mut v,0x604,12);w16(&mut v,0x608,0xa008);v
 }
 fn inspect(v:&[u8],total:u64)->super::super::control_flow::Observation{observe_with_cancel(v,v.len()as u64==total,total,&||false).unwrap()}
 #[test]fn captured_dir64_and_padding_are_declarations_not_loaded_values(){let v=image();let o=inspect(&v,v.len()as u64);assert_eq!(o.relocations_complete,Some(true));assert_eq!(o.pe_relocations.len(),2);let r=&o.pe_relocations[0];assert_eq!((r.code,r.page,r.offset,r.file_offset,r.target_file_offset),(10,0x2000,8,0x608,Some(0x408)));assert_eq!(o.pe_relocations[1].code,0);assert!(o.pe_relocations[1].target_file_offset.is_none());assert!(o.text.contains("Observation mode: held-source declarations."));}
 #[test]fn highadj_consumes_its_adjustment_word_not_another_type(){let mut v=image();w16(&mut v,0x608,0x4008);w16(&mut v,0x60a,0xa123);let o=inspect(&v,v.len()as u64);assert_eq!(o.pe_relocations.len(),1);assert_eq!(o.pe_relocations[0].adjustment,Some((0xa123,0x60a)));assert_eq!(o.relocations_complete,Some(true));}
 #[test]fn missing_pair_and_prefix_keep_observed_record_and_unknown_remainder(){let mut v=image();w16(&mut v,0x608,0x4008);w32(&mut v,0x604,10);w32(&mut v,0x98+112+44,10);let o=inspect(&v,v.len()as u64);assert_eq!(o.relocations_complete,Some(false));assert_eq!(o.pe_relocations.len(),1);assert!(o.pe_relocations[0].adjustment.is_none());w32(&mut v,0x604,12);w32(&mut v,0x98+112+44,12);let o=inspect(&v[..0x60a],v.len()as u64);assert_eq!(o.pe_relocations.len(),1);assert_eq!(o.relocations_complete,Some(false));}
 #[test]fn invalid_block_sizes_do_not_become_complete_empty_inventory(){for size in[0,6,9,16,u32::MAX]{let mut v=image();w32(&mut v,0x604,size);let o=inspect(&v,v.len()as u64);assert_eq!(o.relocations_complete,Some(false),"size{size}");assert!(o.pe_relocations.is_empty());assert!(!o.statements.is_empty());}}
 #[test]fn ambiguous_virtual_tail_and_uncaptured_targets_never_invent_bytes(){for mode in[0,1,2]{let mut v=image();if mode==0{w32(&mut v,0x188+40+12,0x1000);w32(&mut v,0x600,0x1000);}else if mode==1{w32(&mut v,0x188+40+16,4);}else{w32(&mut v,0x600,0xfffff000);w16(&mut v,0x608,0xafff);}let o=inspect(&v,v.len()as u64);assert_eq!(o.pe_relocations[0].target_file_offset,None);assert_eq!(o.relocations_complete,Some(true));}}
 #[test]fn machine_specific_and_misaligned_declarations_stay_uninterpreted(){let mut v=image();w16(&mut v,0x608,0x5008);let o=inspect(&v,v.len()as u64);assert_eq!(o.pe_relocations[0].target_file_offset,None);assert_eq!(relocation_type(5).1,None);w32(&mut v,0x600,0x2001);let o=inspect(&v,v.len()as u64);assert_eq!(o.relocations_complete,Some(false));assert_eq!(o.pe_relocations[0].target_file_offset,None);}
 #[test]fn slot_and_block_bounds_remain_explicit(){let mut v=image();w32(&mut v,0x604,8+130*2);w32(&mut v,0x98+112+44,8+130*2);let o=inspect(&v,v.len()as u64);assert_eq!(o.pe_relocations.len(),128);assert_eq!(o.relocations_complete,Some(false));let mut v=image();for i in 0..33{w32(&mut v,0x600+i*8,0x2000);w32(&mut v,0x604+i*8,8);}w32(&mut v,0x98+112+44,33*8);let o=inspect(&v,v.len()as u64);assert!(o.pe_relocations.is_empty());assert_eq!(o.relocations_complete,Some(false));}
 #[test]fn non_arm64_metadata_does_not_create_instruction_claims_and_cancellation_propagates(){let mut v=image();w16(&mut v,0x84,0x8664);let o=inspect(&v,v.len()as u64);assert_eq!(o.coordinate,"RVA ");assert!(o.statements.is_empty());assert_eq!(o.pe_relocations.len(),2);assert_eq!(o.relocations_complete,Some(true));assert!(observe_with_cancel(&v,true,v.len()as u64,&||true).is_err());}
 #[test]fn typed_projection_preserves_bounds_and_rejects_invented_target(){use super::super::*;let v=image();let o=inspect(&v,v.len()as u64);let d=binary_declarations::derive(&o,v.len()as u64,v.len()as u64,"PE RVA",32768,&||false).unwrap();assert_eq!(get(&d,"relocation_inventory_complete"),&Bool(true));let rows=array(get(&d,"relocations"),"rows");assert_eq!(get(&rows[0],"target_rva"),&Num("8200".into()));assert_eq!(get(&rows[1],"target_rva"),&Null);let mut bad=d.clone();if let Obj(m)=&mut bad{if let Some(Obj(g))=m.get_mut("relocations"){if let Some(Arr(rows))=g.get_mut("rows"){put(&mut rows[0],"target_file_offset",Num("2047".into()));}}}assert!(binary_declarations::validate(&bad,v.len()as u64,v.len()as u64,"PE RVA").is_err());assert_eq!(binary_declarations::derive(&o,v.len()as u64,v.len()as u64,"PE RVA",1,&||false).unwrap(),Null);}
}
