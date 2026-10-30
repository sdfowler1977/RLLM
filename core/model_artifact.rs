//! Read-only tensor-container observations over the caller's bounded file sample.
use super::*;
use std::{collections::BTreeSet, fmt::Write as _};
const HEADER: usize = 1024 * 1024;
const ITEMS: u64 = 4096;
const WORK: usize = 262144;
const OUTPUT: usize = 16384;
fn label(v: &str) -> String { let mut t=String::new(); for c in v.chars().take(160) { for e in c.escape_debug() { t.push(e); } } if v.chars().count()>160 {t.push_str("…");} format!("\"{t}\"") }
fn integer(v:&Json)->Result<u64>{if let Num(n)=v {if !n.is_empty()&&n.bytes().all(|b|b.is_ascii_digit()){return n.parse::<u64>().map_err(|_|"Integer exceeds 64 bits".into());}}Err("Expected unsigned integer".into())}
fn field<'a>(v:&'a Json,k:&str)->Result<&'a Json>{v.get(k)}
fn bits(dtype:&str)->Option<u64>{Some(match dtype {"F4"=>4,"F6_E2M3"|"F6_E3M2"=>6,"BOOL"|"U8"|"I8"|"F8_E5M2"|"F8_E4M3"|"F8_E8M0"|"F8_E4M3FNUZ"|"F8_E5M2FNUZ"=>8,"I16"|"U16"|"F16"|"BF16"=>16,"I32"|"U32"|"F32"=>32,"I64"|"U64"|"F64"|"C64"=>64,_=>return None})}
fn safetensors(data:&[u8],size:u64)->Result<String>{
 let h=u64::from_le_bytes(data.get(..8).ok_or("Incomplete eight-byte header length")?.try_into().unwrap());
 if h==0||h>HEADER as u64{return Err(format!("Header length {h} exceeds supported 1 MiB bound or is empty"));}
 let start=8u64.checked_add(h).ok_or("Header offset overflow")?;if start>size{return Err("Declared header extends beyond file length".into());}
 let bytes=data.get(8..usize::try_from(start).map_err(|_|"Header offset not addressable")?).ok_or("Header is not complete in the inspected prefix; metadata validity is unknown")?;
 if bytes.first()!=Some(&b'{'){return Err("Header must begin with an object opening byte".into());}
 let header=parse(std::str::from_utf8(bytes).map_err(|_|"Header is not UTF-8")?)?;
 let fields=if let Obj(m)=&header{m}else{return Err("Header must be a JSON object".into())};if fields.len()>ITEMS as usize+1{return Err("Tensor/header member count exceeds 4096 bound".into());}
 let mut intervals=Vec::new();let mut rows=String::new();let mut tensor_count=0;let mut elements=0u64;let mut metadata_count=0;
 for (name,v) in fields {
  if name=="__metadata__" {if let Obj(m)=v{if !m.values().all(|v|matches!(v,Str(_))){return Err("Metadata values must be strings".into());}metadata_count=m.len();}else{return Err("Metadata must be a string map".into());}continue;}
  if tensor_count==ITEMS as usize{return Err("Tensor count exceeds 4096 bound".into());}tensor_count+=1;
  let m=if let Obj(m)=v{m}else{return Err("Tensor descriptor must be an object".into())};if m.len()!=3||!m.contains_key("dtype")||!m.contains_key("shape")||!m.contains_key("data_offsets"){return Err("Tensor descriptor requires dtype, shape and data_offsets only".into());}
  let dtype=field(v,"dtype")?.text()?;let width=bits(dtype).ok_or_else(||format!("Unsupported tensor dtype {}; extent not certified",label(dtype)))?;
  let shape=if let Arr(a)=field(v,"shape")?{a}else{return Err("Tensor shape must be an array".into())};if shape.len()>32{return Err("Tensor rank exceeds 32 bound".into());}
  let dims=shape.iter().map(integer).collect::<Result<Vec<_>>>()?;
  let count=if dims.contains(&0){0}else{dims.iter().try_fold(1u64,|n,d|n.checked_mul(*d).ok_or("Tensor shape product overflow"))?};
  let storage_bits=u128::from(count)*u128::from(width);
  if storage_bits%8!=0{return Err("Tensor packed extent does not end at a whole-byte boundary".into());}
  let length=u64::try_from(storage_bits/8).map_err(|_|"Tensor size overflow")?;elements=elements.checked_add(count).ok_or("Aggregate element count overflow")?;
  let offsets=if let Arr(a)=field(v,"data_offsets")?{a}else{return Err("Tensor offsets must be an array".into())};if offsets.len()!=2{return Err("Tensor offsets require two integers".into());}
  let begin=integer(&offsets[0])?;let end=integer(&offsets[1])?;if end<begin||end-begin!=length{return Err("Tensor extent does not match dtype and shape".into());}
  let absolute=start.checked_add(end).ok_or("Tensor absolute range overflow")?;if absolute>size{return Err("Tensor extent exceeds actual file length".into());}
  intervals.push((begin,end));if tensor_count<=24{let _=writeln!(rows,"Tensor {}: dtype {dtype}; shape {dims:?}; elements {count}; file bytes {}..{absolute}",label(name),start+begin);}
 }
 intervals.sort_unstable();let mut cursor=0;for(begin,end)in intervals {if begin!=cursor{return Err("Tensor byte buffer has a hole or overlap".into());}cursor=end;}
 if start.checked_add(cursor)!=Some(size){return Err("Tensor byte buffer does not cover file remainder".into());}
 Ok(format!("Safetensors container observations\nComplete header: {h} bytes; tensor data begins at file byte {start}. Descriptors: {tensor_count}; string metadata keys: {metadata_count}; declared elements: {elements}. Supported dtype/shape lengths and contiguous non-overlapping ranges match actual file length.\n{rows}{}Tensor values, numeric finiteness, training identity, model correctness and inference were not evaluated. No model was loaded.",if tensor_count>24{"Further tensor rows omitted (24-row display bound).\n"}else{""}))
}
struct Reader<'a>{data:&'a[u8],at:usize,be:bool,work:usize}
impl<'a> Reader<'a>{
 fn take(&mut self,n:usize)->Result<&'a[u8]>{let end=self.at.checked_add(n).ok_or("GGUF offset overflow")?;if end>HEADER{return Err("GGUF metadata/table exceeds 1 MiB inspection bound".into());}let b=self.data.get(self.at..end).ok_or("GGUF metadata/table incomplete in supplied bytes")?;self.at=end;Ok(b)}
 fn n(&mut self,n:usize)->Result<u64>{let b=self.take(n)?;Ok(if self.be{b.iter().fold(0,|v,b|(v<<8)|*b as u64)}else{b.iter().enumerate().fold(0,|v,(i,b)|v|((*b as u64)<<(i*8)))})}
 fn u32(&mut self)->Result<u32>{Ok(self.n(4)? as u32)}
 fn string(&mut self)->Result<&'a str>{let len=self.n(8)?;if len>HEADER as u64{return Err("GGUF string exceeds 1 MiB bound".into());}std::str::from_utf8(self.take(len as usize)?).map_err(|_|"GGUF string is not UTF-8".into())}
 fn value(&mut self,kind:u32,depth:usize)->Result<Option<u64>>{
  if self.work==WORK{return Err("GGUF metadata work exceeds 262144 values".into());}self.work+=1;if depth>8{return Err("GGUF metadata nesting exceeds 8".into());}
  match kind {0|1=>Ok(Some(self.n(1)?)),2|3=>Ok(Some(self.n(2)?)),4|5|6=>Ok(Some(self.n(4)?)),7=>{let b=self.n(1)?;if b>1{return Err("GGUF boolean is neither 0 nor 1".into());}Ok(Some(b))},8=>{self.string()?;Ok(None)},9=>{let t=self.u32()?;let count=self.n(8)?;if t>12{return Err("Unknown GGUF array value type".into());}if count>(WORK-self.work)as u64{return Err("GGUF array count exceeds work bound".into());}for _ in 0..count{self.value(t,depth+1)?;}Ok(None)},10|11|12=>Ok(Some(self.n(8)?)),_=>Err("Unknown GGUF metadata value type".into())}
 }
}
fn gguf_type(t:u32)->(&'static str,Option<u64>){match t{0=>("F32",Some(4)),1=>("F16",Some(2)),2=>("Q4_0",None),3=>("Q4_1",None),6=>("Q5_0",None),7=>("Q5_1",None),8=>("Q8_0",None),9=>("Q8_1",None),10=>("Q2_K",None),11=>("Q3_K",None),12=>("Q4_K",None),13=>("Q5_K",None),14=>("Q6_K",None),15=>("Q8_K",None),16=>("IQ2_XXS",None),17=>("IQ2_XS",None),18=>("IQ3_XXS",None),19=>("IQ1_S",None),20=>("IQ4_NL",None),21=>("IQ3_S",None),22=>("IQ2_S",None),23=>("IQ4_XS",None),24=>("I8",Some(1)),25=>("I16",Some(2)),26=>("I32",Some(4)),27=>("I64",Some(8)),28=>("F64",Some(8)),29=>("IQ1_M",None),30=>("BF16",Some(2)),34=>("TQ1_0",None),35=>("TQ2_0",None),39=>("MXFP4",None),_=>("unknown/removed",None)}}
fn gguf(data:&[u8],size:u64)->Result<String>{
 let version_bytes=data.get(4..8).ok_or("Incomplete GGUF version")?;let le=u32::from_le_bytes(version_bytes.try_into().unwrap());let be=u32::from_be_bytes(version_bytes.try_into().unwrap());
 let big=le!=2&&le!=3&&be==3;let version=if big{be}else{le};if version!=2&&version!=3{return Err(format!("Unsupported GGUF version encoding {version}; supported: 2 little-endian / 3 both byte orders"));}
 let mut r=Reader{data,at:8,be:big,work:0};let tensors=r.n(8)?;let metadata=r.n(8)?;if tensors>ITEMS||metadata>ITEMS{return Err("GGUF tensor/metadata count exceeds 4096 bound".into());}
 let mut keys=BTreeSet::new();let mut rows=String::new();let mut alignment=32u64;let mut architecture=None;
 for i in 0..metadata {
  let key=r.string()?;if key.is_empty()||key.len()>65535||!key.is_ascii()||key.split('.').any(|p|p.is_empty()||!p.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_')){return Err("GGUF metadata key is not a supported hierarchical ASCII key".into());}
  if !keys.insert(key){return Err("Duplicate GGUF metadata key".into());}let kind=r.u32()?;
  let selected=if kind==8&&(key=="general.architecture"||key=="general.name"){Some(r.string()?)}else{None};
  let value=if selected.is_some(){None}else{r.value(kind,0)?};
  if key=="general.alignment"{if kind!=4{return Err("GGUF alignment must have UINT32 type".into());}alignment=value.unwrap();if alignment==0||alignment%8!=0||alignment>HEADER as u64{return Err("GGUF alignment must be a positive multiple of 8 within 1 MiB".into());}}
  if key=="general.architecture"{if kind!=8{return Err("GGUF architecture must be a string".into());}architecture=selected;}
  if i<24{let _=writeln!(rows,"Metadata {}: type {kind}{}",label(key),selected.map(|s|format!("; declared {}",label(s))).unwrap_or_default());}
 }
 let mut names=BTreeSet::new();let mut table=Vec::new();
 for _ in 0..tensors{
  let name=r.string()?;if name.len()>64{return Err("GGUF tensor name exceeds 64 bytes".into());}if !names.insert(name){return Err("Duplicate GGUF tensor name".into());}
  let rank=r.u32()?;if rank>4{return Err("GGUF tensor rank exceeds supported 4 dimensions".into());}let mut shape=Vec::new();for _ in 0..rank{let n=r.n(8)?;if n==0{return Err("GGUF tensor dimension is zero".into());}shape.push(n);}
  let elements=shape.iter().try_fold(1u64,|n,d|n.checked_mul(*d).ok_or("GGUF shape product overflow"))?;let dtype=r.u32()?;let offset=r.n(8)?;if offset%alignment!=0{return Err("GGUF tensor offset is not aligned".into());}table.push((name,shape,elements,dtype,offset));
 }
 let table_end=r.at as u64;let data_start=table_end.checked_add((alignment-table_end%alignment)%alignment).ok_or("GGUF data start overflow")?;if data_start>size{return Err("GGUF aligned data start exceeds actual file length".into());}
 let mut ranges=Vec::new();let mut known=0;let mut tensor_rows=String::new();
 for(i,(name,shape,elements,dtype,offset))in table.iter().enumerate(){let begin=data_start.checked_add(*offset).ok_or("GGUF absolute offset overflow")?;if begin>size{return Err("GGUF tensor offset beyond actual file length".into());}let(type_name,width)=gguf_type(*dtype);
  let extent=if let Some(width)=width{let len=elements.checked_mul(width).ok_or("GGUF tensor extent overflow")?;let end=begin.checked_add(len).ok_or("GGUF tensor extent overflow")?;if end>size{return Err("GGUF tensor extent beyond actual file length".into());}ranges.push((begin,end));known+=1;format!("{begin}..{end}")}else{format!("start {begin}; encoded byte length not validated")};
  if i<24{let _=writeln!(tensor_rows,"Tensor {}: type {dtype}({type_name}); shape {shape:?}; elements {elements}; file bytes {extent}",label(name));}
 }
 ranges.sort_unstable();for pair in ranges.windows(2){if pair[0].1>pair[1].0{return Err("Known GGUF tensor extents overlap".into());}}
 Ok(format!("GGUF container observations\nVersion {version}; {} byte order selected from encoded version. Complete metadata/table ends at byte {table_end}; aligned tensor data starts at byte {data_start}; alignment {alignment}. {metadata} metadata entries; {tensors} tensor descriptors; {known} scalar-storage extents checked against actual file length. Declared architecture: {}.\n{rows}{}{tensor_rows}{}Quantized/unknown type lengths, complete data coverage, tensor values, runtime compatibility and inference are not certified. No model was loaded.",if big{"big"}else{"little"},architecture.map(label).unwrap_or_else(||"not reported".into()),if metadata>24{"Further metadata rows omitted.\n"}else{""},if tensors>24{"Further tensor rows omitted.\n"}else{""}))
}
/// Returns None for non-candidates. The filename only selects safetensors;
/// validity comes from the current bytes and actual file size.
pub(super) fn analyze(data:&[u8],complete:bool,size:u64,safe:bool)->Option<String>{
 let name=if data.starts_with(b"GGUF"){"GGUF"}else if safe{"Safetensors"}else{return None};
 let result=if name=="GGUF"{gguf(data,size)}else{safetensors(data,size)};
 let scope=if complete{"entire file bytes supplied"}else{"prefix supplied; unseen tensor values remain unknown"};
 let mut out=format!("Model artifact inspection ({scope}; actual file length {size})\n{}
",match result{Ok(v)=>v,Err(e)=>format!("{name} structural observation refused: {e}. No whole-container validity claim.")});
 if out.len()>OUTPUT{let mut n=OUTPUT-64;while !out.is_char_boundary(n){n-=1;}out.truncate(n);out.push_str("\nModel artifact report abbreviated at 16 KiB.\n");}Some(out)
}
#[cfg(test)]mod checks{
 use super::*;
 fn safe(h:&str,payload:usize)->Vec<u8>{let mut b=(h.len()as u64).to_le_bytes().to_vec();b.extend(h.as_bytes());b.resize(b.len()+payload,0);b}
 fn gs(b:&mut Vec<u8>,s:&str,be:bool){let n=s.len()as u64;b.extend(if be{n.to_be_bytes()}else{n.to_le_bytes()});b.extend(s.as_bytes());}
 fn gu(be:bool,ty:u32,offset:u64)->Vec<u8>{let mut b=b"GGUF".to_vec();b.extend(if be{3u32.to_be_bytes()}else{3u32.to_le_bytes()});for n in [1u64,1]{b.extend(if be{n.to_be_bytes()}else{n.to_le_bytes()});}gs(&mut b,"general.architecture",be);b.extend(if be{8u32.to_be_bytes()}else{8u32.to_le_bytes()});gs(&mut b,"fixture",be);gs(&mut b,"weight",be);b.extend(if be{1u32.to_be_bytes()}else{1u32.to_le_bytes()});b.extend(if be{2u64.to_be_bytes()}else{2u64.to_le_bytes()});b.extend(if be{ty.to_be_bytes()}else{ty.to_le_bytes()});b.extend(if be{offset.to_be_bytes()}else{offset.to_le_bytes()});let n=(b.len()+31)/32*32;b.resize(n+8,0);b}
 #[test]fn tensors_header_shape_ranges_and_scalar_empty(){let h=r#"{"weight":{"dtype":"F32","shape":[2],"data_offsets":[0,8]},"__metadata__":{"purpose":"fixture"}}"#;let b=safe(h,8);let o=analyze(&b,true,b.len()as u64,true).unwrap();assert!(o.contains("Descriptors: 1"));assert!(o.contains("shape [2]"));assert!(!o.contains("refused"));let b=safe(r#"{"scalar":{"dtype":"I16","shape":[],"data_offsets":[0,2]},"empty":{"dtype":"F32","shape":[0,999],"data_offsets":[0,0]}}"#,2);assert!(!analyze(&b,true,b.len()as u64,true).unwrap().contains("refused"));}
 #[test]fn tensors_malformed_duplicate_overflow_holes(){for h in [r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[1,5]}}"#,r#"{"a":{"dtype":"F32","shape":[18446744073709551615,2],"data_offsets":[0,8]}}"#,r#"{"a":{"dtype":"F32","shape":[2],"data_offsets":[0,4]}}"#,r#"{"a":{"dtype":"F32","dtype":"I32","shape":[2],"data_offsets":[0,8]}}"#,r#"{"__metadata__":{"a":1}}"#]{let b=safe(h,8);assert!(analyze(&b,true,b.len()as u64,true).unwrap().contains("refused"));}}
 #[test]fn tensors_prefix_can_certify_header_not_values(){let b=safe(r#"{"w":{"dtype":"U8","shape":[1000],"data_offsets":[0,1000]}}"#,1000);let head=&b[..b.len()-1000];let o=analyze(head,false,b.len()as u64,true).unwrap();assert!(o.contains("prefix supplied"));assert!(o.contains("Descriptors: 1"));assert!(!o.contains("refused"));assert!(analyze(&b[..10],false,b.len()as u64,true).unwrap().contains("not complete"));let mut b=vec![0u8;8];b[..8].copy_from_slice(&((HEADER as u64)+1).to_le_bytes());assert!(analyze(&b,true,b.len()as u64,true).unwrap().contains("bound"));}
 #[test]fn gguf_two_orders_scalar_quantized_and_versions(){for be in [false,true]{let b=gu(be,0,0);let o=analyze(&b,true,b.len()as u64,false).unwrap();assert!(o.contains("1 scalar-storage extents"),"{o}");assert!(o.contains("fixture"));let b=gu(be,21,0);let o=analyze(&b,true,b.len()as u64,false).unwrap();assert!(o.contains("IQ3_S"));assert!(o.contains("encoded byte length not validated"));assert!(!o.contains("refused"));}let mut b=gu(false,0,0);b[4..8].copy_from_slice(&2u32.to_le_bytes());assert!(analyze(&b,true,b.len()as u64,false).unwrap().contains("Version 2"));b[4..8].copy_from_slice(&1u32.to_le_bytes());assert!(analyze(&b,true,b.len()as u64,false).unwrap().contains("Unsupported GGUF version"));}
 #[test]fn gguf_bounds_and_incomplete_are_scoped(){let b=gu(false,0,1);assert!(analyze(&b,true,b.len()as u64,false).unwrap().contains("not aligned"));let b=gu(false,0,32);assert!(analyze(&b,true,b.len()as u64,false).unwrap().contains("beyond actual"));let b=gu(false,0,0);assert!(analyze(&b[..30],false,b.len()as u64,false).unwrap().contains("incomplete"));let mut b=gu(false,0,0);b[8..16].copy_from_slice(&u64::MAX.to_le_bytes());assert!(analyze(&b,true,b.len()as u64,false).unwrap().contains("count exceeds"));assert!(analyze(b"ordinary",true,8,false).is_none());}
}
