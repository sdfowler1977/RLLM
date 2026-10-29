//! Original read-only observations of declared PE image layout and imports.
use std::fmt::{self,Write as _};
const INPUT_LIMIT:usize=4*1024*1024;
const OUTPUT_LIMIT:usize=16384;
const SECTION_LIMIT:usize=96;
const DLL_LIMIT:usize=64;
const IMPORT_LIMIT:usize=128;
const NAME_LIMIT:usize=256;
#[derive(Debug)]enum Issue{Cancelled,Work,Missing(&'static str),Invalid(&'static str),Padding(u64)}
impl fmt::Display for Issue{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{match self{Self::Cancelled=>f.write_str("inspection cancelled"),Self::Work=>f.write_str("logical work bound reached; remaining data unexamined"),Self::Missing(s)|Self::Invalid(s)=>f.write_str(s),Self::Padding(p)=>write!(f,"raw-padding candidate file offset {p}; outside declared virtual size, loaded RVA unresolved")}}}
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
#[derive(Clone,Copy)]struct Section{rva:u64,virtual_size:u64,raw:u64,raw_size:u64,raw_valid:bool}
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
fn observe(r:Reader<'_>,out:&mut Report,b:&mut Budget<'_>)->PResult<()>{
 b.step(1)?;if r.slice(0,2)?!=b"MZ"{row!(out,"No MZ DOS signature observed; this observer does not classify other formats.");return Ok(());}let pe=r.u32(0x3c)?as u64;row!(out,"DOS MZ signature; declared PE signature file offset{pe}.");if r.slice(pe,4)?!=b"PE\0\0"{row!(out,"No PE signature at the declared offset; MZ alone is not a PE classification.");return Ok(());}
 let coff=pe+4;r.slice(coff,20)?;let kind=r.u16(coff)?;let count=r.u16(coff+2)?as usize;let optional_size=r.u16(coff+16)?as u64;let flags=r.u16(coff+18)?;row!(out,"PE signature and COFF fields: machine0x{kind:04x}({}), sections{count}, optional-header bytes{optional_size}, characteristics0x{flags:04x}.",machine(kind));
 let optional=Optional{r,at:coff+20,size:optional_size};let magic=optional.u16(0)?;let wide=match magic{0x10b=>false,0x20b=>true,_=>{row!(out,"Unsupported optional-header magic0x{magic:04x}; format-specific fields and imports unexamined.");return Ok(());}};
 let entry=optional.u32(16)?;let image_base=if wide{optional.u64(24)?}else{optional.u32(28)?as u64};let section_alignment=optional.u32(32)?;let file_alignment=optional.u32(36)?;let image_size=optional.u32(56)?;let headers=optional.u32(60)?as u64;let subsystem=optional.u16(68)?;let dll_flags=optional.u16(70)?;let directories=optional.u32(if wide{108}else{92})?as u64;let directory_start=if wide{112}else{96};
 row!(out,"{} optional fields: entryRVA0x{entry:x}, preferred image base0x{image_base:x}, image bytes{image_size}, header bytes{headers}, section/file alignment{section_alignment}/{file_alignment}, subsystem{subsystem}, DLLflags0x{dll_flags:04x}, directory count{directories}.",if wide{"PE32+"}else{"PE32"});
 if section_alignment==0||file_alignment==0||!file_alignment.is_power_of_two(){row!(out,"Alignment declarations are zero or non-power-of-two; no loader validity claim.");}
 let available=optional_size.saturating_sub(directory_start)/8;if directories>available{row!(out,"Directory count exceeds declared optional-header capacity{available}; only individually available entries can be observed.");}
 if directories>4{match(optional.u32(directory_start+4*8),optional.u32(directory_start+4*8+4)){(Ok(file),Ok(size))=>row!(out,"Certificate-directory declaration: FILE offset{file}, size{size}; not an RVA. Certificate bytes/signature/trust unexamined."),_=>row!(out,"Certificate-directory fields unavailable; no signature or trust inference.")}}
 let table=optional.at.checked_add(optional_size).ok_or(Issue::Invalid("section-table offset overflow"))?;let table_bytes=(count as u64).checked_mul(40).ok_or(Issue::Invalid("section-table size overflow"))?;let mut complete_table=count<=SECTION_LIMIT&&r.slice(table,table_bytes).is_ok();let mut sections=Vec::with_capacity(count.min(SECTION_LIMIT));
 for i in 0..count.min(SECTION_LIMIT){b.step(1)?;let at=table+i as u64*40;let s=match r.slice(at,40){Ok(s)=>s,Err(e)=>{row!(out,"Section header{i} unavailable: {e}; later headers unexamined.");complete_table=false;break;}};let get=|o|u32::from_le_bytes(s[o..o+4].try_into().unwrap())as u64;let section=Section{virtual_size:get(8),rva:get(12),raw_size:get(16),raw:get(20),raw_valid:r.extent(get(20),get(16)).is_ok()};let end_name=s[..8].iter().position(|v|*v==0).unwrap_or(8);row!(out,"Section{i} \"{}\": RVA0x{:x}, virtual bytes{}, raw file[{},{}), flags0x{:08x}.",escaped(&s[..end_name]),section.rva,section.virtual_size,section.raw,section.raw+section.raw_size,get(36));
  if !section.raw_valid{row!(out,"Section{i} raw extent exceeds reported source; mapping unresolved.");}if section.rva+section.virtual_size>1u64<<32{row!(out,"Section{i} virtual extent exceeds32-bit RVA width; mapping unresolved.");}if section.raw_size>section.virtual_size{row!(out,"Section{i} includes{} raw bytes beyond declared virtual size; padding is not assumed loaded.",section.raw_size-section.virtual_size);}if section.virtual_size>section.raw_size{row!(out,"Section{i} declares{} virtual-only bytes; no source zero bytes synthesized.",section.virtual_size-section.raw_size);}sections.push(section);
 }
 row!(out,"Section headers declared{count}, observed{}; complete table{}. Header-table file extent[{},{}).",sections.len(),complete_table,table,table+table_bytes);
 if count>SECTION_LIMIT{row!(out,"Section observation bound96 reached; unseen section mappings make import references unresolved.");}
 for(i,s)in sections.iter().enumerate(){if s.raw_size>0&&s.raw<headers{row!(out,"Section{i} raw file extent aliases declared header bytes; this is layout data, not unique-RVA ambiguity by itself.");}for(j,t)in sections[..i].iter().enumerate(){b.step(1)?;if s.raw_size>0&&t.raw_size>0&&s.raw<t.raw+t.raw_size&&t.raw<s.raw+s.raw_size{row!(out,"Sections{j}/{i} raw file extents alias; no loader or memory behavior inferred.");}}}
 if out.full{return Ok(());}let image=Image{r,sections,headers,complete_table,wide};
 if directories<2{row!(out,"Observed directory count declares no import-directory entry; unavailable declarations are not inferred.");return Ok(());}
 let import_rva=optional.u32(directory_start+8)?as u64;let import_size=optional.u32(directory_start+12)?as u64;imports(&image,import_rva,import_size,out,b)
}
/// Analyze the already held inspection bytes; no binary, dependency or table is
/// loaded, executed, fetched, or treated as trusted instructions.
pub(super) fn analyze(data:&[u8],complete:bool,total:u64,cancelled:&impl Fn()->bool)->Result<String,String>{
 let count=data.len().min(INPUT_LIMIT);let consistent=data.len()as u64<=total&&(!complete||data.len()as u64==total);let whole=consistent&&complete&&count==data.len();let mut out=Report::new();row!(out,"PE image observations (untrusted declared metadata, not executable behavior)");row!(out,"Reported source{total}bytes; supplied{}; examined{count} (existing4MiB bound). Scope:{}.",data.len(),if whole{"complete supplied input"}else{"prefix only; unseen bytes unknown"});
 if !consistent{row!(out,"Supplied length/completion/total inconsistent; whole-source extent comparisons withheld.");}let reader=Reader{bytes:&data[..count],total:if consistent{Some(total)}else{None}};let mut budget=Budget{left:1_000_000,cancelled};
 match observe(reader,&mut out,&mut budget){Ok(())=>{},Err(Issue::Cancelled)=>return Err("Inspection cancelled during PE observations".into()),Err(e)=>row!(out,"PE observation incomplete: {e}. No unavailable table/name is treated as an observed empty result.")}
 if cancelled(){return Err("Inspection cancelled during PE observations".into());}row!(out,"No binary or import was loaded/executed/fetched. Signatures, loader validity, disassembly, exports/relocations/debug data and full PE semantics remain unexamined.");Ok(out.finish())
}
