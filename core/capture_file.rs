//! Workspace binding for the original static capture engine.
use super::*;
use std::os::unix::fs::{OpenOptionsExt,MetadataExt};
fn read_held_using(path:&Path,limit:usize,cancelled:&impl Fn()->bool)->Result<Vec<u8>>{
 let mut file=fs::OpenOptions::new().read(true).custom_flags(0x800|0x20000).open(path).map_err(|e|format!("Cannot open capture input: {e}"))?;
 let before=file.metadata().map_err(|e|e.to_string())?;if !before.is_file()||before.len()>limit as u64{return Err("Capture input must be a regular file within its size bound".into());}
 let mut bytes=Vec::with_capacity(before.len() as usize);let mut buffer=[0;65536];loop{if cancelled(){return Err("Capture input read cancelled".into());}let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}if bytes.len()+n>limit{return Err("Capture input grew beyond its bound".into());}bytes.extend_from_slice(&buffer[..n]);}
 let after=file.metadata().map_err(|e|e.to_string())?;if before.dev()!=after.dev()||before.ino()!=after.ino()||before.len()!=after.len()||after.len()!=bytes.len() as u64||before.mtime()!=after.mtime()||before.mtime_nsec()!=after.mtime_nsec()||before.ctime()!=after.ctime()||before.ctime_nsec()!=after.ctime_nsec(){return Err("Capture input changed while reading".into());}Ok(bytes)
}
fn font_source(root:&Path,name:&str,cancelled:&impl Fn()->bool)->Result<(PathBuf,Vec<u8>)>{
 if !name.trim().is_empty(){let p=path(root,name)?;let bytes=read_held_using(&p,16*1024*1024,cancelled)?;return Ok((p,bytes));}
 #[cfg(target_os="android")]
 {for name in ["/system/fonts/Roboto-Regular.ttf","/system/fonts/NotoSans-Regular.ttf","/system/fonts/DroidSans.ttf"]{if let Ok(p)=Path::new(name).canonicalize(){if let Ok(bytes)=read_held_using(&p,16*1024*1024,cancelled){if font_outline::Face::parse(&bytes).is_ok(){return Ok((p,bytes));}}}}}
 Err("No supported platform TrueType face was available; select a workspace TrueType font for this capture".into())
}

