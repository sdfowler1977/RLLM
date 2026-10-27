//! Original bounded container-envelope observations; no decoding or execution.
use std::fmt::Write as _;
const INPUT_LIMIT: usize = 128 * 1024;
const HEADER_LIMIT: usize = 64;
const OUTPUT_LIMIT: usize = 8191;

struct Scope<'a> { bytes: &'a [u8], total: Option<u64>, whole: bool }
fn u16le(bytes: &[u8], at: usize) -> Option<u16> { Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?)) }
fn u32le(bytes: &[u8], at: usize) -> Option<u32> { Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?)) }
fn bounded(text: &str, limit: usize) -> &str { let mut end=text.len().min(limit);while !text.is_char_boundary(end){end-=1;}&text[..end] }
fn identifier(bytes: &[u8]) -> String {
    let mut value=String::new();for b in bytes {if (b' '..=b'~').contains(b)&&!matches!(*b,b'"'|b'\\'){value.push(*b as char);}else{let _=write!(value,"\\x{b:02x}");}}value
}
fn name_text(text: &str) -> String {
    let mut result=String::new();for c in text.chars(){let piece=if c.is_control()||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {format!("\\u{{{:x}}}",c as u32)}else if matches!(c,'"'|'\\'){format!("\\{c}")}else{c.to_string()};if result.len()+piece.len()>128{result.push_str("…");break;}result.push_str(&piece);}result
}
fn wave(scope:&Scope<'_>, out:&mut String) {
    let bytes=scope.bytes;
    if bytes.len()<12 {out.push_str("RIFF prefix observed; the 12-byte RIFF/form header is incomplete in examined bytes. No WAVE classification or chunk traversal.\n");return;}
    if &bytes[8..12]!=b"WAVE" {let _=writeln!(out,"RIFF form \"{}\" observed; this observer handles WAVE only and did not interpret other form payloads.",identifier(&bytes[8..12]));return;}
    let declared=u32le(bytes,4).unwrap() as u64;
    let end=match declared.checked_add(8){Some(end)=>end,None=>{out.push_str("RIFF extent overflow; traversal refused.\n");return;}};
    let _=writeln!(out,"RIFF/WAVE little-endian envelope: declared RIFF payload {declared} bytes; declared container end offset {end}.");
    if declared<4 {out.push_str("Malformed RIFF extent: the declared payload cannot contain the WAVE form identifier.\n");return;}
    if let Some(total)=scope.total {if end>total{let _=writeln!(out,"Declared RIFF extent exceeds the reported source length by {} bytes: the source is truncated or its header is inconsistent.",end-total);}else if end<total{let _=writeln!(out,"{} bytes follow the declared RIFF extent; they are not interpreted as WAVE chunks.",total-end);}}
    let mut offset=12u64;let mut count=0;let mut formats=0;let mut data_chunks=0;
    while offset<end {
        if count==HEADER_LIMIT {let _=writeln!(out,"Chunk traversal stopped at the {HEADER_LIMIT}-header work bound; remaining declared chunks are unexamined.");break;}
        let header_end=match offset.checked_add(8){Some(value)=>value,None=>{out.push_str("Chunk-header offset overflow.\n");break;}};
        if header_end>end {let _=writeln!(out,"Malformed RIFF tail: {} bytes remain in the declared extent, insufficient for a chunk header.",end-offset);break;}
        if header_end>bytes.len() as u64 {let _=writeln!(out,"Chunk header at offset {offset} is unavailable or truncated in the examined bytes; no later chunks were inferred.");break;}
        let at=offset as usize;let id=&bytes[at..at+4];let length=u32le(bytes,at+4).unwrap() as u64;
        let payload_end=match header_end.checked_add(length){Some(value)=>value,None=>{out.push_str("Chunk-payload offset overflow.\n");break;}};
        let padded_end=match payload_end.checked_add(length&1){Some(value)=>value,None=>{out.push_str("Chunk-padding offset overflow.\n");break;}};
        count+=1;
        let observed=(bytes.len() as u64).saturating_sub(header_end).min(length);
        let _=writeln!(out,"Chunk {count}: id \"{}\"; header offset {offset}; payload offset {header_end}; declared length {length}; observed payload prefix {observed} bytes.",identifier(id));
        if payload_end>end||padded_end>end {out.push_str("Malformed chunk: payload or required odd-length padding exceeds the declared RIFF extent; traversal stopped.\n");break;}
        if id==b"fmt " {
            formats+=1;
            if length<16 {out.push_str("Malformed fmt chunk: fewer than 16 declared bytes for base format fields.\n");}
            else if observed<16 {out.push_str("fmt base fields are incomplete in the examined prefix; no codec or rate was inferred.\n");}
            else {
                let at=header_end as usize;let codec=u16le(bytes,at).unwrap();let channels=u16le(bytes,at+2).unwrap();let rate=u32le(bytes,at+4).unwrap();let byte_rate=u32le(bytes,at+8).unwrap();let align=u16le(bytes,at+12).unwrap();let bits=u16le(bytes,at+14).unwrap();
                let label=match codec {1=>"PCM",2=>"ADPCM",3=>"IEEE float",6=>"A-law",7=>"mu-law",17=>"IMA ADPCM",0xfffe=>"extensible (subformat not resolved here)",_=>"unrecognized format tag"};
                let _=writeln!(out,"Observed fmt fields: format tag {codec} ({label}); channels {channels}; sample rate {rate} Hz; byte rate {byte_rate}; block alignment {align}; bits-per-sample field {bits}.");
                if channels==0||rate==0||align==0 {out.push_str("A zero channels, sample-rate or block-alignment field was observed; no playable-format claim.\n");}
                if length>16 {let _=writeln!(out,"{} additional declared fmt bytes are not interpreted; the format tag alone does not establish a decoder or effective extensible codec.",length-16);}
            }
        }
        if id==b"data" {data_chunks+=1;out.push_str("Audio payload was not decoded, played, or assessed for signal validity.\n");}
        if padded_end>bytes.len() as u64 {let _=writeln!(out,"Chunk payload or padding continues beyond examined offset {}; traversal stops rather than inferring unseen headers.",bytes.len());break;}
        offset=padded_end;
    }
    let _=writeln!(out,"Observed chunk headers {count}; fmt headers {formats}; data headers {data_chunks}. Duplicate format/data chunks are reported separately, not silently reconciled.");
    if offset==end {out.push_str("Envelope traversal reached the declared RIFF end within examined bytes; this is not audio decoding or complete media validation.\n");}
}

