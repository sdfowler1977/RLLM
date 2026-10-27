//! Original local HTML + caller-selected TrueType font -> RGBA PNG pipeline.
//! This is a bounded static rendering subset, not a JavaScript browser session.
use super::{html_document,css_cascade,font_outline,page_layout,raster_surface};
pub(super) struct Capture {pub png:Vec<u8>,pub summary:String}
pub(super) fn render(html:&[u8],font:&[u8],width:u32,height:u32,cancelled:impl Fn()->bool)->Result<Capture,String>{
 if html.len()>262144{return Err("HTML capture input exceeds 256 KiB".into());}
 if width==0||height==0||width>2048||height>2048||(width as usize)*(height as usize)>1024*1024{return Err("Capture viewport must fit 2048 per side and 1 Mi-pixel".into());}
 if cancelled(){return Err("Capture cancelled before rendering".into());}
 let document=html_document::parse(html,true);if !document.source_complete||document.stopped_by_bound{return Err("Capture requires a complete supported HTML source observation".into());}
 let styles=css_cascade::compute(&document);if styles.incomplete{return Err("Capture CSS evaluation exceeded supported input/work bounds".into());}
 let face=font_outline::Face::parse(font).map_err(|e|e.to_string())?;
 let layout=page_layout::layout_with_font(&document,&styles.styles,width as f64,&face)?;
 if layout.truncated{return Err("Capture layout exceeded its work bound; no image saved".into());}
 let mut surface=raster_surface::Surface::new(width,height,[255;4])?;let mut glyphs=0usize;let mut points=0usize;let mut cache=std::collections::BTreeMap::new();
 for command in &layout.commands{if cancelled(){return Err("Capture cancelled during painting; no image saved".into());}match command{
  page_layout::Draw::Rect{bounds,color,..}=>surface.rectangle(bounds.x,bounds.y,bounds.width,bounds.height,*color)?,
  page_layout::Draw::Text{bounds,baseline,text,font_size,color,..}=>{let scale=*font_size/face.units_per_em as f64;let mut x=bounds.x;for ch in text.chars(){if cancelled(){return Err("Capture cancelled during text painting; no image saved".into());}glyphs+=1;if glyphs>65536{return Err("Capture glyph bound exceeded".into());}let id=face.glyph_id(ch).map_err(|e|e.to_string())?;if !cache.contains_key(&id){if cache.len()>=1024{return Err("Capture unique glyph bound exceeded".into());}let outline=face.outline(id).map_err(|e|e.to_string())?;points+=outline.contours.iter().map(Vec::len).sum::<usize>();if points>1024*1024{return Err("Capture aggregate outline point bound exceeded".into());}cache.insert(id,outline);}let outline=&cache[&id];surface.outline(&outline.contours,x+outline.origin_x*scale,*baseline,scale,*color)?;x+=outline.advance*scale;}}
 }}
 if cancelled(){return Err("Capture cancelled before encoding; no image saved".into());}
 let png=surface.png()?;
 let mut summary=format!("Native Rust static capture: {width}x{height} RGBA PNG, {} bytes; {glyphs} glyphs painted with selected TrueType hmtx metrics and unhinted quadratic outlines. Font: {} bytes, {} glyphs, {} units/em. Fixed viewport clips overflow. {}\n{}\nSelected font only; no shaping, kerning, bidi, fallback, font hinting, external resource fetch, JavaScript or interactive browser session. Unsupported CSS/layout remains visible below; this is not a full-browser fidelity claim.",png.len(),font.len(),face.num_glyphs,face.units_per_em,styles.summary(),layout.summary());
 for i in layout.issues.iter().take(24){summary.push_str(&format!("\nNode {}: {}",i.node,i.code));}
 Ok(Capture{png,summary})
}
