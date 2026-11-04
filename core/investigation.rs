//! Bounded, non-executing file and web evidence tools.
use super::*;
#[path="model_artifact.rs"] mod model_artifact;
#[path="dex.rs"] mod dex;
pub(super) const NATIVE_CAPABILITIES:&str="Workspace investigation uses the shared inspection, comparison and retained-evidence paths. inspect/compare content:complete_digest streams complete selected regular files in64KiB chunks and records exact length/SHA256. File/folder observations retain source identity, captured byte ranges, typed nodes and encoded relationships. observations pages saved records and nodes. reconstruct uses current source identity to create selected file, representation or structural outputs through review and readback.\nSupported observations include JSON and CSV/TSV structure, Unicode text, decoded PNG pixels/alpha, RIFF/WAVE, WebAssembly section envelopes, Rust declarations, DEX identifiers/classes/members, GGUF and safetensors metadata/tensor layouts, ELF headers/mappings/symbols/dependencies/relocations, PE headers/sections/imports/base-relocation declarations, HTML/CSS structure and cascade, static page text and rendered PNG capture.\nAArch64 observations retain encoded branches, candidate blocks, register/SP/NZCV facts, integer/address/memory expressions and separately labelled predecessor interpretations. Coordinates identify declared source VAs or PE RVAs. PE base-relocation rows include source entry locations, raw types, page/offset RVAs, paired HIGHADJ words, known field widths and captured target mappings. Coverage, captured prefixes and retained/omitted counts accompany each observation.\nPage capture uses the selected font, dimensions and resource settings. scripts:inline runs the supported inline-program/DOM operations transactionally through the existing reviewed page path. Use the operation reference and returned fields to choose a focused next step. Keep source observations, interpretations and implementation decisions clearly labelled.";
#[path="directory_observation.rs"] pub(super) mod directory_observation;
#[path="directory_comparison.rs"] mod directory_comparison;
pub(super) fn inspect_scoped(root:&Path,p:&Path,relative:&str,access:&operation::InspectionAccess)->Result<String>{
    if !p.is_dir(){return inspect_projected(p,&||access.cancelled(),Some((root,relative,access)));}
    let request=obj(vec![("tool",s("inspect")),("args",obj(vec![("path",s(relative))]))]);
    let report=directory_report(root,relative,access,&request)?;let display=report.describe(120000.min(access.budget.saturating_sub(256*1024)));
    Ok(if access.graph{observation_graph::envelope(display,observation_graph::snapshot(&report).map(|g|vec![g]),access.budget)}else{display})
}
pub(super) fn compare_scoped(root:&Path,left:&Path,right:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{
    if !left.is_dir()&&!right.is_dir(){return comparison::compare_scoped(root,left,right,args,access);}
    if !left.is_dir()||!right.is_dir(){return Err("Select two files or two folders for comparison".into());}
    let request=obj(vec![("tool",s("compare")),("args",args.clone())]);
    let initial=harness_rules::digest(&harness_rules::worker_policy(&request)?);
    let mut half=access.clone();half.budget=access.budget/2;
    let a=directory_report(root,&text(args,"path"),&half,&request)?;
    let b=directory_report(root,&text(args,"other"),&half,&request)?;
    if harness_rules::digest(&harness_rules::worker_policy(&request)?)!=initial{return Err("Harness rules changed during folder comparison; request a fresh comparison".into());}
    let display=directory_comparison::describe(&a,&b,120000.min(access.budget/4),&||access.cancelled())?;
    Ok(if access.graph{observation_graph::envelope(display,observation_graph::snapshot(&a).and_then(|a|observation_graph::snapshot(&b).map(|b|vec![a,b])),access.budget)}else{display})
}
fn directory_report(root:&Path,relative:&str,access:&operation::InspectionAccess,request:&Json)->Result<directory_observation::Report>{
    use directory_observation::{Access,Decision,Limits};
    let policy_identity=harness_rules::digest(&harness_rules::worker_policy(&request)?);
    let available=access.budget.saturating_sub(256*1024);
    if available<1024{return Err("Directory observation exceeds the current response memory budget".into());}
    let mut limits=Limits::default();
    limits.report_bytes=limits.report_bytes.min(available);
    limits.total_sample_bytes=limits.total_sample_bytes.min(limits.report_bytes/2);
    let mut policy=|path:&Path,kind:Access|{
        let (tool,enabled)=match kind{Access::ListDirectory=>("list",access.list),Access::ReadHeader=>("read",access.read)};
        if !enabled{return Decision::Deny;}
        let Some(path)=path.to_str() else{return Decision::Deny;};
        let request=obj(vec![("tool",s(tool)),("args",obj(vec![("path",s(path))]))]);
        match harness_rules::worker_policy(&request){
            Ok(global)=>if harness_rules::worker_review_required(&global,&request){Decision::Ask}else{Decision::Allow},
            Err(_)=>Decision::Deny,
        }
    };
    let report=directory_observation::observe_bound(root,Path::new(relative),access.root_identity,&limits,&mut policy,&||access.cancelled())?;
    if harness_rules::digest(&harness_rules::worker_policy(&request)?)!=policy_identity{return Err("Harness rules changed during directory observation; request a fresh inspection".into());}
    Ok(report)
}
pub(super) fn inspect(p: &Path) -> Result<String> {inspect_using(p,&super::super::execution::cancelled)}
fn inspect_using(p:&Path,cancelled:&impl Fn()->bool)->Result<String>{inspect_projected(p,cancelled,None)}
fn inspect_projected(p:&Path,cancelled:&impl Fn()->bool,projection:Option<(&Path,&str,&operation::InspectionAccess)>)->Result<String>{
    use std::os::unix::fs::OpenOptionsExt;
    let held=projection.map(|(root,relative,access)|file_evidence::Source::open(root,relative,"inspect",&obj(vec![("path",s(relative))]),access)).transpose()?;
    let mut f=match &held{Some(source)=>source.file()?,None=>fs::OpenOptions::new().read(true).custom_flags(0x20000|0x800).open(p).map_err(|e|e.to_string())?};
    let meta=f.metadata().map_err(|e|e.to_string())?;
    if !meta.is_file(){return Err("Inspection requires a regular file".into());}
    let mut data=Vec::new();let mut block=[0u8;65536];
    while data.len()<4*1024*1024+1 {
        if cancelled(){return Err("Inspection cancelled".into());}if let Some(source)=&held{source.check()?;}
        let remaining=(4*1024*1024+1-data.len()).min(block.len());
        let n=f.read(&mut block[..remaining]).map_err(|e|e.to_string())?;
        if n==0{break;}data.extend_from_slice(&block[..n]);
    }
    if let Some(source)=&held{source.check()?;}let after=f.metadata().map_err(|e|e.to_string())?;
    if meta.len()!=after.len()||meta.modified().ok()!=after.modified().ok(){return Err("File changed while being inspected; request a fresh observation".into());}
    let complete=data.len()<=4*1024*1024;if complete&&data.len() as u64!=after.len(){return Err("File length changed while being inspected".into());}data.truncate(4*1024*1024);
    let mut structure=super::exploration::analyze(&data,complete);let mut binary=None;
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
    if let Some(observation)=dex::analyze_with_cancel(&data,complete,after.len(),cancelled) {
        if cancelled(){return Err("Inspection cancelled".into());}
        structure.push('\n');structure.push_str(&observation);
    }
    if let Some(observation)=model_artifact::analyze(&data,complete,after.len(),p.extension().and_then(|v|v.to_str())==Some("safetensors")) {
        structure.push('\n');structure.push_str(&observation);
    }
    if data.starts_with(b"MZ") {
        let observation=super::pe_observation::observe_with_cancel(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&observation.text);binary=Some(observation);
    }
    if data.starts_with(b"\x7fELF") {
        let observation=super::elf_observation::observe_with_cancel(&data,complete,after.len(),cancelled)?;structure.push('\n');structure.push_str(&observation.text);binary=Some(observation);
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
    if cancelled(){return Err("Inspection cancelled before publication".into());}if let Some(source)=&held{source.check()?;}
    let display=format!("Observed static inspection\nSource: {}\nFile size: {} bytes\nSampled: {} bytes\nSignature: {format}{elf}\nSample Shannon entropy: {entropy:.4} bits/byte\nFirst bytes: {}\nASCII strings (up to 160; 256 bytes each):\n{}\n\n{structure}\nSignatures and symbolic instruction IR describe the captured bytes. File sizes and sampled byte counts identify the observation coverage.",p.display(),meta.len(),data.len(),data.iter().take(32).map(|b|format!("{b:02x}")).collect::<Vec<_>>().join(" "),strings.join("\n"));
    if let(Some((root,relative,access)),Some(binary))=(projection,binary){if access.graph&&!binary.coordinate.is_empty(){let snapshot=binary_evidence::snapshot(root,relative,access,&after,&data,complete,&binary,&display);if cancelled(){return Err("Inspection cancelled before typed evidence publication".into());}return Ok(match snapshot{Ok((display,graph))=>observation_graph::envelope(display,Ok(vec![graph]),access.budget),Err(e)=>observation_graph::envelope(display,Err(e),access.budget)});}}
    Ok(display)
}
pub(super) fn validate_remote(v:&Json)->Result<bool>{
    let args=get(v,"args");match text(v,"tool").as_str(){
        "web_read"=>{page_program::enabled(args)?;let u=text(args,"url");if u.len()>2048{return Err("Web URL exceeds 2048 bytes".into());}network::url(&u,u.starts_with("http://"))?;let c=get(args,"capture");if c!=&Null{let Obj(fields)=c else{return Err("Page capture settings must be an object".into());};if c.string().len()>4096||fields.keys().any(|k|!["font","output","width","height","resources"].contains(&k.as_str())){return Err("Unsupported or oversized page capture settings".into());}capture_file::validate(c)?;}},
        "web_search"=>{let q=text(args,"query");if q.trim().is_empty()||q.len()>2048||q.chars().any(char::is_control){return Err("Enter a search query of at most 2048 bytes".into());}},
        "mcp"=>{if get(args,"discover")==&Bool(true){if args!=&obj(vec![("discover",Bool(true))]){return Err("Discovery accepts only discover:true".into());}return Ok(true);}let n=text(args,"name");if n.is_empty()||n.len()>256||!matches!(get(args,"arguments"),Obj(_))||args.string().len()>65536{return Err("MCP requires a tool name and bounded JSON object arguments".into());}},
        _=>return Ok(false)
    }Ok(true)
}
/// Ordinary non-JavaScript search. Custom services remain optional; no model
/// credential or dormant custom-search credential is sent to this default.
pub(super) const DEFAULT_SEARCH_URL:&str="https://api.mwmbl.org/api/v2/search/?q={query}";
pub(super) fn search_template(st:&Json)->String{let configured=text(get(st,"web"),"search_url");if configured.trim().is_empty(){DEFAULT_SEARCH_URL.into()}else{configured.trim().into()}}
pub(super) fn web_effect(st:&mut Json,pending:&Json)->Result<Json>{
    validate_remote(pending)?;
    if text(pending,"tool")=="web_search"&&search::known(st){let mut chain=search::begin(st,pending);let effect=match search::effect(&chain){Ok(e)=>e,Err(reason)=>{chain=search::advance(&chain,&reason)?;search::effect(&chain)?}};let mut effect=effect;put(st,"web_source",s(&text(&effect,"host")));put(&mut effect,"search_chain",chain);return Ok(effect);}
    let args=get(pending,"args");let mut headers=vec![("Accept","text/html, application/json, text/plain".into())];
    let url=if text(pending,"tool")=="web_search"{let web=get(st,"web");let template=search_template(st);let custom=!text(web,"search_url").trim().is_empty()&&template!="https://html.duckduckgo.com/html/?q={query}"&&template!=DEFAULT_SEARCH_URL;if !template.contains("{query}"){return Err("Custom search URL must contain {query}".into());}let key=text(web,"key");if custom&&!key.is_empty(){let header=text(web,"header");let prefix=text(web,"prefix");headers.push((if header.is_empty(){"Authorization"}else{get(web,"header").text()?},if prefix.is_empty(){key}else{format!("{prefix} {key}")}));}template.replace("{query}",&network::encoded(&text(args,"query")))}else{text(args,"url")};
    let mut effect=if text(pending,"tool")=="web_read"{network::page_wire(&url,url.starts_with("http://"),headers)?}else{network::wire(&url,url.starts_with("http://"),"GET","",headers)?};put(&mut effect,"purpose",s("web_tool"));put(&mut effect,"limit",Num((2*1024*1024).to_string()));put(st,"web_source",s(&if text(pending,"tool")=="web_search"{endpoint_summary(&url)}else{url.clone()}));Ok(effect)
}
/// A search response can be a verification page even with HTTP200. Only
/// page-level title/form signals count; quoted words in result text do not.
pub(super) fn search_page(body:&str)->Result<String>{search_challenge(body)?;page(body)}
pub(super) fn search_challenge(body:&str)->Result<()>{
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
 Ok(())
}
// Static HTML extraction. Script/style bodies are never returned as page text.
pub(super) fn page(body:&str)->Result<String>{
    if body.len()>2*1024*1024{return Err("Web response exceeds 2 MiB".into());}
    if !body.trim_start().starts_with('<'){return Ok(byte_limit(body,98304));}
    html_page(body)
}
fn html_space(b:u8)->bool{matches!(b,b' '|b'\t'|b'\n'|b'\r'|12)}
fn html_name(b:u8)->bool{b.is_ascii_alphanumeric()||matches!(b,b'-'|b':'|b'_')}
struct PageTag<'a>{name:&'a str,closing:bool,end:usize,href:Option<&'a str>,complete:bool}
// Read one tag without allocating its attributes. Quoting starts only after '=';
// quotation marks in unquoted values do not change the tokenizer's state.
fn page_tag(text:&str,start:usize)->Option<PageTag<'_>>{
 let b=text.as_bytes();let mut p=start+1;let closing=b.get(p)==Some(&b'/');if closing{p+=1;}
 if !b.get(p).is_some_and(u8::is_ascii_alphabetic){return None;}
 let n=p;while b.get(p).is_some_and(|v|html_name(*v)){p+=1;}let name=&text[n..p];let mut href=None;let mut seen_href=false;
 loop{
  while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  match b.get(p){None=>return Some(PageTag{name,closing,end:b.len(),href:None,complete:false}),Some(b'>')=>return Some(PageTag{name,closing,end:p+1,href,complete:true}),Some(b'/')=>{p+=1;continue},_=>{}}
  let a=p;while b.get(p).is_some_and(|v|!html_space(*v)&&!matches!(v,b'='|b'>'|b'/')){p+=1;}
  if a==p{p+=1;continue;}
  let attr=&text[a..p];while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  let is_href=attr.eq_ignore_ascii_case("href");let first_href=is_href&&!seen_href;if is_href{seen_href=true;}
  if b.get(p)!=Some(&b'='){continue;}p+=1;while b.get(p).is_some_and(|v|html_space(*v)){p+=1;}
  let quote=b.get(p).copied().filter(|v|matches!(v,b'\''|b'"'));if quote.is_some(){p+=1;}
  let v=p;while let Some(c)=b.get(p){if quote.map(|q|*c==q).unwrap_or_else(||html_space(*c)||*c==b'>'){break;}p+=1;}
  if quote.is_some()&&p==b.len(){return Some(PageTag{name,closing,end:b.len(),href:None,complete:false});}
  if first_href&&p-v<=2048{href=Some(&text[v..p]);}
  if quote.is_some(){p+=1;}
 }
}
fn page_end_raw(text:&str,from:usize,name:&str)->Option<usize>{
 let b=text.as_bytes();let mut p=from;
 while p<b.len(){if b[p]==b'<'&&b.get(p+1)==Some(&b'/')&&b.get(p+2..p+2+name.len()).is_some_and(|v|v.eq_ignore_ascii_case(name.as_bytes()))&&b.get(p+2+name.len()).is_some_and(|v|html_space(*v)||matches!(v,b'>'|b'/')){return Some(p);}p+=1;}None
}
fn page_entities(text:&str)->String{
 let mut out=String::with_capacity(text.len());let mut p=0;
 while p<text.len(){if text.as_bytes()[p]==b'&'{let end=text.as_bytes()[p+1..text.len().min(p+34)].iter().position(|b|*b==b';').map(|n|p+1+n);if let Some(end)=end{let value=&text[p+1..end];let decoded=match value{"amp"=>Some('&'),"lt"=>Some('<'),"gt"=>Some('>'),"quot"=>Some('"'),"apos"|"#39"=>Some('\''),"nbsp"=>Some(' '),v if v.starts_with("#x")||v.starts_with("#X")=>u32::from_str_radix(&v[2..],16).ok().and_then(char::from_u32),v if v.starts_with('#')=>v[1..].parse::<u32>().ok().and_then(char::from_u32),_=>None};if let Some(c)=decoded{out.push(if c=='\0'{'\u{fffd}'}else{c});p=end+1;continue;}}}let c=text[p..].chars().next().unwrap();out.push(c);p+=c.len_utf8();}out
}
fn page_append(output:&mut String,text:&str)->bool{let remaining=98304-output.len();output.push_str(&byte_limit(text,remaining));text.len()>remaining}
pub(super) fn html_page(body:&str)->Result<String>{html_page_with_room(body,usize::MAX)}
pub(super) fn html_page_with_room(body:&str,room:usize)->Result<String>{
    if body.len()>2*1024*1024{return Err("Web response exceeds 2 MiB".into());}
    let mut output=String::new();let mut p=0;let mut links=Vec::new();let mut hidden=Vec::<String>::new();let mut raw=None::<String>;let mut rcdata=None::<String>;let mut incomplete=false;let mut omitted_links=0usize;let mut text_abbreviated=false;
    while p<body.len()&&output.len()<98304{
        if let Some(name)=raw.take(){if let Some(end)=page_end_raw(body,p,&name){p=end;}else{incomplete=true;break;}}
        if let Some(name)=rcdata.take(){let end=page_end_raw(body,p,&name).unwrap_or(body.len());if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..end]);}p=end;if p==body.len(){break;}}
        let rest=&body[p..];
        if rest.starts_with("<!--"){if let Some(end)=rest[4..].find("-->"){p+=4+end+3;}else{incomplete=true;break;}continue;}
        if rest.starts_with("<!")||rest.starts_with("<?"){if let Some(end)=rest.find('>'){p+=end+1;}else{incomplete=true;break;}continue;}
        if rest.starts_with('<'){
            if let Some(tag)=page_tag(body,p){
                let name=tag.name;let closing=tag.closing;let start=p;p=tag.end;
                if !tag.complete{incomplete=true;break;}
                if name.eq_ignore_ascii_case("script")||name.eq_ignore_ascii_case("style"){if !closing{raw=Some(name.to_ascii_lowercase());}continue;}
                if name.eq_ignore_ascii_case("template")||name.eq_ignore_ascii_case("noscript"){
                    if closing{if hidden.last().is_some_and(|v|v.eq_ignore_ascii_case(name)){hidden.pop();}}else{if hidden.len()==64{incomplete=true;break;}hidden.push(name.to_ascii_lowercase());}continue;
                }
                if name.eq_ignore_ascii_case("title")||name.eq_ignore_ascii_case("textarea")||name.eq_ignore_ascii_case("xmp")||name.eq_ignore_ascii_case("iframe")||name.eq_ignore_ascii_case("noembed")||name.eq_ignore_ascii_case("noframes"){if !closing{rcdata=Some(name.to_ascii_lowercase());}continue;}
                if name.eq_ignore_ascii_case("plaintext")&&!closing{if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..]);}p=body.len();break;}
                if !hidden.is_empty(){continue;}
                if name.eq_ignore_ascii_case("a")&&!closing{if let Some(value)=tag.href{if links.len()<100{links.push(page_entities(value));}else{omitted_links+=1;}}}
                if ["p","div","br","li","h1","h2","h3","h4","h5","h6","tr","section","article","header","footer"].iter().any(|v|name.eq_ignore_ascii_case(v)){output.push('\n');}
                debug_assert!(p>start);continue;
            }
            // '<' followed by ordinary text is a literal data character.
            if hidden.is_empty(){output.push('<');}p+=1;
        }else{let end=body[p..].find('<').map(|n|p+n).unwrap_or(body.len());if hidden.is_empty(){text_abbreviated|=page_append(&mut output,&body[p..end]);}p=end;}
    }
    let output=page_entities(&output);let abbreviated=text_abbreviated||(p<body.len()&&!incomplete);
    // Visible text/link evidence has its own bounded buffers. Optional structure
    // and geometry must not consume more than the current remaining allowance.
    let structure_room=room.saturating_sub(body.len().saturating_mul(8).saturating_add(2*1024*1024));
    let document=super::html_document::parse_with_room(body.as_bytes(),true,structure_room);
    let summary=if document.allocation_refused(){format!("{}\nCurrent memory allowance omitted further structured-tree and geometry analysis; bounded visible text/link extraction remains available.",document.summary())}else{format!("{}\n{}",document.summary(),page_geometry(&document))};
    let notes=format!("{}{}{}",if incomplete{"\nStatic extraction retained preceding text; an unfinished markup/raw-content tail or inert nesting bound was omitted."}else{""},if abbreviated{"\nVisible text extraction abbreviated at 98304 UTF-8 bytes; later source was not extracted."}else{""},if omitted_links>0{"\nAdditional link targets omitted after 100 retained links."}else{""});
    let observed=format!("{}\n\nPage link targets (up to 100, each at most 2048 source bytes; may be relative; not fetched):\n{}\nThis extraction stage executes no scripts; a preceding selected inline-program stage, if any, is described separately.{notes}",output.trim(),links.join("\n"));
    let limit=120000usize.saturating_sub(summary.len()+2);
    let observed=if observed.len()>limit{let note="\nStatic extraction report abbreviated at 120000 UTF-8 bytes; later text/link records omitted.";format!("{}{}",byte_limit(&observed,limit.saturating_sub(note.len())),note)}else{observed};
    Ok(format!("{observed}\n\n{summary}"))
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
    super::page_layout::Draw::Text{node,bounds,text,..}=>{let mut parent=Some(*node);let mut inert=false;while let Some(i)=parent{let item=&document.nodes[i];if matches!(&item.kind,super::html_document::Kind::Element{name,..}if ["script","style","template","noscript"].contains(&name.as_str())){inert=true;break;}parent=item.parent;}if !inert{output.push_str(&format!("\nText fragment node {node}: ({},{}) {}x{} {:?}",bounds.x,bounds.y,bounds.width,bounds.height,byte_limit(text,128)));}},
   }}
   if layout.commands.len()>24{output.push_str("\nFurther display commands omitted from this observation.");}
   for issue in layout.issues.iter().take(16){output.push_str(&format!("\nLayout observation node {}: {}",issue.node,issue.code));}
  },Err(error)=>{output.push_str("\nPage geometry unavailable for this input: ");output.push_str(&error);}
 }output
}