#[derive(Debug,PartialEq)]
enum LebError { Truncated, Overflow }
fn leb32(bytes:&[u8], start:usize, end:usize)->Result<(u32,usize),LebError>{
    let mut value=0u32;
    for i in 0..5usize {
        let at=start.checked_add(i).ok_or(LebError::Overflow)?;
        if at>=end {return Err(LebError::Truncated);}
        let byte=*bytes.get(at).ok_or(LebError::Truncated)?;
        if i==4&&(byte&0xf0)!=0 {return Err(LebError::Overflow);}
        value|=((byte&0x7f) as u32)<<(i*7);
        if byte&0x80==0{return Ok((value,i+1));}
    }
    Err(LebError::Overflow)
}
fn section_name(id:u8)->&'static str{match id{0=>"custom",1=>"type",2=>"import",3=>"function",4=>"table",5=>"memory",6=>"global",7=>"export",8=>"start",9=>"element",10=>"code",11=>"data",12=>"data count",13=>"tag (extension)",_=>"unknown identifier"}}
fn custom_name(scope:&Scope<'_>, start:usize, payload_end:u64, out:&mut String){
    let available=(scope.bytes.len() as u64).min(payload_end) as usize;
    match leb32(scope.bytes,start,available){
        Err(LebError::Truncated)=>out.push_str("Custom-name length is unavailable or truncated in the examined section prefix.\n"),
        Err(LebError::Overflow)=>out.push_str("Malformed custom-name length: u32 LEB overflow or excessive encoded length.\n"),
        Ok((length,used))=>{
            let name_start=match start.checked_add(used){Some(v)=>v,None=>{out.push_str("Custom-name offset overflow.\n");return;}};
            let end=match (name_start as u64).checked_add(length as u64){Some(v)=>v,None=>{out.push_str("Custom-name extent overflow.\n");return;}};
            if end>payload_end{out.push_str("Malformed custom-name envelope: declared name exceeds its section payload.\n");return;}
            let observed_end=end.min(scope.bytes.len() as u64) as usize;
            let observed=&scope.bytes[name_start..observed_end];let complete=end<=scope.bytes.len() as u64;
            let text=match std::str::from_utf8(observed){
                Ok(value)=>value,
                Err(error) if !complete&&error.error_len().is_none()=>std::str::from_utf8(&observed[..error.valid_up_to()]).unwrap(),
                Err(error)=>{let _=writeln!(out,"Custom-name bytes are invalid UTF-8 at observed name offset {}; name not displayed.",error.valid_up_to());return;},
            };
            let _=writeln!(out,"Custom section name {}: \"{}\"; declared UTF-8 byte length {length}. Display escaped and bounded; custom payload contents not interpreted.",if complete{"observed"}else{"prefix only (remaining bytes unseen)"},name_text(text));
        }
    }
}
fn wasm(scope:&Scope<'_>,out:&mut String){
    let bytes=scope.bytes;
    if bytes.len()<8 {out.push_str("WebAssembly magic observed; the 8-byte magic/version header is incomplete in examined bytes. No sections inferred.\n");return;}
    if bytes[4..8]!=[1,0,0,0]{let _=writeln!(out,"WebAssembly magic with version bytes {:02x?}; only version 1 section envelopes are handled. No compatibility fallback or execution.",&bytes[4..8]);return;}
    out.push_str("WebAssembly version 1 magic/version observed. Inventory covers section envelopes only, not opcode/type/import/export semantics, section ordering, feature support, or executable validity.\n");
    let mut offset=8usize;let mut count=0;
    while offset<bytes.len(){
        if count==HEADER_LIMIT{let _=writeln!(out,"Section inventory stopped at {HEADER_LIMIT} headers; remaining bytes are unexamined as sections.");break;}
        let id=bytes[offset];let length_start=offset+1;
        let (length,used)=match leb32(bytes,length_start,bytes.len()){
            Ok(value)=>value,
            Err(LebError::Truncated)=>{let _=writeln!(out,"Section length at offset {length_start} is truncated in examined bytes; no section extent inferred.");break;},
            Err(LebError::Overflow)=>{let _=writeln!(out,"Malformed section length at offset {length_start}: u32 LEB overflow or excessive encoded length.");break;},
        };
        let start=match length_start.checked_add(used){Some(v)=>v,None=>{out.push_str("Section-header offset overflow.\n");break;}};
        let end=match (start as u64).checked_add(length as u64){Some(v)=>v,None=>{out.push_str("Section-payload extent overflow.\n");break;}};
        count+=1;
        let observed=(bytes.len()-start).min(length as usize);
        let _=writeln!(out,"Section {count}: id {id} ({}); header offset {offset}; payload offset {start}; declared length {length}; end offset {end}; observed payload prefix {observed} bytes.",section_name(id));
        if let Some(total)=scope.total{if end>total{out.push_str("Declared section exceeds the reported source length: truncated source or inconsistent section envelope.\n");break;}}
        if id==0 {custom_name(scope,start,end,out);}
        if end>bytes.len() as u64 {out.push_str("Section payload extends beyond examined prefix; unseen payload and later sections were not inferred.\n");break;}
        offset=end as usize;
    }
    let _=writeln!(out,"Observed section headers: {count}. Ordinary payloads were skipped by checked declared length, not decoded or executed.");
    if offset==bytes.len(){if scope.whole{out.push_str("Envelope traversal reached the supplied EOF; semantic module validation was not performed.\n");}else{out.push_str("Traversal reached the examined prefix boundary; further sections or malformed trailing bytes may exist.\n");}}
}

