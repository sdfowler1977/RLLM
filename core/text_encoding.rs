//! Original bounded decoding for explicitly selected HTTP text representations.
//! This is a declared-encoding subset, not browser encoding detection.
#[derive(Debug)]pub(super) enum TextError{Response(String),Resource(String)}
impl TextError{pub(super) fn message(self)->String{match self{Self::Response(s)|Self::Resource(s)=>s}}}
impl From<&str> for TextError{fn from(v:&str)->Self{Self::Response(v.into())}}
const C1:[u16;32]=[0x20ac,0x0081,0x201a,0x0192,0x201e,0x2026,0x2020,0x2021,0x02c6,0x2030,0x0160,0x2039,0x0152,0x008d,0x017d,0x008f,0x0090,0x2018,0x2019,0x201c,0x201d,0x2022,0x2013,0x2014,0x02dc,0x2122,0x0161,0x203a,0x0153,0x009d,0x017e,0x0178];
fn scalar(byte:u8)->char{char::from_u32(if (0x80..=0x9f).contains(&byte){C1[(byte-0x80)as usize]as u32}else{byte as u32}).unwrap()}
fn western(label:&str)->bool{matches!(label,"windows-1252"|"cp1252"|"x-cp1252"|"iso-8859-1"|"iso8859-1"|"iso88591"|"iso_8859-1"|"iso_8859-1:1987"|"iso-ir-100"|"latin1"|"l1"|"cp819"|"ibm819"|"csisolatin1"|"ansi_x3.4-1968")}
fn owned(text:&str,limit:usize)->Result<String,TextError>{
    if text.len()>limit{return Err("Decoded page text exceeds its UTF-8 memory bound".into());}
    let mut out=String::new();out.try_reserve_exact(text.len()).map_err(|_|TextError::Resource("Decoded page text allocation failed".into()))?;out.push_str(text);Ok(out)
}
// Two passes bound expanded UTF-8 before allocation. HTML uses replacement decoding;
// other text representations retain the harness's strict evidence-decoding contract.
fn utf16(bytes:&[u8],be:bool,html:bool,limit:usize,cancelled:&impl Fn()->bool)->Result<(String,bool),TextError>{
    let walk=|emit:&mut dyn FnMut(char)->Result<(),TextError>|->Result<bool,TextError>{
        let unit=|i:usize|if be{u16::from_be_bytes([bytes[i],bytes[i+1]])}else{u16::from_le_bytes([bytes[i],bytes[i+1]])};
        let mut i=0usize;let mut repaired=false;
        while i<bytes.len(){
            if i%4096<4&&cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}
            let remaining=bytes.len()-i;
            let mut invalid=false;
            let c=if remaining<2{i=bytes.len();invalid=true;'\u{fffd}'}else{
                let first=unit(i);i+=2;
                if (0xd800..=0xdbff).contains(&first){
                    if remaining<4{i=bytes.len();invalid=true;'\u{fffd}'}else{
                        let next=unit(i);
                        if (0xdc00..=0xdfff).contains(&next){i+=2;char::from_u32(0x10000+(((first as u32)-0xd800)<<10)+(next as u32)-0xdc00).unwrap()}
                        else{invalid=true;'\u{fffd}'}
                    }
                }else if (0xdc00..=0xdfff).contains(&first){invalid=true;'\u{fffd}'}
                else{char::from_u32(first as u32).unwrap()}
            };
            if invalid{if !html{return Err("Page bytes are not valid declared UTF-16 text".into());}repaired=true;}
            emit(c)?;
        }
        if cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}Ok(repaired)
    };
    let mut size=0usize;
    let repaired=walk(&mut |c|{size=size.checked_add(c.len_utf8()).filter(|v|*v<=limit).ok_or("Decoded page text exceeds its UTF-8 memory bound")?;Ok(())})?;
    let mut out=String::new();out.try_reserve_exact(size).map_err(|_|TextError::Resource("Decoded page text allocation failed".into()))?;
    walk(&mut |c|{out.push(c);Ok(())})?;Ok((out,repaired))
}
fn utf16_result(bytes:&[u8],be:bool,html:bool,bom:bool,limit:usize,cancelled:&impl Fn()->bool)->Result<(String,&'static str),TextError>{
    let (out,repaired)=utf16(bytes,be,html,limit,cancelled)?;
    let label=match (be,bom,repaired){
        (true,true,true)=>"UTF-16BE (byte-order mark; malformed sequences replaced)",
        (false,true,true)=>"UTF-16LE (byte-order mark; malformed sequences replaced)",
        (true,false,true)=>"UTF-16BE (declared; malformed sequences replaced)",
        (false,false,true)=>"UTF-16LE (declared; malformed sequences replaced)",
        (true,true,false)=>"UTF-16BE (byte-order mark)",(false,true,false)=>"UTF-16LE (byte-order mark)",
        (true,false,false)=>"UTF-16BE (declared web encoding)",(false,false,false)=>"UTF-16LE (declared web encoding)",
    };Ok((out,label))
}
/// Decode one explicitly declared text representation, honouring cancellation
/// and classifying the failure. A website or representation fault is a Response
/// error; a current-memory or cancellation fault is a Resource error. This is
/// the only decoder. The retained `decode_classified` name is the entry the
/// page reader consumes.
pub(super) fn decode_with_cancel(bytes:&[u8],charset:Option<&str>,html:bool,legacy_allowed:bool,limit:usize,cancelled:&impl Fn()->bool)->Result<(String,&'static str),TextError>{
    if cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}
    let label=charset.map(|s|s.trim_matches(|c|matches!(c,' '| '\t'|'\r'|'\n'|'\x0c')).to_ascii_lowercase());
    // Typed HTML byte-order marks precede the HTTP label, including absent labels.
    // JSON/XML callers do not opt into this web-text encoding path.
    if html&&legacy_allowed{
        if let Some(body)=bytes.strip_prefix(b"\xff\xfe"){return utf16_result(body,false,true,true,limit,cancelled);}
        if let Some(body)=bytes.strip_prefix(b"\xfe\xff"){return utf16_result(body,true,true,true,limit,cancelled);}
        if let Some(body)=bytes.strip_prefix(b"\xef\xbb\xbf"){
            let text=std::str::from_utf8(body).map_err(|_|"HTML UTF-8 BOM conflicts with invalid UTF-8 bytes")?;
            return Ok((owned(text,limit)?,"UTF-8 (HTML BOM)"));
        }
    }
    if legacy_allowed&&matches!(label.as_deref(),Some("utf-16"|"utf-16le"|"utf-16be")){
        if let Some(body)=bytes.strip_prefix(b"\xff\xfe"){return utf16_result(body,false,html,true,limit,cancelled);}
        if let Some(body)=bytes.strip_prefix(b"\xfe\xff"){return utf16_result(body,true,html,true,limit,cancelled);}
        return utf16_result(bytes,label.as_deref()==Some("utf-16be"),html,false,limit,cancelled);
    }
    match label.as_deref(){
        Some("ascii"|"us-ascii")=>{
            if !bytes.is_ascii(){return Err("Page bytes do not match declared US-ASCII charset".into());}
            return Ok((owned(std::str::from_utf8(bytes).unwrap(),limit)?,"US-ASCII"));
        },
        None|Some("utf-8"|"utf8")=>{
            let bytes=bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
            let text=std::str::from_utf8(bytes).map_err(|_|"Page bytes are not valid UTF-8 text")?;
            return Ok((owned(text,limit)?,"UTF-8"));
        },
        Some(label) if western(label)&&legacy_allowed=>{},
        _=>return Err("Page charset is unsupported for this media type; supported text decoding is UTF-8, strict US-ASCII, UTF-16 and declared Windows-1252/Latin-1 web labels".into()),
    }
    let mut size=0usize;
    for block in bytes.chunks(4096){
        if cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}
        for &byte in block{size=size.checked_add(scalar(byte).len_utf8()).filter(|n|*n<=limit).ok_or("Decoded page text exceeds its UTF-8 memory bound")?;}
    }
    let mut out=String::new();out.try_reserve_exact(size).map_err(|_|TextError::Resource("Decoded page text allocation failed".into()))?;
    for block in bytes.chunks(4096){
        if cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}
        for &byte in block{out.push(scalar(byte));}
    }
    if cancelled(){return Err(TextError::Resource("Page decoding cancelled".into()));}
    Ok((out,"Windows-1252 (declared web encoding)"))
}
/// Retained entry name used by the page reader. The cancellation-aware and
/// fault-classified decoding work lives in `decode_with_cancel`.
pub(super) fn decode_classified(bytes:&[u8],charset:Option<&str>,html:bool,legacy_allowed:bool,limit:usize,cancelled:&impl Fn()->bool)->Result<(String,&'static str),TextError>{decode_with_cancel(bytes,charset,html,legacy_allowed,limit,cancelled)}
