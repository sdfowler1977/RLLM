//! Original author-style cascade for explicitly supported CSS values.
//! The input tree is lexical HTML, not an HTML5 browser DOM. No resource fetch.
use super::{css_stylesheet as ast,css_syntax::Kind,html_document as html,css_selectors};
use std::collections::BTreeMap;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum Display {None,Block,Inline}
#[derive(Clone,Debug,PartialEq)]
pub(super) struct Style {
 pub display:Display,pub width:Option<f64>,pub height:Option<f64>,
 pub margin:[f64;4],pub padding:[f64;4],pub font_size:f64,pub line_height:f64,
 pub color:[u8;4],pub background:[u8;4],pub normal_line_height:bool,
}
impl Default for Style {fn default()->Self{Self{display:Display::Inline,width:None,height:None,margin:[0.;4],padding:[0.;4],font_size:16.,line_height:19.2,color:[0,0,0,255],background:[0,0,0,0],normal_line_height:true}}}
#[derive(Clone,Debug)]
pub(super) struct Observation {pub styles:Vec<Style>,pub applied:usize,pub unsupported:usize,pub incomplete:bool,pub issues:Vec<String>}
impl Observation {pub fn summary(&self)->String{format!("Native author CSS cascade: {} node styles, {} supported winning properties, {} unsupported rules/values; incomplete: {}. Lexical source-order tree; embedded and inline author styles only; no external CSS fetch, conditional rules, layers, browser DOM or font rasterization.",self.styles.len(),self.applied,self.unsupported,self.incomplete)}}
impl Observation {
 pub fn describe(&self,doc:&html::Document)->String {
  let mut result=self.summary();let mut shown=0;
  if self.incomplete{return result;}
  for (id,node) in doc.nodes.iter().enumerate(){if let html::Kind::Element{name,..}=&node.kind{if shown==32{result.push_str("\nFurther node styles omitted from this observation.");break;}let s=&self.styles[id];result.push_str(&format!("\nNode {id} <{name}> source {}..{}: display {:?}; width {:?}, height {:?} CSS px; margin {:?}; padding {:?}; font-size {}, line-height {} CSS px{}; color {:?}; background {:?}",node.span.start,node.span.end,s.display,s.width,s.height,s.margin,s.padding,s.font_size,s.line_height,if s.normal_line_height{" (normal approximated at 1.2em)"}else{""},s.color,s.background));shown+=1;}}
  result
 }
}