/// Resolve local document image URLs against the document directory. Network
/// loading is a separate transport; no model-provider credentials are involved.
fn image_source(root:&Path,input:&Path,target:&str,limit:usize,cancelled:&impl Fn()->bool)->Result<Vec<u8>>{
 if target.len()>8192{return Err("Image source URL exceeds supported length".into());}
 if let Some(data)=target.strip_prefix("data:image/png;base64,"){
  if data.len()>limit.saturating_add(2)/3*4{return Err("Inline PNG exceeds remaining input budget".into());}
  let bytes=network::unbase64(data)?;if bytes.len()>limit{return Err("Inline PNG exceeds remaining input budget".into());}return Ok(bytes);
 }
 let target=target.trim_matches(|c:char|c.is_ascii_whitespace());
 if target.starts_with('/')||target.contains('\\')||target.split('/').next().unwrap_or("").contains(':'){return Err("This local capture does not yet fetch remote or absolute image URLs".into());}
 let target=target.split(['?','#']).next().unwrap_or("");if target.is_empty(){return Err("Image source path is empty".into());}
 let mut decoded=Vec::new();let mut at=0;let bytes=target.as_bytes();
 while at<bytes.len(){if bytes[at]==b'%'{let hex=|b:u8|->Option<u8>{match b{b'0'..=b'9'=>Some(b-b'0'),b'a'..=b'f'=>Some(b-b'a'+10),b'A'..=b'F'=>Some(b-b'A'+10),_=>None}};let a=bytes.get(at+1).and_then(|b|hex(*b)).ok_or("Invalid URL percent escape")?;let b=bytes.get(at+2).and_then(|b|hex(*b)).ok_or("Invalid URL percent escape")?;let c=a*16+b;if matches!(c,0|b'/'|b'\\'){return Err("Image URL contains an unsupported encoded path separator".into());}decoded.push(c);at+=3;}else{decoded.push(bytes[at]);at+=1;}}
 let decoded=String::from_utf8(decoded).map_err(|_|"Image path is not UTF-8")?;
 let directory=input.parent().ok_or("HTML source has no directory")?.strip_prefix(root).map_err(|_|"HTML source is outside workspace")?;
 let mut parts:Vec<String>=directory.components().map(|c|c.as_os_str().to_string_lossy().into_owned()).collect();
 for part in decoded.split('/') {match part{""|"."=>{},".."=>{if parts.pop().is_none(){return Err("Image path leaves the selected workspace".into());}},_=>parts.push(part.into())}}
 let resolved=path(root,&parts.join("/"))?;read_held_using(&resolved,limit,cancelled)
}
pub(super) fn validate(args:&Json)->Result<()> {
 page_program::enabled(args)?;
 if get(args,"resources")!=&Null&&!matches!(get(args,"resources"),Str(mode) if matches!(mode.as_str(),""|"inline"|"same_origin")){return Err("Capture resources must be inline or same_origin".into());}
 for key in ["font","output"]{if key=="font"&&(get(args,key)==&Null||text(args,key).trim().is_empty()){continue;}let value=get(args,key).text()?;if value.is_empty()||value.len()>1024||value.starts_with('/')||value.contains('\\')||value.contains('\0')||value.split('/').any(|p|p==".."||p.is_empty()){return Err(format!("Invalid capture {key} path"));}}
 let output=text(args,"output");if output.contains('/')||output=="."||!output.to_ascii_lowercase().ends_with(".png"){return Err("Capture output must be a new workspace-root PNG filename".into());}
 for key in ["width","height"]{if get(args,key)!=&Null {let v=get(args,key);if matches!(v,Str(s) if s.trim().is_empty()){continue;}let n=match v{Num(s)=>s.parse::<u32>().ok(),Str(s)=>s.parse::<u32>().ok(),_=>None}.ok_or("Capture dimensions must be positive integers")?;if n==0||n>2048{return Err("Capture dimensions must be 1 through 2048".into());}}}Ok(())
}
pub(super) fn capture(root:&Path,input:&Path,args:&Json,access:&operation::InspectionAccess)->Result<String>{
 validate(args)?;let name=text(args,"output");let output=root.join(&name);
 if text(args,"resources")=="same_origin"{return Err("Same-origin resource mode requires an approved remote HTML page, not a local file capture".into());}
 if output.exists(){return Err("Capture output already exists; choose a new PNG filename".into());}
 let cancelled=||access.cancelled();
 // The report response cap is separate from the capture engine working room.
 let html=read_held_using(input,262144,&cancelled)?;
 if minimum_bytes_for_len(html.len(),args)?>access.budget.saturating_sub(html.len()){return Err("Current resource budget cannot admit local capture staging".into());}
 let (html,program_note)=if page_program::enabled(args)?{let output=page_program::run(&html,access.budget.saturating_sub(html.len()),&cancelled,access.deadline.saturating_duration_since(std::time::Instant::now())).map_err(page_program::Error::message)?;(output.html.into_bytes(),output.summary)}else{(html,String::new())};
 let (font_path,font)=font_source(root,&text(args,"font"),&cancelled)?;
 let required=working_bytes(&html,&font,args,&cancelled,false)?;
 if required>access.budget{return Err("Current resource budget cannot admit local rendering and font storage".into());}
 let dimension=|key:&str,default:u32|->u32{match get(args,key){Num(s)|Str(s)=>s.parse().unwrap_or(default),_=>default}};
 let result=page_capture::render_with_loader(&html,&font,dimension("width",800),dimension("height",600),|target,limit|image_source(root,input,target,limit,&cancelled),&cancelled)?;
 let offset=save(root,&name,&result,&cancelled)?;
 Ok(format!("{}\nHTML source: {} ({} bytes). Font source: {}. Saved new workspace PNG: {name}; all {} bytes read back exactly. Existing files were not replaced.",result.summary,input.display(),html.len(),font_path.display(),offset)+&if program_note.is_empty(){String::new()}else{format!("\n{program_note}")})
}

fn save(root:&Path,name:&str,result:&page_capture::Capture,cancelled:&impl Fn()->bool)->Result<usize>{save_bytes(root,name,&result.png,cancelled)}
pub(super) fn save_bytes(root:&Path,name:&str,bytes:&[u8],cancelled:&impl Fn()->bool)->Result<usize>{
 let output=root.join(name);
 if cancelled(){return Err("Capture cancelled before output creation".into());}
 let mut file=fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(0x20000).open(&output).map_err(|e|format!("Cannot create capture output: {e}"))?;
 let offset=write_verify(&mut file,name,bytes,cancelled)?;
 let held=file.metadata().map_err(|e|e.to_string())?;let current=fs::symlink_metadata(&output).map_err(|e|e.to_string())?;if !current.is_file()||current.dev()!=held.dev()||current.ino()!=held.ino(){return Err(format!("Capture output name changed; created file {name} requires inspection"));}
 if cancelled(){return Err(format!("Capture cancelled before completion; new file {name} retained"));}
 Ok(offset)
}

