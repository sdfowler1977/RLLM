//! Bounded, non-executing file and web evidence tools.
use super::*;
pub(super) const NATIVE_CAPABILITIES:&str="Compiled native investigation capabilities: original Rust modules inside this Android app's client native library, called through the harness. There is no separate REA executable, binary path, process launch, or native discover handshake. The rea operation denotes only an optional configured remote MCP connection. An empty workspace means no input files are present, not that the built-in analyzers are absent. Use permitted list/read/search to locate actual inputs, inspect(path) for local observations, and compare(path,other) for byte differences; return findings to the current question or recursive task. Inspect includes JSON structure, CSV/TSV record shape, Unicode text, complete PNG pixel decoding and alpha counts, RIFF/WAVE structure, WebAssembly section envelopes, Rust lexical declarations, ELF32/64 headers/mappings/symbol and dependency-name candidates, HTML source-order structure, and CSS token structure, stylesheet declarations/selectors, supported author-style cascade and approximate normal-flow geometry. These are bounded observations with complete/prefix status. Web reading includes static text/links and HTML structure counts. The capture(path,font,output,width,height) operation renders supported local HTML and author CSS into a new workspace PNG using original Rust TrueType outline decoding, actual selected-font advances and unhinted coverage painting. A blank font selects a supported Android system face when available; a workspace TrueType file can be selected explicitly. Width/height default to 800/600. This is a fixed-viewport static subset; unsupported styles/layout are reported. It loads supported relative workspace CSS files, PNG images and inline PNG data, decodes their actual samples, and paints them with intrinsic/CSS dimensions and nearest-neighbour scaling. It does not yet fetch remote resources or execute scripts. Full upstream REA parity, complete native rendered capture with browser sessions, full CSS layout, shaping, JavaScript execution and deep decompilation remain implementation work; do not infer those capabilities from the Rust implementation language. Use this compiled description rather than earlier conversational guesses when explaining what is implemented. To demonstrate behavior, inspect an available target and use its actual returned result. If the workspace is empty, ask for the target/input location instead of inventing a binary path.";
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
    if p.extension().and_then(|v|v.to_str())==Some("rs") {
        structure.push('\n');structure.push_str(&super::rust_source_observation::analyze(&data,complete));
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        structure.push('\n');
        if complete{match super::image_png::decode_png(&data,&super::image_png::Limits::default()){
            Ok(image)=>{let transparent=image.rgba.chunks_exact(4).filter(|p|p[3]==0).count();let partial=image.rgba.chunks_exact(4).filter(|p|p[3]>0&&p[3]<65535).count();structure.push_str(&format!("Native PNG decode: {}x{}, source sample depth {} bits; {} decoded straight-alpha RGBA16 samples. Fully transparent pixels: {transparent}; partially transparent pixels: {partial}. {}; {}. No image inference or visual meaning is asserted.",image.width,image.height,image.source_depth,image.rgba.len(),image.color_status,image.animation_status));},
            Err(error)=>structure.push_str(&format!("Native PNG decoding refused: {error}")),
        }}else{structure.push_str("PNG prefix only; complete decoding was not attempted.");}
    }
    if data.starts_with(b"\x7fELF") {
        structure.push('\n');structure.push_str(&super::elf_observation::analyze(&data,complete,after.len()));
    }
    if matches!(p.extension().and_then(|v|v.to_str()),Some("html"|"htm")) {
        structure.push('\n');let document=super::html_document::parse(&data,complete);structure.push_str(&document.summary());
        structure.push('\n');structure.push_str(&page_geometry(&document));
    }
    if p.extension().and_then(|v|v.to_str())==Some("css") {
        structure.push('\n');let tokens=super::css_syntax::tokenize(&data,complete);structure.push_str(&tokens.summary());structure.push('\n');structure.push_str(&super::css_stylesheet::parse(&tokens).summary());
    }
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
/// Ordinary non-JavaScript search. Custom services remain optional; no model
/// credential or dormant custom-search credential is sent to this default.
pub(super) const DEFAULT_SEARCH_URL:&str="https://html.duckduckgo.com/html/?q={query}";
pub(super) fn search_template(st:&Json)->String{let configured=text(get(st,"web"),"search_url");if configured.trim().is_empty(){DEFAULT_SEARCH_URL.into()}else{configured.trim().into()}}
pub(super) fn web_effect(st:&mut Json,pending:&Json)->Result<Json>{
    validate_remote(pending)?;
    let args=get(pending,"args");let mut headers=vec![("Accept","text/html, application/json, text/plain".into())];
    let url=if text(pending,"tool")=="web_search"{let web=get(st,"web");let template=search_template(st);let custom=!text(web,"search_url").trim().is_empty()&&template!=DEFAULT_SEARCH_URL;if !template.contains("{query}"){return Err("Custom search URL must contain {query}".into());}let key=text(web,"key");if custom&&!key.is_empty(){let header=text(web,"header");let prefix=text(web,"prefix");headers.push((if header.is_empty(){"Authorization"}else{get(web,"header").text()?},if prefix.is_empty(){key}else{format!("{prefix} {key}")}));}template.replace("{query}",&network::encoded(&text(args,"query")))}else{text(args,"url")};
    let mut effect=network::wire(&url,url.starts_with("http://"),"GET","",headers)?;put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num((2*1024*1024).to_string()));put(st,"web_source",s(&if text(pending,"tool")=="web_search"{endpoint_summary(&url)}else{url.clone()}));Ok(effect)
}
/// A search response can be a verification page even with HTTP200. Only
/// page-level title/form signals count; quoted words in result text do not.
pub(super) fn search_page(body:&str)->Result<String>{
 if body.trim_start().starts_with('<'){
  let doc=super::html_document::parse(body.as_bytes(),true);
  let mut verification=false;
  for node in &doc.nodes{if let super::html_document::Kind::Element{name,attributes,..}=&node.kind{
   let mut parent=node.parent;let mut inert=false;let mut title_context=true;
   while let Some(i)=parent{let ancestor=&doc.nodes[i];if let super::html_document::Kind::Element{name,..}=&ancestor.kind{if ["template","svg","math","noscript"].contains(&name.as_str()){inert=true;}if !["html","head"].contains(&name.as_str()){title_context=false;}}parent=ancestor.parent;}
   if inert{continue;}
   if name=="title"&&title_context{let title=node.children.iter().filter_map(|i|doc.nodes.get(*i)).filter_map(|n|if let super::html_document::Kind::Text{data,..}=&n.kind{Some(data.as_str())}else{None}).collect::<String>();let title=title.trim().to_ascii_lowercase();if ["verify you are human","are you a human?","just a moment...","security verification"].contains(&title.as_str()){verification=true;}}
   if name=="form"{let attr=|key:&str|attributes.iter().find(|a|a.name==key).and_then(|a|a.value.as_deref()).unwrap_or("");if attr("id")=="challenge-form"&&attr("action").split('?').next().unwrap_or("").ends_with("/anomaly.js"){verification=true;}}
  }}
  if verification{return Err("Search source returned a verification page; no search results were accepted and no automatic retry was started".into());}
 }
 page(body)
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
    let document=super::html_document::parse(body.as_bytes(),true);
    let summary=format!("{}\n{}",document.summary(),page_geometry(&document));
    let observed=format!("{}\n\nPage link targets (may be relative; not fetched):\n{}\nStatic extraction only; website JavaScript was not executed.",output.trim(),links.join("\n"));
    Ok(format!("{}\n\n{}",byte_limit(&observed,120000usize.saturating_sub(summary.len()+2)),summary))
}
fn page_geometry(document:&super::html_document::Document)->String{
 let cascade=super::css_cascade::compute(document);let mut output=cascade.describe(document);
 if cascade.incomplete{return output;}
 match super::page_layout::layout(document,&cascade.styles,800.,super::page_layout::TextMetrics::default()){
  Ok(layout)=>{output.push('\n');output.push_str(&layout.summary());output.push_str("\nObservation viewport: 800 CSS px (not the phone screen). Geometry is approximate and is not a screenshot.");
   for b in layout.boxes.iter().take(16){output.push_str(&format!("\nBox node {}: x={} y={} width={} height={}",b.node,b.bounds.x,b.bounds.y,b.bounds.width,b.bounds.height));}
   if layout.boxes.len()>16{output.push_str("\nFurther box records omitted from this observation.");}
   for command in layout.commands.iter().take(24){match command{
    super::page_layout::Draw::Image{node,bounds}=>output.push_str(&format!("\nImage node {node}: ({},{}) {}x{}",bounds.x,bounds.y,bounds.width,bounds.height)),
    super::page_layout::Draw::Rect{node,bounds,color}=>output.push_str(&format!("\nPaint rectangle node {node}: ({},{}) {}x{} rgba{:?}",bounds.x,bounds.y,bounds.width,bounds.height,color)),
    super::page_layout::Draw::Text{node,bounds,text,..}=>output.push_str(&format!("\nText fragment node {node}: ({},{}) {}x{} {:?}",bounds.x,bounds.y,bounds.width,bounds.height,byte_limit(text,128))),
   }}
   if layout.commands.len()>24{output.push_str("\nFurther display commands omitted from this observation.");}
   for issue in layout.issues.iter().take(16){output.push_str(&format!("\nLayout observation node {}: {}",issue.node,issue.code));}
  },Err(error)=>{output.push_str("\nPage geometry unavailable for this input: ");output.push_str(&error);}
 }output
}

