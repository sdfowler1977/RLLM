//! Original bounded normal-flow geometry. Not an HTML/CSS browser renderer.
//! Approximate mode uses explicit uniform scalar advances; font mode uses held
//! TrueType hmtx/hhea metrics with validated outlines. Neither mode shapes text.
use std::collections::BTreeMap;
use super::font_outline::Face;
use super::{css_cascade::{Display, Style}, html_document::{Document, Kind, TextMode}};
const NODE_LIMIT:usize=4096;
const COMMAND_LIMIT:usize=8192;
const TEXT_LIMIT:usize=65536;
const ISSUE_LIMIT:usize=128;
const DEPTH_LIMIT:usize=128;
const COORD_LIMIT:f64=1_000_000.0;
type Result<T> = std::result::Result<T,String>;

#[derive(Clone,Copy,Debug,PartialEq)]
pub(super) struct TextMetrics { pub advance_ratio:f64 }
impl Default for TextMetrics {fn default()->Self{Self{advance_ratio:0.6}}}
#[derive(Clone,Copy,Debug,PartialEq)]
pub(super) struct Bounds {pub x:f64,pub y:f64,pub width:f64,pub height:f64}
#[derive(Clone,Debug,PartialEq)]
pub(super) enum Draw {
    Rect{node:usize,bounds:Bounds,color:[u8;4]},
    /// bounds.x is the pen origin; baseline is absolute Y. Bounds describe the
    /// advance box, not exact ink. Actual font ink also contributes to result bounds.
    Text{node:usize,bounds:Bounds,baseline:f64,text:String,font_size:f64,color:[u8;4]},
}
#[derive(Clone,Debug,PartialEq)]
pub(super) struct BoxGeometry {pub node:usize,pub bounds:Bounds,pub content:Bounds}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Issue {pub node:usize,pub code:&'static str}
#[derive(Clone,Debug)]
pub(super) struct LayoutResult {
    pub commands:Vec<Draw>,pub boxes:Vec<BoxGeometry>,pub issues:Vec<Issue>,
    pub omitted_issues:usize,pub truncated:bool,pub approximate_metrics:bool,
    pub source_complete:bool,pub viewport_width:f64,pub content_width:f64,pub content_height:f64,
    pub bounds:Bounds,
    pub metric_provenance:String,pub validated_glyphs:usize,pub expanded_outline_points:usize,
}
impl LayoutResult {
    pub fn summary(&self)->String {
        format!("Native normal-flow layout: {} boxes, {} display commands, viewport width {} px, observed content bounds ({}, {}) {} × {} px. Complete source: {}; truncated layout: {}; issues: {} recorded, {} omitted. Metric source: {}. No glyph rasterization occurs in this layout module. Lexical HTML tree only. No CSS margin collapsing, inline decoration or baseline alignment, float/flex/grid/table/positioned layout, replaced-image rendering, shaping, bidi, JavaScript, screenshot or full browser fidelity is established.",self.boxes.len(),self.commands.len(),self.viewport_width,self.bounds.x,self.bounds.y,self.bounds.width,self.bounds.height,self.source_complete,self.truncated,self.issues.len(),self.omitted_issues,self.metric_provenance)
    }
}
struct Flow {left:f64,right:f64,x:f64,y:f64,line_height:f64,has_text:bool,pending_space:bool}
impl Flow {
    fn new(left:f64,width:f64,y:f64)->Self{Self{left,right:left+width,x:left,y,line_height:0.0,has_text:false,pending_space:false}}
    fn flush(&mut self){if self.has_text{self.y+=self.line_height;}self.x=self.left;self.line_height=0.0;self.has_text=false;self.pending_space=false;}
}
#[derive(Clone,Copy)]struct Ink{left:f64,bottom:f64,right:f64,top:f64}
#[derive(Clone,Copy)]struct GlyphMetric{advance:f64,ink:Option<Ink>}
const CHAR_CACHE:usize=4096;
const GLYPH_CACHE:usize=1024;
const OUTLINE_POINTS:usize=1_000_000;
struct Engine<'a>{doc:&'a Document,styles:&'a [Style],metrics:TextMetrics,font:Option<&'a Face<'a>>,chars:BTreeMap<char,u16>,glyphs:BTreeMap<u16,GlyphMetric>,out:LayoutResult,text_used:usize}
fn checked(bounds:Bounds)->Result<Bounds>{
    if [bounds.x,bounds.y,bounds.width,bounds.height,bounds.x+bounds.width,bounds.y+bounds.height].iter().any(|v|!v.is_finite()||v.abs()>COORD_LIMIT)||bounds.width<0.0||bounds.height<0.0{return Err("Layout geometry exceeds supported coordinate bounds".into());}Ok(bounds)
}
fn replaced(name:&str)->bool{matches!(name,"img"|"video"|"audio"|"canvas"|"iframe"|"object"|"embed"|"input"|"select"|"textarea"|"svg"|"math")}
fn ws(c:char)->bool{matches!(c,' '| '\t'|'\n'|'\r'|'\u{c}')}
impl Engine<'_>{
    fn issue(&mut self,node:usize,code:&'static str){if self.out.issues.len()<ISSUE_LIMIT{self.out.issues.push(Issue{node,code});}else{self.out.omitted_issues+=1;}}
    fn glyph_metric(&mut self,node:usize,c:char)->Result<GlyphMetric>{
        let face=self.font.ok_or("Font metrics requested without a face")?;
        let id=if let Some(id)=self.chars.get(&c){*id}else{
            if self.chars.len()>=CHAR_CACHE{return Err("Font layout character-cache bound exceeded".into());}
            let id=face.glyph_id(c).map_err(|e|e.to_string())?;
            if id==0{self.issue(node,"missing-glyph-uses-font-notdef");}self.chars.insert(c,id);id
        };
        if let Some(metric)=self.glyphs.get(&id){return Ok(*metric);}
        if self.glyphs.len()>=GLYPH_CACHE{return Err("Font layout unique-glyph bound exceeded".into());}
        let outline=face.outline(id).map_err(|e|e.to_string())?;
        let points=outline.contours.iter().try_fold(0usize,|n,c|n.checked_add(c.len())).ok_or("Font point count overflow")?;
        if points>OUTLINE_POINTS.saturating_sub(self.out.expanded_outline_points){return Err("Font layout aggregate outline-point bound exceeded".into());}
        self.out.expanded_outline_points+=points;
        if !outline.advance.is_finite()||outline.advance<0.0{return Err("Invalid font advance metric".into());}
        let mut ink:Option<Ink>=None;
        for p in outline.contours.iter().flatten(){
            let x=p.x+outline.origin_x;
            if !x.is_finite()||!p.y.is_finite(){return Err("Invalid font outline coordinate".into());}
            if let Some(b)=&mut ink{b.left=b.left.min(x);b.bottom=b.bottom.min(p.y);b.right=b.right.max(x);b.top=b.top.max(p.y);}else{ink=Some(Ink{left:x,bottom:p.y,right:x,top:p.y});}
        }
        let metric=GlyphMetric{advance:outline.advance,ink};self.glyphs.insert(id,metric);self.out.validated_glyphs=self.glyphs.len();Ok(metric)
    }
    fn advance(&mut self,node:usize,c:char,style:&Style)->Result<f64>{
        if let Some(face)=self.font{Ok(self.glyph_metric(node,c)?.advance*style.font_size/f64::from(face.units_per_em))}else{Ok(style.font_size*self.metrics.advance_ratio)}
    }
    fn measure(&mut self,node:usize,text:&str,style:&Style)->Result<f64>{
        let mut width=0.0;for c in text.chars(){width+=self.advance(node,c,style)?;if !width.is_finite()||width>COORD_LIMIT{return Err("Font advance sum exceeds layout coordinate bound".into());}}Ok(width)
    }
    fn line_metrics(&self,style:&Style)->Result<(f64,f64,f64)>{
        if let Some(face)=self.font{
            let scale=style.font_size/f64::from(face.units_per_em);let ascent=f64::from(face.ascender)*scale;let em_height=f64::from(i32::from(face.ascender)-i32::from(face.descender))*scale;
            let line=if style.normal_line_height{em_height+f64::from(face.line_gap)*scale}else{style.line_height};
            if !line.is_finite()||!(0.0..=2048.0).contains(&line){return Err("Font-derived line height outside supported bounds".into());}
            Ok((line,ascent+(line-em_height)/2.0,em_height))
        }else{Ok((style.line_height,style.font_size,style.font_size))}
    }
    fn observe(&mut self,b:Bounds)->Result<()> {
        let b=checked(b)?;let old=self.out.bounds;let x=old.x.min(b.x);let y=old.y.min(b.y);let right=(old.x+old.width).max(b.x+b.width);let bottom=(old.y+old.height).max(b.y+b.height);
        self.out.bounds=checked(Bounds{x,y,width:right-x,height:bottom-y})?;Ok(())
    }
    fn emit(&mut self,command:Draw)->Result<bool>{
        if self.out.commands.len()>=COMMAND_LIMIT{self.out.truncated=true;self.issue(0,"display-command-limit");return Ok(false);}
        let b=match &command{Draw::Rect{bounds,..}|Draw::Text{bounds,..}=>*bounds};self.observe(b)?;self.out.commands.push(command);Ok(true)
    }
    fn text_fragment(&mut self,node:usize,text:String,flow:&mut Flow,style:&Style)->Result<()> {
        if text.is_empty()||self.out.truncated{return Ok(());}
        let width=self.measure(node,&text,style)?;let (line_height,baseline_offset,em_height)=self.line_metrics(style)?;let baseline=flow.y+baseline_offset;
        if !baseline.is_finite()||baseline.abs()>COORD_LIMIT{return Err("Font baseline exceeds layout bounds".into());}
        let bounds=checked(Bounds{x:flow.x,y:flow.y,width,height:em_height})?;
        if bounds.x+bounds.width>flow.right+0.00001{self.issue(node,"inline-horizontal-overflow");}
        if em_height>line_height{self.issue(node,"glyph-height-exceeds-line-height");}
        if let Some(face)=self.font{
            let scale=style.font_size/f64::from(face.units_per_em);let mut pen=flow.x;
            for c in text.chars(){let metric=self.glyph_metric(node,c)?;if let Some(ink)=metric.ink{let ink_bounds=Bounds{x:pen+ink.left*scale,y:baseline-ink.top*scale,width:(ink.right-ink.left)*scale,height:(ink.top-ink.bottom)*scale};if ink_bounds.x<flow.left||ink_bounds.x+ink_bounds.width>flow.right{self.issue(node,"font-ink-horizontal-overflow");}if ink_bounds.y<flow.y||ink_bounds.y+ink_bounds.height>flow.y+line_height{self.issue(node,"font-ink-outside-line-box");}self.observe(ink_bounds)?;}pen+=metric.advance*scale;}
        }
        if self.emit(Draw::Text{node,bounds,baseline,text,font_size:style.font_size,color:style.color})?{
            flow.x+=width;flow.line_height=flow.line_height.max(line_height);flow.has_text=true;
        }Ok(())
    }
    fn word(&mut self,node:usize,word:&str,flow:&mut Flow,style:&Style)->Result<()> {
        let width=self.measure(node,word,style)?;
        let space=if flow.has_text&&flow.pending_space{self.advance(node,' ',style)?}else{0.0};
        if flow.has_text&&flow.x+space+width>flow.right{flow.flush();}
        if flow.has_text&&flow.pending_space{self.text_fragment(node," ".into(),flow,style)?;}
        flow.pending_space=false;
        // Emergency breaks are scalar boundaries, explicitly not word shaping,
        // hyphenation or grapheme-aware line breaking.
        let mut chars=word.chars().peekable();
        while chars.peek().is_some()&&!self.out.truncated {
            let mut part=String::new();let mut used=0.0;
            while let Some(&c)=chars.peek(){let advance=self.advance(node,c,style)?;if !part.is_empty()&&used+advance>flow.right-flow.x+1e-9{break;}part.push(c);chars.next();used+=advance;}
            self.text_fragment(node,part,flow,style)?;
            if chars.peek().is_some(){self.issue(node,"emergency-scalar-word-wrap");flow.flush();}
        }Ok(())
    }
    fn text(&mut self,node:usize,text:&str,flow:&mut Flow,style:&Style)->Result<()> {
        if style.font_size==0.0 {
            let count=text.chars().take(TEXT_LIMIT.saturating_sub(self.text_used)+1).count();
            if count>TEXT_LIMIT.saturating_sub(self.text_used){self.out.truncated=true;self.issue(node,"text-scalar-limit");return Ok(());}
            self.text_used+=count;
            if !text.is_empty(){flow.has_text=true;flow.line_height=flow.line_height.max(self.line_metrics(style)?.0);flow.pending_space=text.chars().last().map(ws).unwrap_or(false);}
            self.issue(node,"zero-font-text-not-painted");return Ok(());
        }
        let mut word=String::new();let mut text_limit_reached=false;
        for c in text.chars(){
            if self.out.truncated{break;}
            if self.text_used>=TEXT_LIMIT{text_limit_reached=true;break;}self.text_used+=1;
            if ws(c){if !word.is_empty(){self.word(node,&word,flow,style)?;word.clear();}flow.pending_space=true;}
            else{word.push(c);}
        }
        if !word.is_empty()&&!self.out.truncated{self.word(node,&word,flow,style)?;}if text_limit_reached{self.out.truncated=true;self.issue(node,"text-scalar-limit");}Ok(())
    }
    fn block(&mut self,node:usize,flow:&mut Flow,depth:usize)->Result<()> {
        flow.flush();let style=self.styles[node].clone();let [mt,mr,mb,ml]=style.margin;let [pt,pr,pb,pl]=style.padding;
        let width=style.width.unwrap_or((flow.right-flow.left-ml-mr-pl-pr).max(0.0));
        let x=flow.left+ml;let y=flow.y+mt;let content_x=x+pl;let content_y=y+pt;
        let top=checked(Bounds{x,y,width:width+pl+pr,height:0.0})?;
        if top.x<flow.left||top.x+top.width+mr>flow.right+0.00001{self.issue(node,"block-horizontal-overflow");}
        let rectangle=if style.background[3]!=0{
            let index=self.out.commands.len();if self.emit(Draw::Rect{node,bounds:top,color:style.background})?{Some(index)}else{None}
        }else{None};
        let mut inner=Flow::new(content_x,width,content_y);
        let doc=self.doc;let omit_children=matches!(&doc.nodes[node].kind,Kind::Element{name,..} if replaced(name));
        if !omit_children{for &child in &doc.nodes[node].children {if self.out.truncated{break;}self.node(child,&mut inner,depth+1)?;}}
        inner.flush();let natural=(inner.y-content_y).max(0.0);let height=style.height.unwrap_or(natural);
        if natural>height+0.00001{self.issue(node,"explicit-height-content-overflow-visible");}
        let bounds=checked(Bounds{x,y,width:width+pl+pr,height:height+pt+pb})?;
        let content=checked(Bounds{x:content_x,y:content_y,width,height})?;
        if let Some(index)=rectangle{self.out.commands[index]=Draw::Rect{node,bounds,color:style.background};}
        self.observe(bounds)?;self.out.boxes.push(BoxGeometry{node,bounds,content});
        flow.y=y+bounds.height+mb;flow.x=flow.left;checked(Bounds{x:flow.x,y:flow.y,width:0.0,height:0.0})?;Ok(())
    }
    fn node(&mut self,node:usize,flow:&mut Flow,depth:usize)->Result<()> {
        if self.out.truncated{return Ok(());}if depth>DEPTH_LIMIT{return Err("Layout tree exceeds supported depth".into());}
        let style=self.styles[node].clone();if style.display==Display::None{return Ok(());}
        let doc=self.doc;match &doc.nodes[node].kind {
            Kind::Text{mode,data}=>{
                if matches!(mode,TextMode::Script|TextMode::Style){return Ok(());}
                self.text(node,data,flow,&style)
            },
            Kind::Element{name,..}=>{
                // These are not visual text containers in this foundation.
                if matches!(name.as_str(),"script"|"style"|"head"|"title"|"template"|"meta"|"link"|"base"){return Ok(());}
                if name=="br"{let had_text=flow.has_text;flow.flush();if !had_text{flow.y+=self.line_metrics(&style)?.0;}return Ok(());}
                if replaced(name)||name=="button"{self.issue(node,"replaced-media-or-control-surface-unimplemented");}
                if replaced(name)&&style.display!=Display::Block{return Ok(());}
                if style.display==Display::Block{return self.block(node,flow,depth);}
                if style.width.is_some()||style.height.is_some()||style.margin.iter().any(|x|*x!=0.0)||style.padding.iter().any(|x|*x!=0.0)||style.background[3]!=0{self.issue(node,"inline-box-size-or-decoration-unimplemented");}
                for &child in &doc.nodes[node].children{if self.out.truncated{break;}self.node(child,flow,depth+1)?;}Ok(())
            },
            Kind::Document=>{for &child in &doc.nodes[node].children{if self.out.truncated{break;}self.node(child,flow,depth+1)?;}Ok(())},
            _=>Ok(())
        }
    }
}
fn validate(doc:&Document,styles:&[Style],viewport:f64,metrics:TextMetrics)->Result<()> {
    if !viewport.is_finite()||!(1.0..=8192.0).contains(&viewport){return Err("Viewport width must be finite and between 1 and 8192 CSS pixels".into());}
    if !metrics.advance_ratio.is_finite()||!(0.1..=2.0).contains(&metrics.advance_ratio){return Err("Approximate glyph advance ratio must be finite and between 0.1 and 2".into());}
    if doc.nodes.is_empty()||doc.nodes.len()>NODE_LIMIT||styles.len()!=doc.nodes.len(){return Err("Layout requires one style per node and at most 4096 nodes".into());}
    if !matches!(doc.nodes[0].kind,Kind::Document)||doc.nodes[0].parent.is_some(){return Err("Layout requires a document root".into());}
    let mut incoming=vec![0usize;doc.nodes.len()];let mut text_bytes=0usize;
    for (index,node) in doc.nodes.iter().enumerate(){
        if let Kind::Text{data,..}=&node.kind{text_bytes=text_bytes.saturating_add(data.len());if text_bytes>256*1024{return Err("Source text exceeds layout input bound".into());}}
        for &child in &node.children {
            if child<=index||child>=doc.nodes.len()||doc.nodes[child].parent!=Some(index){return Err("Invalid or cyclic source tree relationship".into());}
            incoming[child]+=1;if incoming[child]>1{return Err("Repeated child in source tree".into());}
        }
        let style=&styles[index];
        for size in [style.width,style.height].into_iter().flatten(){if !size.is_finite()||!(0.0..=100_000.0).contains(&size){return Err("Unsupported explicit layout size".into());}}
        if style.margin.iter().any(|v|!v.is_finite()||v.abs()>10_000.0)||style.padding.iter().any(|v|!v.is_finite()||!(0.0..=10_000.0).contains(v)){return Err("Unsupported margin or padding".into());}
        if !style.font_size.is_finite()||!(0.0..=512.0).contains(&style.font_size)||!style.line_height.is_finite()||!(0.0..=2048.0).contains(&style.line_height){return Err("Unsupported font size or line height".into());}
    }
    if incoming.iter().skip(1).any(|n|*n!=1){return Err("Source tree has an unreachable node".into());}Ok(())
}
/// Normal block flow with collapsible whitespace and scalar-based emergency
/// wrapping. Styles use content-box sizes; margins are added, never collapsed.
pub(super) fn layout(document:&Document,styles:&[Style],viewport_width:f64,metrics:TextMetrics)->Result<LayoutResult>{
    run(document,styles,viewport_width,metrics,None)
}
/// Uses the caller-held face's unhinted hmtx advances and hhea line metrics.
/// No kerning, shaping, font fallback, variation or glyph substitution occurs.
pub(super) fn layout_with_font(document:&Document,styles:&[Style],viewport_width:f64,face:&Face<'_>)->Result<LayoutResult>{
    if !(16..=16384).contains(&face.units_per_em)||face.ascender<=face.descender{return Err("Unsupported font vertical metrics".into());}
    run(document,styles,viewport_width,TextMetrics::default(),Some(face))
}
fn run<'a>(document:&'a Document,styles:&'a [Style],viewport_width:f64,metrics:TextMetrics,font:Option<&'a Face<'a>>)->Result<LayoutResult>{
    validate(document,styles,viewport_width,metrics)?;
    let mut engine=Engine{doc:document,styles,metrics,font,chars:BTreeMap::new(),glyphs:BTreeMap::new(),text_used:0,out:LayoutResult{commands:Vec::new(),boxes:Vec::new(),issues:Vec::new(),omitted_issues:0,truncated:false,approximate_metrics:font.is_none(),source_complete:document.source_complete&&!document.stopped_by_bound&&document.parsed_bytes==document.examined_bytes,viewport_width,content_width:viewport_width,content_height:0.0,bounds:Bounds{x:0.0,y:0.0,width:viewport_width,height:0.0},metric_provenance:if font.is_some(){"caller-held TrueType hmtx advances/hhea baselines; validated unhinted outlines; no kerning or shaping".into()}else{format!("approximate uniform scalar advance ratio {}; not measured fonts",metrics.advance_ratio)},validated_glyphs:0,expanded_outline_points:0}};
    if !engine.out.source_complete{engine.issue(0,"partial-source-layout-only");}
    if !document.issues.is_empty()||document.omitted_issues>0{engine.issue(0,"source-parser-issues-present");}
    let mut flow=Flow::new(0.0,viewport_width,0.0);engine.node(0,&mut flow,0)?;flow.flush();engine.observe(Bounds{x:0.0,y:flow.y,width:0.0,height:0.0})?;
    engine.out.content_width=engine.out.bounds.width;engine.out.content_height=engine.out.bounds.height;Ok(engine.out)
}

