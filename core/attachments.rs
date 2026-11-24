//! Original bounded image transport validation. Framing/dimensions are checked;
//! this is not a pixel decoder or a claim that the selected model supports vision.
use super::*;
pub const MAX_IMAGE_BYTES:usize=2*1024*1024;
pub const MAX_TOTAL_BYTES:usize=2*1024*1024;
pub const MAX_IMAGES:usize=4;
fn dimensions(w:u32,h:u32)->Result<(u32,u32)>{
    if w==0||h==0||w>16384||h>16384||u64::from(w)*u64::from(h)>32_000_000{return Err("Image dimensions exceed 16384 per side or 32 million pixels".into());}Ok((w,h))
}
fn be32(b:&[u8])->u32{u32::from_be_bytes([b[0],b[1],b[2],b[3]])}
fn crc(bytes:&[u8])->u32{let mut v=!0u32;for b in bytes{v^=u32::from(*b);for _ in 0..8{v=(v>>1)^(0xedb88320u32.wrapping_mul(v&1));}}!v}
fn png(b:&[u8])->Result<(u32,u32)>{
    if b.len()<33||&b[..8]!=b"\x89PNG\r\n\x1a\n"{return Err("Invalid PNG signature".into());}
    let mut p=8;let mut count=0;let mut dims=None;let mut data=false;
    while p<b.len(){
        count+=1;if count>4096||b.len()-p<12{return Err("Invalid PNG chunk framing".into());}
        let n=be32(&b[p..p+4]) as usize;
        let end=p.checked_add(12).and_then(|v|v.checked_add(n)).ok_or("PNG length overflow")?;
        if end>b.len(){return Err("Truncated PNG chunk".into());}
        let typ=&b[p+4..p+8];let payload=&b[p+8..end-4];
        if crc(&b[p+4..end-4])!=be32(&b[end-4..end]){return Err("PNG checksum mismatch".into());}
        if count==1{
            if typ!=b"IHDR"||n!=13{return Err("PNG must start with IHDR".into());}
            dims=Some(dimensions(be32(payload),be32(&payload[4..]))?);
            let depth=payload[8];let color=payload[9];
            let valid=match color{0=>[1,2,4,8,16].contains(&depth),2|4|6=>[8,16].contains(&depth),3=>[1,2,4,8].contains(&depth),_=>false};
            if !valid||payload[10]!=0||payload[11]!=0||payload[12]>1{return Err("Unsupported PNG header".into());}
        }else if typ==b"IHDR"{return Err("Duplicate PNG header".into());}
        if typ==b"IDAT"{data=true;}
        if typ==b"IEND"{if n!=0||!data||end!=b.len(){return Err("Invalid PNG end".into());}return dims.ok_or("Missing PNG dimensions".into());}
        p=end;
    }Err("PNG is missing IEND".into())
}
fn jpeg(b:&[u8])->Result<(u32,u32)>{
    if b.len()<4||b[..2]!=[255,216]{return Err("Invalid JPEG signature".into());}
    let mut p=2;let mut dims=None;let mut scan=false;let mut entropy=false;let mut count=0;
    while p<b.len(){
        if entropy{while p<b.len()&&b[p]!=255{p+=1;}if p==b.len(){break;}}
        if b[p]!=255{return Err("Invalid JPEG marker".into());}while p<b.len()&&b[p]==255{p+=1;}if p==b.len(){break;}
        let marker=b[p];p+=1;
        if entropy&&(marker==0||(0xd0..=0xd7).contains(&marker)){continue;}
        entropy=false;count+=1;if count>65536{return Err("Too many JPEG segments".into());}
        if marker==0xd9{if !scan||p!=b.len(){return Err("Invalid JPEG end".into());}return dims.ok_or("Missing JPEG dimensions".into());}
        if marker==0||marker==0xd8||(0xd0..=0xd7).contains(&marker){return Err("Unexpected JPEG marker".into());}
        if marker==1{continue;}
        if b.len()-p<2{return Err("Truncated JPEG length".into());}let n=u16::from_be_bytes([b[p],b[p+1]])as usize;
        if n<2||n>b.len()-p{return Err("Truncated JPEG segment".into());}
        if [0xc0,0xc1,0xc2].contains(&marker){
            if n<8||b[p+2]!=8{return Err("Unsupported JPEG frame".into());}
            let components=b[p+7] as usize;if ![1,3,4].contains(&components)||n!=8+3*components{return Err("Invalid JPEG component frame".into());}
            if dims.is_some(){return Err("Multiple JPEG frames unsupported".into());}
            dims=Some(dimensions(u16::from_be_bytes([b[p+5],b[p+6]])as u32,u16::from_be_bytes([b[p+3],b[p+4]])as u32)?);
        }else if (0xc0..=0xcf).contains(&marker)&&![0xc4,0xc8,0xcc].contains(&marker){return Err("Unsupported JPEG encoding".into());}
        if marker==0xda{if dims.is_none()||n<6||n!=6+2*b[p+2]as usize{return Err("Invalid JPEG scan".into());}scan=true;entropy=true;}
        p+=n;
    }Err("JPEG is missing complete end marker".into())
}
pub fn image(input:&Json)->Result<Json>{
    let name=field(input,"name")?;if name.is_empty()||name.len()>256||name.chars().any(char::is_control){return Err("Image name must be 1–256 bytes without control characters".into());}
    if let Ok(kind)=input.get("kind"){if kind!=&s("image"){return Err("Unsupported attachment kind".into());}}
    let mime=field(input,"mime")?;if !["image/png","image/jpeg"].contains(&mime){return Err("Only PNG and JPEG image attachments are supported".into());}
    let declared=match input.get("bytes")?{Num(v)=>v.parse::<usize>().map_err(|_|"Invalid attachment byte count")?,_=>return Err("Attachment byte count must be an integer".into())};
    let data=field(input,"data")?;
    if declared==0||declared>MAX_IMAGE_BYTES||data.len()>((MAX_IMAGE_BYTES+2)/3)*4{return Err("Image exceeds 2 MiB; choose a smaller image".into());}
    let raw=network::unbase64(data)?;
    if raw.len()!=declared||network::b64(&raw)!=data{return Err("Image size or canonical base64 does not match".into());}
    let (w,h)=if mime=="image/png"{png(&raw)?}else{jpeg(&raw)?};
    Ok(obj(vec![("kind",s("image")),("name",s(name)),("mime",s(mime)),("data",s(data)),("bytes",Num(declared.to_string())),("width",Num(w.to_string())),("height",Num(h.to_string()))]))
}
pub fn images(message:&Json)->Result<Vec<Json>>{
    let list=match message.get("attachments"){Ok(v)=>v.array()?,Err(_)=>return Ok(Vec::new())};
    if list.len()>MAX_IMAGES{return Err("At most four images may be attached".into());}
    if !list.is_empty()&&field(message,"role")?!="user"{return Err("Images may only be attached to user messages".into());}
    let mut total=0;let mut out=Vec::new();for value in list{let valid=image(value)?;total+=field_number(&valid,"bytes")?;if total>MAX_TOTAL_BYTES{return Err("Attached images exceed 2 MiB total".into());}out.push(valid);}Ok(out)
}
fn field_number(v:&Json,key:&str)->Result<usize>{match v.get(key)?{Num(n)=>n.parse::<usize>().map_err(|_|"Invalid attachment size".into()),_=>Err("Invalid attachment size".into())}}
/// Base64 transport bytes, not tokenizer counts or image-token estimates.
pub fn encoded_bytes(messages:&[Json])->Result<usize>{
    let mut bytes=0usize;let mut decoded=0usize;let mut count=0;
    for message in messages{for value in images(message)?{count+=1;decoded+=field_number(&value,"bytes")?;bytes=bytes.checked_add(field(&value,"data")?.len()).ok_or("Attachment size overflow")?;}}
    if count>MAX_IMAGES||decoded>MAX_TOTAL_BYTES{return Err("Conversation images exceed four images or 2 MiB; start a new chat or remove images".into());}Ok(bytes)
}
/// Safe UI metadata: never includes the encoded payload.
pub fn metadata(value:&Json)->Result<Json>{let mut value=image(value)?;if let Obj(fields)=&mut value{fields.remove("data");}Ok(value)}