fn byte_limit(text:&str,max:usize)->String{let mut n=text.len().min(max);while !text.is_char_boundary(n){n-=1;}text[..n].to_string()}
#[cfg(test)]mod checks{use super::*;
    #[test]fn static_page_excludes_active_content(){let p=page("<p>Hello &amp; welcome</p><script>steal()</script><style>private</style><a href='https://example.org/?a=>'>Link</a>").unwrap();assert!(p.contains("Hello & welcome"));assert!(!p.contains("steal"));assert!(!p.contains("private"));assert!(p.contains("https://example.org/?a=>"));assert!(page("<p attr='unterminated").is_err());assert!(page("<script>if(x<2){}</script><p>Visible</p>").unwrap().contains("Visible"));assert!(page(&"界".repeat(100000)).unwrap().len()<=98304);}
    #[test]fn free_search_default_needs_no_url_or_credentials(){
        let mut st=obj(vec![("web",obj(vec![("key",s("dormant-custom-secret")),("header",s("X-Private"))])),("profiles",Arr(vec![obj(vec![("key",s("model-secret"))])]))]);
        let request=obj(vec![("tool",s("web_search")),("args",obj(vec![("query",s("Rust + café & 日本"))]))]);
        assert_eq!(search_template(&st),DEFAULT_SEARCH_URL);let effect=web_effect(&mut st,&request).unwrap();assert_eq!(text(&effect,"host"),"html.duckduckgo.com");let wire=text(&effect,"wire");assert!(wire.starts_with("GET /html/?q=Rust%20%2B%20caf%C3%A9%20%26%20%E6%97%A5%E6%9C%AC HTTP/1.1"));assert!(!wire.contains("secret"));assert!(!wire.contains("Authorization"));assert!(!wire.contains("X-Private"));
        put(&mut st,"web",obj(vec![("search_url",s(DEFAULT_SEARCH_URL)),("key",s("dormant-custom-secret"))]));assert!(!text(&web_effect(&mut st,&request).unwrap(),"wire").contains("secret"));
    }
    #[test]fn verification_pages_do_not_become_search_evidence(){
        assert!(search_page("<title>Verify you are human</title><p>Check</p>").is_err());assert!(search_page("<title>DuckDuckGo</title><form id='challenge-form' action='//duckduckgo.com/anomaly.js?q=x'></form>").is_err());
        assert!(search_page("<title>CAPTCHA research at DuckDuckGo</title><p>How to verify you are human: quoted article</p>").unwrap().contains("quoted article"));assert!(search_page("{\"title\":\"Verify you are human\"}").is_ok());assert!(search_page("<html><head><title>Search</title></head><body><template><title>Verify you are human</title><form id='challenge-form' action='/anomaly.js'></form></template><svg><title>Verify you are human</title></svg></body></html>").is_ok());
    }
    #[test]fn remote_arguments_have_distinct_validation(){assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("https://example.org/path"))]))])).unwrap());assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("file:///secret"))]))])).is_err());assert!(validate_remote(&obj(vec![("tool",s("rea")),("args",obj(vec![("name",s("inspect")),("arguments",s("bad"))]))])).is_err());}
}
