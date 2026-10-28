//! Original local HTML + caller-selected TrueType font -> RGBA PNG pipeline.
//! This is a bounded static rendering subset, not a JavaScript browser session.
use super::{html_document,css_cascade,font_outline,page_layout,raster_surface,image_png};
use std::collections::BTreeMap;
pub(super) struct Capture {pub png:Vec<u8>,pub summary:String}
pub(super) fn render(html:&[u8],font:&[u8],width:u32,height:u32,cancelled:impl Fn()->bool)->Result<Capture,String>{
 render_with_loader(html,font,width,height,|_,_|Err("No resource loader supplied".into()),cancelled)
}
pub(super) fn render_with_loader(html:&[u8],font:&[u8],width:u32,height:u32,mut load:impl FnMut(&str,usize)->Result<Vec<u8>,String>,cancelled:impl Fn()->bool)->Result<Capture,String>{
 if html.len()>262144{return Err("HTML capture input exceeds 256 KiB".into());}
 if width==0||height==0||width>2048||height>2048||(width as usize)*(height as usize)>1024*1024{return Err("Capture viewport must fit 2048 per side and 1 Mi-pixel".into());}
 if cancelled(){return Err("Capture cancelled before rendering".into());}
 let document=html_document::parse(html,true);if !document.source_complete||document.stopped_by_bound{return Err("Capture requires a complete supported HTML source observation".into());}
 let has_base=document.resources.iter().any(|r|r.kind==html_document::ResourceKind::Base&&r.target.is_some());
 let mut resource_notes=Vec::new();let mut sheets=BTreeMap::new();let mut css_bytes=0usize;let mut sheet_count=0usize;
 for (id,node) in document.nodes.iter().enumerate(){if let html_document::Kind::Element{name,attributes,..}=&node.kind{if name!="link"{continue;}
  let attr=|key:&str|attributes.iter().find(|a|a.name==key).and_then(|a|a.value.as_deref());let rel=attr("rel").unwrap_or("");
  if !rel.split_ascii_whitespace().any(|v|v.eq_ignore_ascii_case("stylesheet")){continue;}
  if has_base{resource_notes.push(format!("Node {id} stylesheet not loaded: base URL selection is not implemented for local capture"));continue;}
  if let Some(issue)=css_cascade::stylesheet_attribute_issue(attributes){resource_notes.push(format!("Node {id} stylesheet not loaded: {issue}"));continue;}
  if rel.split_ascii_whitespace().any(|v|v.eq_ignore_ascii_case("alternate")){resource_notes.push(format!("Node {id} stylesheet not loaded: alternate stylesheet is not selected"));continue;}
  sheet_count+=1;if sheet_count>32{return Err("Capture stylesheet count exceeds 32".into());}if cancelled(){return Err("Capture cancelled while loading stylesheets".into());}
  let Some(target)=attr("href") else{resource_notes.push(format!("Node {id} stylesheet has no href"));continue;};
  let remaining=262144usize.saturating_sub(css_bytes);if remaining==0{return Err("Capture external stylesheet input exceeds 256 KiB".into());}
  match load(target,remaining){Ok(bytes)=>{if bytes.len()>remaining{return Err("Stylesheet loader exceeded its remaining byte budget".into());}css_bytes+=bytes.len();match String::from_utf8(bytes){Ok(css)=>{sheets.insert(id,css);},Err(_)=>resource_notes.push(format!("Node {id} stylesheet is not UTF-8"))}},Err(error)=>resource_notes.push(format!("Node {id} stylesheet not loaded: {error}"))}
 }}
 let styles=css_cascade::compute_with_stylesheets(&document,&sheets);if styles.incomplete{return Err("Capture CSS evaluation exceeded supported input/work bounds".into());}
 let face=font_outline::Face::parse(font).map_err(|e|e.to_string())?;
 let mut images=BTreeMap::new();let mut sizes=BTreeMap::new();let mut input_bytes=css_bytes;let mut decoded_bytes=0usize;let mut decode_work=64*1024*1024usize;
 let image_resources:Vec<_>=document.resources.iter().filter(|r|r.kind==html_document::ResourceKind::Image).collect();
 if image_resources.len()>128{return Err("Capture image count exceeds 128".into());}
 for resource in image_resources{
  if cancelled(){return Err("Capture cancelled during image loading".into());}
  if styles.styles[resource.node].display==css_cascade::Display::None{continue;}
  if has_base{resource_notes.push(format!("Node {} image not painted: base URL resolution is not implemented for local capture",resource.node));continue;}
  if document.nodes[resource.node].parent.and_then(|p|document.nodes.get(p)).map(|n|matches!(&n.kind,html_document::Kind::Element{name,..} if name=="picture")).unwrap_or(false){resource_notes.push(format!("Node {} image not painted: picture source selection is not implemented",resource.node));continue;}
  if let html_document::Kind::Element{attributes,..}=&document.nodes[resource.node].kind{if attributes.iter().any(|a|a.name=="srcset"){resource_notes.push(format!("Node {} image not painted: responsive srcset selection is not implemented",resource.node));continue;}}
  let Some(target)=resource.target.as_deref() else {resource_notes.push(format!("Node {}: image has no src",resource.node));continue;};
  let result=(||->Result<(u32,u32,Vec<u8>,String),String>{
   let remaining=(8*1024*1024usize).saturating_sub(input_bytes);if remaining==0{return Err("Capture aggregate image input budget exhausted".into());}
   let bytes=load(target,remaining)?;input_bytes=input_bytes.checked_add(bytes.len()).ok_or("Image input count overflow")?;if input_bytes>8*1024*1024{return Err("Capture aggregate image input exceeds 8 MiB".into());}
   let mut limits=image_png::Limits::default();limits.decoded_bytes=(32*1024*1024usize).saturating_sub(decoded_bytes);
   let image=image_png::decode_png_with_budget(&bytes,&limits,&mut decode_work).map_err(|e|e.to_string())?;decoded_bytes=decoded_bytes.checked_add(image.rgba.len()*2).ok_or("Image decoded count overflow")?;
   let note=format!("Node {}: PNG {}x{}, {}-bit source; {}; {}; nearest-neighbour paint with explicit 16-to-8-bit reduction",resource.node,image.width,image.height,image.source_depth,image.color_status,image.animation_status);
   Ok((image.width,image.height,image.rgba8().map_err(|e|e.to_string())?,note))
  })();
  match result{Ok((w,h,rgba,note))=>{sizes.insert(resource.node,(w,h));images.insert(resource.node,rgba);resource_notes.push(note);},Err(error)=>resource_notes.push(format!("Node {} image not painted: {}",resource.node,error))}
 }
 let layout=page_layout::layout_with_images(&document,&styles.styles,width as f64,&face,&sizes)?;
 if layout.truncated{return Err("Capture layout exceeded its work bound; no image saved".into());}
 let mut surface=raster_surface::Surface::new(width,height,[255;4])?;let mut glyphs=0usize;let mut points=0usize;let mut cache=std::collections::BTreeMap::new();
 for command in &layout.commands{if cancelled(){return Err("Capture cancelled during painting; no image saved".into());}match command{
  page_layout::Draw::Image{node,bounds}=>{let (w,h)=sizes[node];surface.image_rgba8(&images[node],w,h,bounds.x,bounds.y,bounds.width,bounds.height)?;},
  page_layout::Draw::Rect{bounds,color,..}=>surface.rectangle(bounds.x,bounds.y,bounds.width,bounds.height,*color)?,
  page_layout::Draw::Text{bounds,baseline,text,font_size,color,..}=>{let scale=*font_size/face.units_per_em as f64;let mut x=bounds.x;for ch in text.chars(){if cancelled(){return Err("Capture cancelled during text painting; no image saved".into());}glyphs+=1;if glyphs>65536{return Err("Capture glyph bound exceeded".into());}let id=face.glyph_id(ch).map_err(|e|e.to_string())?;if !cache.contains_key(&id){if cache.len()>=1024{return Err("Capture unique glyph bound exceeded".into());}let outline=face.outline(id).map_err(|e|e.to_string())?;points+=outline.contours.iter().map(Vec::len).sum::<usize>();if points>1024*1024{return Err("Capture aggregate outline point bound exceeded".into());}cache.insert(id,outline);}let outline=&cache[&id];surface.outline(&outline.contours,x+outline.origin_x*scale,*baseline,scale,*color)?;x+=outline.advance*scale;}}
 }}
 if cancelled(){return Err("Capture cancelled before encoding; no image saved".into());}
 let png=surface.png()?;
 let mut summary=format!("Native Rust static capture: {width}x{height} RGBA PNG, {} bytes; {glyphs} glyphs painted with selected TrueType hmtx metrics and unhinted quadratic outlines. Font: {} bytes, {} glyphs, {} units/em. Fixed viewport clips overflow. {}\n{}\nSelected font only; no shaping, kerning, bidi, fallback, font hinting, remote resource fetch, JavaScript or interactive browser session. Unsupported CSS/layout remains visible below; this is not a full-browser fidelity claim.",png.len(),font.len(),face.num_glyphs,face.units_per_em,styles.summary(),layout.summary());
 summary.push_str(&format!("\n{} relative stylesheets loaded ({} bytes); {} PNG images painted. Unsupported conditional or alternate stylesheets are not applied.",sheets.len(),css_bytes,images.len()));
 for issue in &styles.issues{summary.push_str("\n");summary.push_str(issue);}
 for note in resource_notes.iter().take(128){summary.push_str("\n");summary.push_str(note);}
 for i in layout.issues.iter().take(24){summary.push_str(&format!("\nNode {}: {}",i.node,i.code));}
 Ok(Capture{png,summary})
}