/// Observe a held prefix. `total_bytes` is the caller's source-size observation;
/// `complete` is true only when all source bytes were supplied. Neither is an
/// attestation that the underlying source could not change outside this call.
pub(super) fn analyze(data:&[u8],complete:bool,total_bytes:u64)->String{
    let count=data.len().min(INPUT_LIMIT);let consistent=(data.len() as u64)<=total_bytes&&(!complete||data.len() as u64==total_bytes);
    let whole=consistent&&complete&&count==data.len();
    let scope=Scope{bytes:&data[..count],total:if consistent{Some(total_bytes)}else{None},whole};
    let mut out=format!("Binary container observations\nReported source length {total_bytes} bytes; supplied {} bytes; examined {count} bytes (limit {INPUT_LIMIT}). Scope: {}.\n",data.len(),if whole{"complete supplied source"}else{"examined prefix only; unexamined bytes unknown"});
    if !consistent{out.push_str("Supplied byte count, completion flag and reported total are inconsistent. File-length comparisons are withheld; observed byte fields remain observations only.\n");}
    if scope.bytes.starts_with(b"RIFF"){wave(&scope,&mut out);}
    else if scope.bytes.starts_with(b"\0asm"){wasm(&scope,&mut out);}
    else if scope.bytes.starts_with(b"RF64")||scope.bytes.starts_with(b"RIFX"){out.push_str("An RF64/RIFX signature was observed. This observer does not interpret its extended-size or big-endian format; no little-endian WAVE fields were inferred.\n");}
    else if count<4{out.push_str("Fewer than four examined bytes: insufficient signature evidence for RIFF/WAVE or WebAssembly.\n");}
    else{out.push_str("No supported RIFF/WAVE or WebAssembly version-1 envelope was identified. Other formats remain unclassified by this observer.\n");}
    if out.len()>OUTPUT_LIMIT{
        let mut result=bounded(&out,OUTPUT_LIMIT-1300).to_string();
        result.push_str("\nReport abbreviated at the output bound; middle inventory rows omitted. Final observations follow:\n");
        let mut tail=out.len().saturating_sub(1024);while !out.is_char_boundary(tail){tail+=1;}
        if let Some(line)=out[tail..].find('\n'){tail+=line+1;}
        result.push_str(&out[tail..]);result
    }else{out}
}