#[cfg(test)]mod checks {
    use super::*;
    fn basic()->Style{Style{display:Display::Inline,width:None,height:None,margin:[0.0;4],padding:[0.0;4],font_size:10.0,line_height:12.0,color:[1,2,3,255],background:[0;4],..Style::default()}}
    fn setup(source:&str)->(Document,Vec<Style>){let d=super::super::html_document::parse(source.as_bytes(),true);let s=d.nodes.iter().map(|n|{let mut s=basic();if matches!(&n.kind,Kind::Element{name,..} if matches!(name.as_str(),"div"|"p")){s.display=Display::Block;}s}).collect();(d,s)}
    #[test]fn block_box_model_and_background_precede_text(){
        let(d,mut s)=setup("<div>Hello</div><p>Next</p>");s[1].padding=[2.0,3.0,4.0,5.0];s[1].margin=[7.0,0.0,9.0,1.0];s[1].background=[4,5,6,255];
        let r=layout(&d,&s,100.0,TextMetrics{advance_ratio:0.5}).unwrap();let b=r.boxes.iter().find(|b|b.node==1).unwrap();assert_eq!(b.bounds,Bounds{x:1.0,y:7.0,width:99.0,height:18.0});assert_eq!(b.content.x,6.0);assert_eq!(b.content.y,9.0);assert_eq!(r.boxes[1].bounds.y,34.0);assert!(matches!(r.commands[0],Draw::Rect{..}));assert!(matches!(&r.commands[1],Draw::Text{bounds,text,..} if bounds.x==6.0&&bounds.y==9.0&&text=="Hello"));
    }
    #[test]fn inline_whitespace_wrap_unicode_and_breaks(){
        let(d,s)=setup("<div>one <span>two</span> 三四<br>five</div>");let r=layout(&d,&s,35.0,TextMetrics{advance_ratio:0.5}).unwrap();let fragments:Vec<_>=r.commands.iter().filter_map(|c|if let Draw::Text{text,bounds,..}=c{Some((text.as_str(),bounds.y))}else{None}).collect();assert_eq!(fragments,vec![("one",0.0),(" ",0.0),("two",0.0),("三四",12.0),("five",24.0)]);assert_eq!(r.content_height,36.0);
        let(d,s)=setup("<div>日本語abc</div>");let r=layout(&d,&s,10.0,TextMetrics{advance_ratio:0.5}).unwrap();assert!(r.issues.iter().any(|i|i.code=="emergency-scalar-word-wrap"));assert_eq!(r.content_height,36.0);
    }
    #[test]fn hidden_content_and_explicit_overflow(){
        let(d,mut s)=setup("<script>secret</script><style>secret</style><div>hidden</div><p>longword</p>");s[5].display=Display::None;let p=d.nodes.iter().position(|n|matches!(&n.kind,Kind::Element{name,..} if name=="p")).unwrap();s[p].height=Some(1.0);s[p].width=Some(100.0);
        let r=layout(&d,&s,20.0,TextMetrics::default()).unwrap();let text=r.commands.iter().filter_map(|c|if let Draw::Text{text,..}=c{Some(text.as_str())}else{None}).collect::<String>();assert_eq!(text,"longword");assert!(r.issues.iter().any(|i|i.code=="explicit-height-content-overflow-visible"));assert!(r.issues.iter().any(|i|i.code=="block-horizontal-overflow"));
    }
    #[test]fn invalid_inputs_and_tree_refuse_without_partial_geometry(){
        let(d,s)=setup("<div>x</div>");assert!(layout(&d,&s,f64::NAN,TextMetrics::default()).is_err());assert!(layout(&d,&s[..1],100.0,TextMetrics::default()).is_err());let mut invalid=s.clone();invalid[0].padding[0]=-1.0;assert!(layout(&d,&invalid,100.0,TextMetrics::default()).is_err());let mut tree=d.clone();tree.nodes[1].children.push(0);assert!(layout(&tree,&s,100.0,TextMetrics::default()).is_err());
    }
    #[test]fn source_and_work_bounds_are_explicit(){
        let(d,s)=setup(&format!("<div>{}</div>","x ".repeat(40000)));let r=layout(&d,&s,100.0,TextMetrics::default()).unwrap();assert!(r.truncated);assert!(r.commands.len()<=COMMAND_LIMIT);assert!(r.issues.len()<=ISSUE_LIMIT);assert!(r.summary().len()<8192);
        let mut d=super::super::html_document::parse(b"<div>x",false);let s=vec![basic();d.nodes.len()];let r=layout(&d,&s,100.0,TextMetrics::default()).unwrap();assert!(!r.source_complete);d.nodes[1].parent=Some(2);assert!(layout(&d,&s,100.0,TextMetrics::default()).is_err());
    }
    #[test]fn zero_font_and_zero_line_height_do_not_divide_or_paint(){
        let(d,mut s)=setup("<div>invisible words here</div><p>visible</p>");s[1].font_size=0.0;s[1].line_height=0.0;s[2].font_size=0.0;s[2].line_height=0.0;
        let r=layout(&d,&s,35.0,TextMetrics::default()).unwrap();assert!(!r.commands.iter().any(|c|matches!(c,Draw::Text{node:2,..})));assert_eq!(r.boxes.iter().find(|b|b.node==1).unwrap().bounds.height,0.0);assert!(!r.truncated);
        s[2].font_size=10.0;let r=layout(&d,&s,35.0,TextMetrics::default()).unwrap();assert!(r.issues.iter().any(|i|i.code=="glyph-height-exceeds-line-height"));
    }

    #[test]fn replaced_raw_contents_are_not_painted_and_coordinate_overflow_refuses(){
        let(d,s)=setup("<iframe>hidden raw <code></iframe><textarea>private draft</textarea><svg><text>shape</text></svg>");
        let r=layout(&d,&s,100.0,TextMetrics::default()).unwrap();assert!(r.commands.is_empty());assert!(r.issues.iter().any(|i|i.code=="replaced-media-or-control-surface-unimplemented"));
        let(d,mut s)=setup(&"<div>x</div>".repeat(12));for style in &mut s{if style.display==Display::Block{style.height=Some(100_000.0);}}
        assert!(layout(&d,&s,100.0,TextMetrics::default()).is_err());
    }
    #[test]fn deterministic_geometry_remains_finite_bounded_and_node_scoped(){
        for width in [1.0,5.0,20.0,125.0,8192.0]{for count in 0..40{
            let source=format!("<div>{}<span>日本</span><br>end</div>","word ".repeat(count));let(d,mut s)=setup(&source);s[1].padding=[1.0;4];let r=layout(&d,&s,width,TextMetrics::default()).unwrap();
            for command in &r.commands{let(node,b)=match command{Draw::Rect{node,bounds,..}|Draw::Text{node,bounds,..}=>(*node,*bounds)};assert!(node<d.nodes.len());assert!(checked(b).is_ok());}
            assert!(r.commands.len()<=COMMAND_LIMIT);assert!(r.approximate_metrics);assert!(!r.truncated);
        }}
    }

    #[test]fn held_font_advances_baselines_and_cache_are_actual(){
        let bytes=super::super::font_outline::checks::fixture(true,None);let face=Face::parse(&bytes).unwrap();let(d,s)=setup("<div>AZA</div>");
        let r=layout_with_font(&d,&s,18.0,&face).unwrap();assert!(!r.approximate_metrics);assert_eq!(r.validated_glyphs,2);assert_eq!(r.expanded_outline_points,3);assert!(r.metric_provenance.contains("hmtx"));
        let commands:Vec<_>=r.commands.iter().filter_map(|c|if let Draw::Text{bounds,baseline,text,..}=c{Some((*bounds,*baseline,text.as_str()))}else{None}).collect();
        assert_eq!(commands.len(),2);assert_eq!(commands[0].2,"AZ");assert_eq!(commands[0].0.width,12.5);assert_eq!(commands[0].1,8.25);assert_eq!(commands[1].2,"A");assert_eq!(commands[1].0.width,6.5);assert_eq!(commands[1].1,18.75);assert!(r.issues.iter().any(|i|i.code=="missing-glyph-uses-font-notdef"));
        let mut explicit=s.clone();for v in &mut explicit{v.normal_line_height=false;v.line_height=20.0;}let r=layout_with_font(&d,&explicit,100.0,&face).unwrap();assert!(r.commands.iter().any(|c|matches!(c,Draw::Text{baseline,..} if *baseline==13.0)));
    }
    #[test]fn font_failures_and_hidden_zero_glyphs_never_substitute_shapes(){
        let bytes=super::super::font_outline::checks::fixture(true,Some(vec![0,0]));let face=Face::parse(&bytes).unwrap();let(d,mut s)=setup("<div>AB</div>");assert!(layout_with_font(&d,&s,100.0,&face).is_err());s[2].font_size=0.0;s[2].normal_line_height=true;let r=layout_with_font(&d,&s,100.0,&face).unwrap();assert_eq!(r.validated_glyphs,0);assert!(r.commands.is_empty());
        s[1].display=Display::None;let r=layout_with_font(&d,&s,100.0,&face).unwrap();assert!(r.commands.is_empty());
    }
    #[test]fn font_ink_overhang_and_long_repeated_glyphs_are_bounded(){
        let bytes=super::super::font_outline::checks::fixture(true,None);let face=Face::parse(&bytes).unwrap();let(d,s)=setup("<div>C</div>");let r=layout_with_font(&d,&s,8.0,&face).unwrap();assert!((r.content_width-10.2).abs()<1e-9);assert!(r.issues.iter().any(|i|i.code=="font-ink-horizontal-overflow"));
        let(d,s)=setup(&format!("<div>{}</div>","A".repeat(65537)));let r=layout_with_font(&d,&s,8192.0,&face).unwrap();assert!(r.truncated);assert_eq!(r.validated_glyphs,1);assert_eq!(r.expanded_outline_points,3);assert!(r.commands.len()<100);assert!(r.commands.iter().all(|c|matches!(c,Draw::Text{baseline,..} if baseline.is_finite())));
    }

    #[test]fn font_cache_and_aggregate_point_limits_refuse_instead_of_guessing(){
        let bytes=super::super::font_outline::checks::fixture(true,None);let face=Face::parse(&bytes).unwrap();
        let source:String=(0x1000..0x1000+4097).filter_map(char::from_u32).collect();let(d,s)=setup(&format!("<div>{source}</div>"));assert!(layout_with_font(&d,&s,8192.0,&face).unwrap_err().contains("character-cache"));
        let(d,s)=setup("<div>A</div>");let out=layout(&d,&s,100.0,TextMetrics::default()).unwrap();
        let mut engine=Engine{doc:&d,styles:&s,metrics:TextMetrics::default(),font:Some(&face),chars:BTreeMap::new(),glyphs:BTreeMap::new(),out,text_used:0};
        engine.out.expanded_outline_points=OUTLINE_POINTS-2;
        assert!(engine.glyph_metric(2,'A').err().unwrap().contains("aggregate outline-point"));assert!(engine.glyphs.is_empty());
        engine.out.expanded_outline_points=0;for id in 100..100+GLYPH_CACHE as u16{engine.glyphs.insert(id,GlyphMetric{advance:0.0,ink:None});}
        assert!(engine.glyph_metric(2,'A').err().unwrap().contains("unique-glyph"));
    }

}