#[derive(Clone,Debug)]
enum Value {Display(Display),Length(Option<f64>),Scalar(f64),Color([u8;4]),Normal,Initial,Inherit,Unset}
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
struct Rank {important:bool,inline:bool,specificity:[u16;3],order:usize}
const PROPS:[&str;15]=["display","width","height","margin-top","margin-right","margin-bottom","margin-left","padding-top","padding-right","padding-bottom","padding-left","font-size","line-height","color","background-color"];
fn ident(c:&ast::Component)->Option<&str>{match &c.kind{ast::ComponentKind::Token(Kind::Ident(s))=>Some(s),_=>None}}
fn px(c:&ast::Component)->Option<f64>{let n=match &c.kind{ast::ComponentKind::Token(Kind::Dimension{number,unit}) if unit.eq_ignore_ascii_case("px")=>number.value,ast::ComponentKind::Token(Kind::Number(n)) if n.value==Some(0.)=>Some(0.),_=>None}?;if n.is_finite()&&n.abs()<=1_000_000.{Some(n)}else{None}}
fn color(c:&ast::Component)->Option<[u8;4]>{
 if let Some(v)=ident(c){return Some(match v.to_ascii_lowercase().as_str(){"transparent"=>[0,0,0,0],"black"=>[0,0,0,255],"white"=>[255,255,255,255],"red"=>[255,0,0,255],"green"=>[0,128,0,255],"blue"=>[0,0,255,255],"yellow"=>[255,255,0,255],"gray"|"grey"=>[128,128,128,255],_=>return None});}
 let v=match &c.kind{ast::ComponentKind::Token(Kind::Hash{value,..})=>value,_=>return None};
 if !v.is_ascii()||!v.bytes().all(|b|b.is_ascii_hexdigit()){return None;}
 let mut out=[0,0,0,255];match v.len(){3|4=>{for(i,b)in v.bytes().enumerate(){out[i]=(b as char).to_digit(16)? as u8*17;}},6|8=>{for i in 0..v.len()/2{out[i]=u8::from_str_radix(&v[i*2..i*2+2],16).ok()?;}},_=>return None}Some(out)
}
fn values(d:&ast::Declaration)->Option<Vec<(usize,Value)>>{
 if !d.valid{return None;}let name=d.name.to_ascii_lowercase();let v:Vec<_>=d.value.iter().filter(|v|!v.trivia()).collect();
 if v.is_empty(){return None;}
 let indexes:Vec<usize>=match name.as_str(){"margin"=>(3..7).collect(),"padding"=>(7..11).collect(),_=>vec![PROPS.iter().position(|p|*p==name)?]};
 if v.len()==1 {if let Some(word)=ident(v[0]){let value=match word.to_ascii_lowercase().as_str(){"initial"=>Some(Value::Initial),"inherit"=>Some(Value::Inherit),"unset"=>Some(Value::Unset),_=>None};if let Some(value)=value{return Some(indexes.into_iter().map(|i|(i,value.clone())).collect());}}}
 if indexes.len()==4 {if v.len()>4{return None;}let mut lengths=Vec::new();for c in v{let n=px(c)?;if name=="padding"&&n<0.{return None;}lengths.push(n);}let a=lengths[0];let b=*lengths.get(1).unwrap_or(&a);let c=*lengths.get(2).unwrap_or(&a);let e=*lengths.get(3).unwrap_or(&b);return Some(indexes.into_iter().zip([a,b,c,e]).map(|(i,n)|(i,Value::Scalar(n))).collect());}
 if v.len()!=1{return None;}let i=indexes[0];let c=v[0];let word=ident(c).unwrap_or("").to_ascii_lowercase();
 let value=match i{
 0=>Value::Display(match word.as_str(){"none"=>Display::None,"block"=>Display::Block,"inline"=>Display::Inline,_=>return None}),
 1|2=>Value::Length(if word=="auto"{None}else{let n=px(c)?;if n<0.{return None;}Some(n)}),
 3..=10=>{let n=px(c)?;if i>=7&&n<0.{return None;}Value::Scalar(n)},
 11=>{let n=px(c)?;if n<0.{return None;}Value::Scalar(n)},
 12=>{if word=="normal"{Value::Normal}else{let n=px(c)?;if n<0.{return None;}Value::Scalar(n)}},
 13|14=>Value::Color(color(c)?),_=>return None};Some(vec![(i,value)])
}
fn initial(node:&html::Node)->Style{let mut s=Style::default();if let html::Kind::Element{name,..}=&node.kind{s.display=if ["script","style","head","meta","link","title","template"].contains(&name.as_str()){Display::None}else if ["html","body","div","p","section","article","header","footer","main","nav","aside","ul","ol","li","h1","h2","h3","h4","h5","h6","pre","form","blockquote"].contains(&name.as_str()){Display::Block}else{Display::Inline};}else if matches!(node.kind,html::Kind::Document){s.display=Display::Block;}s}
fn inherited(i:usize)->bool{matches!(i,11|12|13)}
fn get(s:&Style,i:usize)->Value{match i{0=>Value::Display(s.display),1=>Value::Length(s.width),2=>Value::Length(s.height),3..=6=>Value::Scalar(s.margin[i-3]),7..=10=>Value::Scalar(s.padding[i-7]),11=>Value::Scalar(s.font_size),12=>if s.normal_line_height{Value::Normal}else{Value::Scalar(s.line_height)},13=>Value::Color(s.color),_=>Value::Color(s.background)}}
fn set(s:&mut Style,i:usize,v:Value){match(i,v){(0,Value::Display(v))=>s.display=v,(1,Value::Length(v))=>s.width=v,(2,Value::Length(v))=>s.height=v,(3..=6,Value::Scalar(v))=>s.margin[i-3]=v,(7..=10,Value::Scalar(v))=>s.padding[i-7]=v,(11,Value::Scalar(v))=>s.font_size=v,(12,Value::Scalar(v))=>{s.line_height=v;s.normal_line_height=false;},(12,Value::Normal)=>s.normal_line_height=true,(13,Value::Color(v))=>s.color=v,(14,Value::Color(v))=>s.background=v,_=>{}}}
fn declarations(map:&mut BTreeMap<usize,(Rank,Value)>,decls:&[ast::Declaration],base:Rank,order:&mut usize,unsupported:&mut usize){for d in decls{*order+=1;let rank=Rank{important:d.important,order:*order,..base};if let Some(items)=values(d){for(i,v)in items{if map.get(&i).map(|(r,_)|rank>=*r).unwrap_or(true){map.insert(i,(rank,v));}}}else{*unsupported+=1;}}}
/// Inline styles and each embedded style element are parsed independently.
/// Incomplete input produces observations but no provisional author declarations.
pub(super) fn compute(doc:&html::Document)->Observation{
 let mut out=Observation{styles:Vec::with_capacity(doc.nodes.len()),applied:0,unsupported:0,incomplete:!doc.source_complete||doc.stopped_by_bound,issues:Vec::new()};
 let mut rules=Vec::new();let mut stylesheet_bytes=0usize;
 for node in &doc.nodes {if let html::Kind::Element{name,attributes,..}=&node.kind {if name!="style"{continue;}if attributes.iter().any(|a|a.name=="media"&&a.value.as_deref().unwrap_or("").trim()!=""||a.name=="type"&&!a.value.as_deref().unwrap_or("").eq_ignore_ascii_case("text/css")){out.unsupported+=1;continue;}
 let mut source=String::new();for &id in &node.children{if let Some(html::Node{kind:html::Kind::Text{data,..},..})=doc.nodes.get(id){if stylesheet_bytes.saturating_add(data.len())>262144{out.incomplete=true;break;}stylesheet_bytes+=data.len();source.push_str(data);}}
 if out.incomplete{continue;}let sheet=ast::parse(&super::css_syntax::tokenize(source.as_bytes(),true));
 if sheet.stopped_by_bound||!sheet.source_complete{out.incomplete=true;continue;}
 for rule in sheet.rules{if rule.supported{match css_selectors::parse(&rule.prelude){Ok(selector)=>rules.push((selector,rule.declarations)),Err(_)=>out.unsupported+=1}}else{out.unsupported+=1;}}
 }}
 let mut work=0usize;
 for (id,node) in doc.nodes.iter().enumerate(){let defaults=initial(node);let parent=node.parent.and_then(|p|out.styles.get(p));let mut style=defaults.clone();if let Some(p)=parent{style.font_size=p.font_size;style.line_height=p.line_height;style.normal_line_height=p.normal_line_height;style.color=p.color;}
 let mut winners=BTreeMap::new();let mut order=0usize;
 if !out.incomplete&&matches!(node.kind,html::Kind::Element{..}) {for (selector,decls)in &rules {match selector.matches_with_budget(doc,id,&mut work){Ok(Some(specificity))=>declarations(&mut winners,decls,Rank{important:false,inline:false,specificity,order:0},&mut order,&mut out.unsupported),Ok(None)=>{order+=decls.len();},Err(e)=>{out.incomplete=true;if out.issues.len()<32{out.issues.push(e.code.to_string());}break;}}}
 if let html::Kind::Element{attributes,..}=&node.kind{if let Some(value)=attributes.iter().find(|a|a.name=="style").and_then(|a|a.value.as_ref()){let (decls,sheet)=ast::inline(&super::css_syntax::tokenize(value.as_bytes(),true));if !sheet.stopped_by_bound&&sheet.source_complete{declarations(&mut winners,&decls,Rank{important:false,inline:true,specificity:[0;3],order:0},&mut order,&mut out.unsupported);}else{out.incomplete=true;}}}}
 for (i,(_,v)) in winners{let v=match v{Value::Initial=>get(&Style::default(),i),Value::Inherit=>get(parent.unwrap_or(&Style::default()),i),Value::Unset=>if inherited(i){get(parent.unwrap_or(&Style::default()),i)}else{get(&Style::default(),i)},v=>v};set(&mut style,i,v);out.applied+=1;}if style.normal_line_height{style.line_height=style.font_size*1.2;}out.styles.push(style);
 }
 if out.incomplete{out.applied=0;out.styles=doc.nodes.iter().map(initial).collect();}
 out
}
#[cfg(test)]mod checks{use super::*;
#[test]fn cascade_specificity_inline_important_and_inheritance(){let d=html::parse(b"<style>p{color:red;margin:1px 2px} #x{color:blue!important} div{font-size:20px} p{color:green}</style><div><p id=x style='color:yellow'>ok</p><p style='color:white!important'>next</p></div>",true);let o=compute(&d);assert!(!o.incomplete);let p:Vec<_>=d.nodes.iter().enumerate().filter(|(_,n)|matches!(&n.kind,html::Kind::Element{name,..} if name=="p")).map(|(i,_)|&o.styles[i]).collect();assert_eq!(p[0].color,[0,0,255,255]);assert_eq!(p[0].font_size,20.);assert_eq!(p[0].margin,[1.,2.,1.,2.]);assert_eq!(p[1].color,[255;4]);}
#[test]fn unsupported_and_invalid_values_do_not_override_valid(){let d=html::parse(b"<style>p{color:red;color:var(--x);padding:-1px;width:20px;width:calc(2px)} @media all{p{color:blue}}</style><p>x</p>",true);let o=compute(&d);let i=d.nodes.iter().position(|n|matches!(&n.kind,html::Kind::Element{name,..} if name=="p")).unwrap();assert_eq!(o.styles[i].color,[255,0,0,255]);assert_eq!(o.styles[i].width,Some(20.));assert!(o.unsupported>=3);}
#[test]fn incomplete_source_never_applies_provisional_author_style(){let d=html::parse(b"<style>p{color:red}</style><p>x</p>",false);let o=compute(&d);assert!(o.incomplete);assert_eq!(o.applied,0);}
}