#[cfg(test)]mod checks{
    use super::*;
    fn wave_bytes()->Vec<u8>{let mut b=b"RIFF".to_vec();b.extend(40u32.to_le_bytes());b.extend(b"WAVEfmt ");b.extend(16u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(2u16.to_le_bytes());b.extend(48000u32.to_le_bytes());b.extend(192000u32.to_le_bytes());b.extend(4u16.to_le_bytes());b.extend(16u16.to_le_bytes());b.extend(b"data");b.extend(4u32.to_le_bytes());b.extend([0;4]);b}
    fn wasm_bytes()->Vec<u8>{b"\0asm\x01\0\0\0".to_vec()}
    #[test]fn wave_fields_and_prefix_have_distinct_scope(){let b=wave_bytes();let text=analyze(&b,true,b.len() as u64);assert!(text.contains("format tag 1 (PCM); channels 2; sample rate 48000 Hz; byte rate 192000; block alignment 4"));assert!(text.contains("reached the declared RIFF end"));let text=analyze(&b[..44],false,b.len() as u64);assert!(text.contains("observed payload prefix 0 bytes"));assert!(text.contains("continues beyond examined"));assert!(!text.contains("reached the declared RIFF end"));}
    #[test]fn wave_malformed_and_other_forms_are_not_inferred(){let mut b=wave_bytes();b[40..44].copy_from_slice(&u32::MAX.to_le_bytes());assert!(analyze(&b,true,b.len() as u64).contains("Malformed chunk"));let b=b"RIFF\x03\0\0\0WAVE";assert!(analyze(b,true,12).contains("Malformed RIFF extent"));assert!(analyze(b"RIFF",false,100).contains("header is incomplete"));assert!(analyze(b"RIFXxxxxxxxxWAVE",true,16).contains("does not interpret"));}
    #[test]fn wasm_sections_custom_name_and_unknown_are_inventory(){let mut b=wasm_bytes();b.extend([0,5,3]);b.extend("雪".as_bytes());b.push(0xab);b.extend([10,1,0,200,0]);let text=analyze(&b,true,b.len() as u64);assert!(text.contains("name observed: \"雪\""));assert!(text.contains("id 10 (code)"));assert!(text.contains("id 200 (unknown identifier)"));assert!(text.contains("semantic module validation was not performed"));}
    #[test]fn wasm_leb_and_custom_bounds_are_checked(){assert_eq!(leb32(&[0xff,0xff,0xff,0xff,0x0f],0,5),Ok((u32::MAX,5)));assert_eq!(leb32(&[0xff,0xff,0xff,0xff,0x1f],0,5),Err(LebError::Overflow));assert_eq!(leb32(&[0x80;6],0,6),Err(LebError::Overflow));assert_eq!(leb32(&[0x80],0,1),Err(LebError::Truncated));let mut b=wasm_bytes();b.extend([0,2,3,b'a']);assert!(analyze(&b,true,b.len() as u64).contains("name exceeds its section"));let mut b=wasm_bytes();b.extend([1,0xff,0xff,0xff,0xff,0x1f]);assert!(analyze(&b,true,b.len() as u64).contains("u32 LEB overflow"));}
    #[test]fn wasm_sample_total_and_version_limits_are_honest(){let mut b=wasm_bytes();b.extend([10,50,0]);let text=analyze(&b,false,100);assert!(text.contains("extends beyond examined prefix"));assert!(!text.contains("supplied EOF"));assert!(analyze(&b,true,b.len() as u64).contains("exceeds the reported source"));assert!(analyze(&b,true,999).contains("inconsistent"));let mut other=wasm_bytes();other[4]=2;assert!(analyze(&other,true,8).contains("only version 1"));}
    #[test]fn header_input_and_output_work_bounds(){let mut b=wasm_bytes();for _ in 0..HEADER_LIMIT+1{b.extend([200,0]);}let text=analyze(&b,true,b.len() as u64);assert!(text.contains("stopped at 64 headers"));assert!(text.len()<8192);let data=vec![0;INPUT_LIMIT+1];let text=analyze(&data,true,data.len() as u64);assert!(text.contains("prefix only"));assert!(text.len()<8192);assert!(analyze(b"",true,0).contains("insufficient signature"));}
}
