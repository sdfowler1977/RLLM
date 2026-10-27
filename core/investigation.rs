//! Bounded, non-executing file and web evidence tools.
use super::*;
pub(super) fn inspect(p: &Path) -> Result<String> {
    let mut f=fs::File::open(p).map_err(|e|e.to_string())?;
    let meta=f.metadata().map_err(|e|e.to_string())?;
    if !meta.is_file(){return Err("Inspection requires a regular file".into());}
    let mut data=Vec::new();let mut block=[0u8;65536];
    while data.len()<4*1024*1024+1 {
        if super::super::execution::cancelled(){return Err("Inspection cancelled".into());}
        let remaining=(4*1024*1024+1-data.len()).min(block.len());
        let n=f.read(&mut block[..remaining]).map_err(|e|e.to_string())?;
        if n==0{break;}data.extend_from_slice(&block[..n]);
    }
    let after=f.metadata().map_err(|e|e.to_string())?;
    if meta.len()!=after.len()||meta.modified().ok()!=after.modified().ok(){return Err("File changed while being inspected; request a fresh observation".into());}
    let complete=data.len()<=4*1024*1024;if complete&&data.len() as u64!=after.len(){return Err("File length changed while being inspected".into());}data.truncate(4*1024*1024);
    let mut structure=super::exploration::analyze(&data,complete);
    if [b"RIFF".as_slice(),b"RIFX",b"RF64",b"\0asm"].iter().any(|magic|data.starts_with(magic)) {
        structure.push('\n');structure.push_str(&super::binary_observation::analyze(&data,complete,after.len()));
    }
    let format=if data.starts_with(b"\x7fELF"){"ELF"}else if data.starts_with(b"MZ"){"DOS/PE candidate"}else if data.starts_with(b"dex\n"){"DEX"}else if data.starts_with(b"PK\x03\x04"){"ZIP"}else if data.starts_with(b"\0asm"){"WebAssembly"}else if data.starts_with(b"\x89PNG\r\n\x1a\n"){"PNG"}else if data.starts_with(b"%PDF-"){"PDF"}else if std::str::from_utf8(&data).is_ok(){"UTF-8 text"}else{"Unknown binary"};
    let mut counts=[0usize;256];for b in &data{counts[*b as usize]+=1;}
    let entropy=counts.iter().filter(|&&n|n>0).map(|&n|{let p=n as f64/data.len() as f64;-p*p.log2()}).sum::<f64>();
    let mut strings=vec![];let mut start=None;
    for (i,b) in data.iter().copied().chain(std::iter::once(0)).enumerate(){
        if b.is_ascii_graphic()||b==b' ' {if start.is_none(){start=Some(i);}}
        else if let Some(begin)=start.take(){if i-begin>=5{strings.push(format!("0x{begin:08x}: {}",String::from_utf8_lossy(&data[begin..i.min(begin+256)])));if strings.len()==160{break;}}}
    }
    let elf=if format=="ELF"&&data.len()>=20{let machine=if data[5]==1{u16::from_le_bytes([data[18],data[19]])}else{u16::from_be_bytes([data[18],data[19]])};format!("\nELF class byte: {}; endian byte: {}; machine: {machine}",data[4],data[5])}else{String::new()};
    Ok(format!("Observed static inspection\nSource: {}\nFile size: {} bytes\nSampled: {} bytes\nSignature: {format}{elf}\nSample Shannon entropy: {entropy:.4} bits/byte\nFirst bytes: {}\nASCII strings (up to 160; 256 bytes each):\n{}\n\n{structure}\nSignatures are hints, not proof of validity. No code was executed or decompiled.",p.display(),meta.len(),data.len(),data.iter().take(32).map(|b|format!("{b:02x}")).collect::<Vec<_>>().join(" "),strings.join("\n")))
}
pub(super) fn validate_remote(v:&Json)->Result<bool>{
    let args=get(v,"args");match text(v,"tool").as_str(){
        "web_read"=>{let u=text(args,"url");if u.len()>2048{return Err("Web URL exceeds 2048 bytes".into());}network::url(&u,u.starts_with("http://"))?;},
        "web_search"=>{let q=text(args,"query");if q.trim().is_empty()||q.len()>2048||q.chars().any(char::is_control){return Err("Enter a search query of at most 2048 bytes".into());}},
        "rea"=>{if get(args,"discover")==&Bool(true){if args!=&obj(vec![("discover",Bool(true))]){return Err("Discovery accepts only discover:true".into());}return Ok(true);}let n=text(args,"name");if n.is_empty()||n.len()>256||!matches!(get(args,"arguments"),Obj(_))||args.string().len()>65536{return Err("REA requires a tool name and bounded JSON object arguments".into());}},
        _=>return Ok(false)
    }Ok(true)
}
pub(super) fn web_effect(st:&mut Json,pending:&Json)->Result<Json>{
    validate_remote(pending)?;
    let args=get(pending,"args");let mut headers=vec![("Accept","text/html, application/json, text/plain".into())];
    let url=if text(pending,"tool")=="web_search"{let web=get(st,"web");let template=text(web,"search_url");if !template.contains("{query}"){return Err("Configure a search URL containing {query} in App settings → Network services → Web access".into());}let key=text(web,"key");if !key.is_empty(){let header=text(web,"header");let prefix=text(web,"prefix");headers.push((if header.is_empty(){"Authorization"}else{get(web,"header").text()?},if prefix.is_empty(){key}else{format!("{prefix} {key}")}));}template.replace("{query}",&network::encoded(&text(args,"query")))}else{text(args,"url")};
    let mut effect=network::wire(&url,url.starts_with("http://"),"GET","",headers)?;put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num((2*1024*1024).to_string()));put(st,"web_source",s(&if text(pending,"tool")=="web_search"{endpoint_summary(&url)}else{url.clone()}));Ok(effect)
}
// Static HTML extraction. Script/style bodies are never returned as page text.
pub(super) fn page(body:&str)->Result<String>{
    if body.len()>2*1024*1024{return Err("Web response exceeds 2 MiB".into());}
    if !body.trim_start().starts_with('<'){return Ok(byte_limit(body,98304));}
    let mut output=String::new();let mut rest=body;let mut hidden=String::new();let mut links=Vec::new();
    while !rest.is_empty()&&output.len()<98304{
        if !hidden.is_empty(){let closing=format!("</{hidden}");let found=rest.as_bytes().windows(closing.len()).enumerate().find(|(i,w)|w.eq_ignore_ascii_case(closing.as_bytes())&&rest.as_bytes().get(i+closing.len()).map(|b|b.is_ascii_whitespace()||*b==b'>').unwrap_or(false)).map(|(i,_)|i);if let Some(i)=found{rest=&rest[i..];hidden.clear();}else{break;}}

        if let Some(tail)=rest.strip_prefix("<!--"){rest=tail.find("-->").map(|i|&tail[i+3..]).unwrap_or("");continue;}
        if rest.starts_with('<'){
            let mut quote=None;let mut end=None;
            for(i,c)in rest.char_indices().skip(1).take(8192){if let Some(q)=quote{if c==q{quote=None;}}else if c=='\''||c=='"'{quote=Some(c);}else if c=='>'{end=Some(i);break;}}
            let i=end.ok_or("Malformed or oversized HTML tag")?;let tag=&rest[1..i];rest=&rest[i+1..];let low=tag.to_ascii_lowercase();let name=low.split(|c:char|c.is_whitespace()||c=='/').find(|v|!v.is_empty()).unwrap_or("");
            if !hidden.is_empty(){if low.starts_with('/')&&name==hidden{hidden.clear();}continue;}
            if ["script","style","noscript","template"].contains(&name)&&!low.starts_with('/'){hidden=name.into();continue;}
            if name=="a"&&links.len()<100{if let Some(i)=low.find("href="){let v=&tag[i+5..];let value=if let Some(q)=v.chars().next().filter(|c|*c=='\''||*c=='"'){v[1..].split(q).next().unwrap_or("")}else{v.split_whitespace().next().unwrap_or("")};if value.len()<=2048{links.push(value.to_string());}}}
            if ["p","div","br","li","h1","h2","h3","tr"].contains(&name){output.push('\n');}
        }else{let i=rest.find('<').unwrap_or(rest.len());if hidden.is_empty(){for c in rest[..i].chars(){if output.len()+c.len_utf8()>98304{break;}output.push(c);}}rest=&rest[i..];}
    }
    for(a,b)in [("&amp;","&"),("&lt;","<"),("&gt;",">"),("&quot;","\""),("&#39;","'"),("&nbsp;"," ")]{output=output.replace(a,b);}
    Ok(byte_limit(&format!("{}\n\nPage link targets (may be relative; not fetched):\n{}\nStatic extraction only; website JavaScript was not executed.",output.trim(),links.join("\n")),120000))
}
fn byte_limit(text:&str,max:usize)->String{let mut n=text.len().min(max);while !text.is_char_boundary(n){n-=1;}text[..n].to_string()}
#[cfg(test)]mod checks{use super::*;
    #[test]fn static_page_excludes_active_content(){let p=page("<p>Hello &amp; welcome</p><script>steal()</script><style>private</style><a href='https://example.org/?a=>'>Link</a>").unwrap();assert!(p.contains("Hello & welcome"));assert!(!p.contains("steal"));assert!(!p.contains("private"));assert!(p.contains("https://example.org/?a=>"));assert!(page("<p attr='unterminated").is_err());assert!(page("<script>if(x<2){}</script><p>Visible</p>").unwrap().contains("Visible"));assert!(page(&"界".repeat(100000)).unwrap().len()<=98304);}
    #[test]fn remote_arguments_have_distinct_validation(){assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("https://example.org/path"))]))])).unwrap());assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("file:///secret"))]))])).is_err());assert!(validate_remote(&obj(vec![("tool",s("rea")),("args",obj(vec![("name",s("inspect")),("arguments",s("bad"))]))])).is_err());}
}