pub(super) fn remote(root:&Path,html:&[u8],args:&Json,source:&str,budget:usize,cancelled:impl Fn()->bool)->Result<String>{
 remote_observed(root,html,args,source,budget,cancelled).map(|v|v.0)
}
pub(super) fn remote_resources(root:&Path,html:&[u8],args:&Json,source:&str,budget:usize,reserved:usize,summary:&str,loader:impl FnMut(usize,page_capture::ResourceKind,&str,usize)->Result<Vec<u8>>,cancelled:impl Fn()->bool)->Result<String>{
 remote_resources_observed(root,html,args,source,budget,reserved,summary,loader,cancelled).map(|v|v.0)
}
pub(super) fn remote_observed(root:&Path,html:&[u8],args:&Json,source:&str,budget:usize,cancelled:impl Fn()->bool)->Result<(String,Json)>{remote_inputs(root,html,args,source,budget,0,None,|_,_,_,_|Err("External resource was not approved".into()),cancelled)}
pub(super) fn remote_resources_observed(root:&Path,html:&[u8],args:&Json,source:&str,budget:usize,reserved:usize,summary:&str,loader:impl FnMut(usize,page_capture::ResourceKind,&str,usize)->Result<Vec<u8>>,cancelled:impl Fn()->bool)->Result<(String,Json)>{remote_inputs(root,html,args,source,budget,reserved,Some(summary),loader,cancelled)}
fn remote_inputs(root:&Path,html:&[u8],args:&Json,source:&str,budget:usize,reserved:usize,resource_summary:Option<&str>,mut loader:impl FnMut(usize,page_capture::ResourceKind,&str,usize)->Result<Vec<u8>>,cancelled:impl Fn()->bool)->Result<(String,Json)>{
 validate(args)?;if html.len()>262144{return Err("Remote HTML capture exceeds 256 KiB".into());}
 let name=text(args,"output");if root.join(&name).exists(){return Err("Capture output already exists; choose a new PNG filename".into());}
 if cancelled(){return Err("Remote capture cancelled before font loading".into());}
 let(font_path,font)=font_source(root,&text(args,"font"),&cancelled)?;
 let required=working_bytes(html,&font,args,&cancelled,true)?;if required>budget||reserved>budget.saturating_sub(required){return Err(format!("Current resource budget cannot admit remote rendering and retained resources: estimated {} bytes, available {budget}",required.saturating_add(reserved)));}
 let dimension=|key:&str,default:u32|->u32{match get(args,key){Num(s)|Str(s)=>s.parse().unwrap_or(default),_=>default}};
 let mut image_reserved=0usize;
 let mut load=|node:usize,kind:page_capture::ResourceKind,target:&str,limit:usize|{
  if let Some(data)=target.strip_prefix("data:image/png;base64,"){if data.len()>limit.saturating_add(2)/3*4{return Err("Inline PNG exceeds remaining input budget".into());}let bytes=network::unbase64(data)?;if bytes.len()>limit{return Err("Inline PNG exceeds remaining input budget".into());}let mut cost=bytes.len();if bytes.len()>=24&&bytes.starts_with(b"\x89PNG\r\n\x1a\n")&&&bytes[12..16]==b"IHDR"{let w=u32::from_be_bytes(bytes[16..20].try_into().unwrap())as usize;let h=u32::from_be_bytes(bytes[20..24].try_into().unwrap())as usize;cost=cost.checked_add(w.checked_mul(h).and_then(|v|v.checked_mul(32)).ok_or("Inline PNG memory estimate overflow")?).ok_or("Inline PNG memory estimate overflow")?;}if cost>budget.saturating_sub(required).saturating_sub(reserved).saturating_sub(image_reserved){return Err("Inline PNG exceeds the current render memory budget; image was not painted".into());}image_reserved+=cost;Ok(bytes)}
  else{loader(node,kind,target,limit)}
 };
 let result=if resource_summary.is_some(){page_capture::render_resources(html,&font,dimension("width",800),dimension("height",600),&mut load,&cancelled)}else{page_capture::render_with_loader(html,&font,dimension("width",800),dimension("height",600),|target,limit|load(usize::MAX,page_capture::ResourceKind::Png,target,limit),&cancelled)}?;
 let offset=save(root,&name,&result,&cancelled)?;
 let resources=resource_summary.unwrap_or("External CSS/PNG resources were not fetched; inline styles/data PNG only.");
 let artifact=obj(vec![("path",s(&name)),("bytes",Num(result.png.len().to_string())),("sha256",s(&harness_rules::sha256(&result.png))),("width",Num(u32::from_be_bytes(result.png[16..20].try_into().unwrap()).to_string())),("height",Num(u32::from_be_bytes(result.png[20..24].try_into().unwrap()).to_string()))]);
 Ok((format!("{}\nRemote HTML source: {source} ({} UTF-8 bytes). Font source: {}. {resources}\nSaved new workspace PNG: {name}; all {offset} bytes read back exactly. Existing files were not replaced.",result.summary,html.len(),font_path.display()),artifact))
}