pub(super) fn byte_limit(text:&str,max:usize)->String{let mut n=text.len().min(max);while !text.is_char_boundary(n){n-=1;}text[..n].to_string()}
#[cfg(test)]mod checks{use super::*;
    #[test]fn static_page_excludes_active_content(){let p=page("<p>Hello &amp; welcome</p><script>steal()</script><style>private</style><a href='https://example.org/?a=>'>Link</a>").unwrap();assert!(p.contains("Hello & welcome"));assert!(!p.contains("steal"));assert!(!p.contains("private"));assert!(p.contains("https://example.org/?a=>"));assert!(page("<p attr='unterminated").unwrap().contains("unfinished markup"));assert!(page("<script>if(x<2){}</script><p>Visible</p>").unwrap().contains("Visible"));assert!(page(&"界".repeat(100000)).unwrap().len()<=98304);}
    #[test]fn static_page_skips_long_attributes_and_preserves_readable_tail(){
        let source=format!("<div data-props='{}'><p>Useful tail &amp; evidence</p><a href='https://example.org/a?x=&quot;y&quot;'>Read</a></div>","nested < > ".repeat(3000));
        let observed=html_page(&source).unwrap();assert!(observed.contains("Useful tail & evidence"));assert!(observed.contains("https://example.org/a?x=\"y\""));assert!(!observed.contains("nested < >"));
        let observed=html_page("<p>Keep this</p><div data-state='unfinished >").unwrap();assert!(observed.contains("Keep this"));assert!(observed.contains("unfinished markup"));
        let observed=html_page("<p>1 < 2 &amp; 3 > 1</p><p x=unquoted'value>After</p>").unwrap();assert!(observed.contains("1 < 2 & 3 > 1"));assert!(observed.contains("After"));
    }
    #[test]fn static_page_inert_nested_and_attribute_identity(){
        let observed=html_page("<template><template>HIDDEN_ONE</template>HIDDEN_TWO<script>\"</template>\";HIDDEN_THREE</script></template><noscript>HIDDEN_FOUR</noscript><p>Visible</p><a data-href='wrong' HREF='first' href='second'>Link</a>").unwrap();assert!(observed.contains("Visible"));assert!(!observed.contains("HIDDEN_"));assert!(observed.contains("\nfirst\n"));assert!(!observed.contains("second"));assert!(!observed.contains("wrong"));
        let source=format!("<a href='{}' href='wrong'>Link</a>","x".repeat(2049));assert!(!html_page(&source).unwrap().contains("wrong"));
        assert_eq!(page_entities("&日本日本日本日本日本日本日本日本日本日本日本日本;"),"&日本日本日本日本日本日本日本日本日本日本日本日本;");
    }
    #[test]fn static_page_resource_and_partial_report_bounds(){
        assert!(html_page(&"x".repeat(2*1024*1024+1)).is_err());
        let source=format!("<p>{}</p>{}","界".repeat(40000),"<a href='https://example.org/'>Link</a>".repeat(110));let observed=html_page(&source).unwrap();assert!(observed.len()<=120000);assert!(observed.contains("abbreviated"));assert!(html_page(&format!("<p>{}","x".repeat(100000))).unwrap().contains("abbreviated"));
        let source=format!("<p>Start</p>{}",format!("<a href='{}'>Link</a>","a".repeat(2048)).repeat(100));let observed=html_page(&source).unwrap();assert!(observed.len()<=120000);assert!(observed.contains("report abbreviated"));
    }
    #[test]fn free_search_default_needs_no_url_or_credentials(){
        let mut st=obj(vec![("web",obj(vec![("key",s("dormant-custom-secret")),("header",s("X-Private"))])),("profiles",Arr(vec![obj(vec![("key",s("model-secret"))])]))]);
        let request=obj(vec![("tool",s("web_search")),("args",obj(vec![("query",s("Rust + café & 日本"))]))]);
        assert_eq!(search_template(&st),DEFAULT_SEARCH_URL);let effect=web_effect(&mut st,&request).unwrap();assert!(["api.mwmbl.org","html.duckduckgo.com"].contains(&text(&effect,"host").as_str()));let wire=text(&effect,"wire");assert!(wire.contains("q=Rust%20%2B%20caf%C3%A9%20%26%20%E6%97%A5%E6%9C%AC HTTP/1.1"));assert!(!wire.contains("secret"));assert!(!wire.contains("Authorization"));assert!(!wire.contains("X-Private"));
        put(&mut st,"web",obj(vec![("search_url",s("https://html.duckduckgo.com/html/?q={query}")),("key",s("dormant-custom-secret"))]));assert!(!text(&web_effect(&mut st,&request).unwrap(),"wire").contains("secret"));
    }
    #[test]fn verification_pages_do_not_become_search_evidence(){
        assert!(search_page("<title>Verify you are human</title><p>Check</p>").is_err());assert!(search_page("<title>DuckDuckGo</title><form id='challenge-form' action='//duckduckgo.com/anomaly.js?q=x'></form>").is_err());
        assert!(search_page("<title>CAPTCHA research at DuckDuckGo</title><p>How to verify you are human: quoted article</p>").unwrap().contains("quoted article"));assert!(search_page("{\"title\":\"Verify you are human\"}").is_ok());assert!(search_page("<html><head><title>Search</title></head><body><template><title>Verify you are human</title><form id='challenge-form' action='/anomaly.js'></form></template><svg><title>Verify you are human</title></svg></body></html>").is_ok());
    }
    #[test]fn remote_arguments_have_distinct_validation(){assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("https://example.org/path"))]))])).unwrap());assert!(validate_remote(&obj(vec![("tool",s("web_read")),("args",obj(vec![("url",s("file:///secret"))]))])).is_err());assert!(validate_remote(&obj(vec![("tool",s("mcp")),("args",obj(vec![("name",s("inspect")),("arguments",s("bad"))]))])).is_err());}
}

