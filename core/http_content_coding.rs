//! HTTP representation coding; bytes are never paths or executable content.
use super::*;
pub(super) fn coding(value:&str)->Result<bool>{match value.trim_matches([' ','\t']).to_ascii_lowercase().as_str(){"identity"=>Ok(false),"gzip"|"x-gzip"=>Ok(true),_=>Err("Unsupported or stacked HTTP content coding".into())}}
pub(super) fn header(raw:&[u8])->Result<bool>{let mut rest=raw;for _ in 0..=4{let end=rest.windows(4).position(|v|v==b"\r\n\r\n").ok_or("Incomplete coded HTTP headers")?;let h=std::str::from_utf8(&rest[..end]).map_err(|_|"Invalid coded HTTP headers")?;let mut lines=h.split("\r\n");let code=lines.next().and_then(|s|s.split_whitespace().nth(1)).and_then(|s|s.parse::<u16>().ok()).ok_or("Invalid coded HTTP status")?;if (100..200).contains(&code){rest=&rest[end+4..];continue;}let mut value=None;for line in lines{let(k,v)=line.split_once(':').ok_or("Invalid coded HTTP field")?;if k.eq_ignore_ascii_case("content-encoding"){if value.is_some(){return Err("Duplicate HTTP Content-Encoding".into());}value=Some(coding(v)?);}}return Ok(value.unwrap_or(false));}Err("Excessive coded informational responses".into())}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub(super) enum ErrorKind{Response,OutputLimit,Resource}
#[derive(Debug)]pub(super) struct DecodeError{pub kind:ErrorKind,pub message:String}
impl From<&str> for DecodeError{fn from(s:&str)->Self{Self{kind:ErrorKind::Response,message:s.into()}}}
impl From<String> for DecodeError{fn from(s:String)->Self{Self{kind:ErrorKind::Response,message:s}}}
fn resource(message:&str)->DecodeError{DecodeError{kind:ErrorKind::Resource,message:message.into()}}
fn output(message:&str)->DecodeError{DecodeError{kind:ErrorKind::OutputLimit,message:message.into()}}
fn charge(work:&mut usize,n:usize)->std::result::Result<(),DecodeError>{if n>*work{*work=0;return Err(resource("HTTP gzip work bound exceeded"));}*work-=n;Ok(())}
fn crc(data:&[u8])->u32{let mut c=!0u32;for b in data{c^=u32::from(*b);for _ in 0..8{c=(c>>1)^((0u32.wrapping_sub(c&1))&0xedb88320);}}!c}
fn take<'a>(data:&'a[u8],at:&mut usize,n:usize)->Result<&'a[u8]>{let end=at.checked_add(n).filter(|v|*v<=data.len()).ok_or("Truncated HTTP gzip member")?;let p=&data[*at..end];*at=end;Ok(p)}
pub(super) fn decode(data:&[u8],gzip:bool,limit:usize)->Result<Vec<u8>>{decode_classified(data,gzip,limit).map_err(|e|e.message)}
pub(super) fn decode_classified(data:&[u8],gzip:bool,limit:usize)->std::result::Result<Vec<u8>,DecodeError>{
 if !gzip{if data.len()>limit{return Err(output("HTTP representation exceeds decoded byte allowance"));}return Ok(data.to_vec());}
 if data.len()>2*1024*1024{return Err("HTTP gzip input exceeds 2 MiB".into());}let mut work=128*1024*1024usize;let mut at=0;let mut out=Vec::new();
 while at<data.len(){let begin=at;charge(&mut work,10)?;let h=take(data,&mut at,10)?;if h[..3]!=[31,139,8]||h[3]&0xe0!=0{return Err("Invalid or unsupported HTTP gzip header".into());}let flags=h[3];
  if flags&4!=0{let len=take(data,&mut at,2)?;let len=u16::from_le_bytes(len.try_into().unwrap())as usize;charge(&mut work,len+2)?;take(data,&mut at,len)?;}
  for mask in [8,16]{if flags&mask!=0{loop{charge(&mut work,1)?;if take(data,&mut at,1)?[0]==0{break;}}}}
  if flags&2!=0{let end=at;charge(&mut work,(end-begin).saturating_mul(8))?;let c=u16::from_le_bytes(take(data,&mut at,2)?.try_into().unwrap());if c!=(crc(&data[begin..end])&0xffff)as u16{return Err("HTTP gzip header CRC mismatch".into());}}
  let(member,used)=png_inflate::decode_raw_bounded(&data[at..],limit.saturating_sub(out.len()),&mut work).map_err(|e|DecodeError{kind:match e.kind{png_inflate::ErrorKind::Input=>ErrorKind::Response,png_inflate::ErrorKind::OutputLimit=>ErrorKind::OutputLimit,png_inflate::ErrorKind::Resource=>ErrorKind::Resource},message:e.to_string()})?;at=at.checked_add(used).ok_or("HTTP gzip offset overflow")?;charge(&mut work,member.len().saturating_mul(8).saturating_add(8))?;let trailer=take(data,&mut at,8)?;
  if u32::from_le_bytes(trailer[..4].try_into().unwrap())!=crc(&member)||u32::from_le_bytes(trailer[4..].try_into().unwrap())!=member.len()as u32{return Err("HTTP gzip CRC or size mismatch".into());}
  let wanted=out.len().checked_add(member.len()).filter(|n|*n<=limit).ok_or_else(||output("HTTP gzip decoded byte allowance exceeded"))?;
  if wanted>out.capacity(){let capacity=out.capacity().saturating_mul(2).max(64).max(wanted).min(limit);charge(&mut work,out.len().saturating_add(capacity))?;out.try_reserve_exact(capacity-out.len()).map_err(|_|resource("HTTP gzip output allocation failed"))?;}
  charge(&mut work,member.len())?;out.extend_from_slice(&member);
 }
 if at==0{return Err("Empty HTTP gzip representation".into());}Ok(out)
}