pub(super) fn minimum_bytes(html:&str,args:&Json)->Result<usize>{minimum_bytes_for_len(html.len(),args)}
fn minimum_bytes_for_len(html_bytes:usize,args:&Json)->Result<usize>{
 let dim=|key:&str,default:usize|->usize{match get(args,key){Num(s)|Str(s)=>s.parse().unwrap_or(default),_=>default}};
 let pixels=dim("width",800).checked_mul(dim("height",600)).ok_or("Capture viewport overflow")?;if pixels>1024*1024{return Err("Capture viewport exceeds 1 Mi-pixel".into());}
 // Reserve font input, document/style/layout staging and native/PNG scratch;
 // the worker refines this floor with actual font outlines and inline images.
 (32*1024*1024usize).checked_add(pixels*16).and_then(|v|v.checked_add(html_bytes*64)).ok_or_else(||"Capture working-set estimate overflow".into())
}
fn working_bytes(html:&[u8],font:&[u8],args:&Json,cancelled:&impl Fn()->bool,require_utf8:bool)->Result<usize>{
 let document=html_document::parse(html,true);if !document.source_complete||document.stopped_by_bound{return Err("Capture requires complete bounded HTML".into());}
 let face=font_outline::Face::parse(font).map_err(|e|e.to_string())?;let mut glyphs=std::collections::BTreeSet::new();glyphs.insert(face.glyph_id(' ').map_err(|e|e.to_string())?);for node in &document.nodes{if let html_document::Kind::Text{data,..}=&node.kind{for c in data.chars(){glyphs.insert(face.glyph_id(c).map_err(|e|e.to_string())?);if glyphs.len()>1024{return Err("Capture admission glyph set exceeds 1024".into());}}}}
 let(mut points,mut contours)=(0usize,0usize);for id in glyphs{if cancelled(){return Err("Remote capture cancelled during font admission".into());}let outline=face.outline(id).map_err(|e|e.to_string())?;points+=outline.contours.iter().map(Vec::len).sum::<usize>();contours+=outline.contours.len();if points>1024*1024||contours>1024*1024{return Err("Capture outline admission exceeds its bound".into());}}
 if require_utf8{std::str::from_utf8(html).map_err(|_|"Capture HTML is not UTF-8")?;}
 let minimum=minimum_bytes_for_len(html.len(),args)?;
 minimum.checked_add(points.checked_mul(32).ok_or("Outline working-set overflow")?).and_then(|v|v.checked_add(contours*32)).and_then(|v|v.checked_add(font.len())).ok_or_else(||"Capture working-set estimate overflow".into())
}

/// Shared binary output machinery; the caller owns create-only opening and path binding.
pub(super) fn write_verify(file:&mut fs::File,name:&str,bytes:&[u8],cancelled:&impl Fn()->bool)->Result<usize>{
 for block in bytes.chunks(65536){if cancelled(){return Err(format!("Capture cancelled; incomplete new file {name} may remain"));}file.write_all(block).map_err(|e|format!("Capture output incomplete ({name}): {e}"))?;}
 file.sync_all().map_err(|e|format!("Capture output not verified ({name}): {e}"))?;
 let before=file.metadata().map_err(|e|e.to_string())?;
 use std::io::{Seek,SeekFrom};file.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;let mut buffer=[0;65536];let mut offset=0usize;loop{if cancelled(){return Err(format!("Capture cancelled during readback; new file {name} retained"));}let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}if offset+n>bytes.len()||bytes[offset..offset+n]!=buffer[..n]{return Err(format!("Capture output readback mismatch; new file {name} retained"));}offset+=n;}if offset!=bytes.len(){return Err(format!("Capture output readback truncated; new file {name} retained"));}
 let after=file.metadata().map_err(|e|e.to_string())?;if before.dev()!=after.dev()||before.ino()!=after.ino()||before.len()!=after.len()||after.len()!=bytes.len()as u64||before.mtime()!=after.mtime()||before.mtime_nsec()!=after.mtime_nsec()||before.ctime()!=after.ctime()||before.ctime_nsec()!=after.ctime_nsec(){return Err(format!("Capture output changed during readback; new file {name} retained"));}
 Ok(offset)
}