#[cfg(test)] mod cancellation_scope_checks {
 use super::*;
 #[test] fn old_owner_refuses_file_folder_and_comparison_after_reset() {
  let exec=&super::super::super::execution::cancellation_token;
  super::super::super::execution::reset();let token=exec();
  let root=std::env::temp_dir().join(format!("rllm-cancel-scope-{}-{}",std::process::id(),now()));fs::create_dir_all(&root).unwrap();fs::write(root.join("a"),b"one").unwrap();fs::write(root.join("b"),b"two").unwrap();
  let mut access=operation::InspectionAccess{list:true,read:true,graph:false,budget:2*1024*1024,root_identity:None,cancellation_token:token,deadline:std::time::Instant::now()+std::time::Duration::from_secs(120)};
  super::super::super::execution::cancel();super::super::super::execution::reset();
  let args=obj(vec![("path",s("a")),("other",s("b"))]);
  assert!(inspect_scoped(&root,&root.join("a"),"a",&access).unwrap_err().contains("cancelled"));
  assert!(inspect_scoped(&root,&root,"",&access).unwrap_err().contains("cancelled"));
  assert!(compare_scoped(&root,&root.join("a"),&root.join("b"),&args,&access).unwrap_err().contains("cancelled"));
  access.cancellation_token=exec();assert!(inspect_scoped(&root,&root.join("a"),"a",&access).is_ok());assert!(inspect_scoped(&root,&root,"",&access).is_ok());assert!(compare_scoped(&root,&root.join("a"),&root.join("b"),&args,&access).is_ok());
 }
}
