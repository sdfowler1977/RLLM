//! Bounded resource planning over the existing lexical source-order tree.
//! Plans are data, not permission to fetch. No network or browser execution.
use std::{fmt,ops::Range};
use super::{html_document::{Document,Kind,Attribute},resource_url::{Url,resolve}};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Role{Stylesheet,ImagePng}
#[derive(Clone,Debug)]pub struct Resource{pub role:Role,pub node:usize,pub resolved:Url,}
#[derive(Clone,Debug)]pub struct InlineImage{}
#[derive(Clone,Debug,PartialEq,Eq)]pub enum StyleSlot{Inline{node:usize,span:Range<usize>},External{node:usize,resource:usize}}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Note{pub node:usize,pub code:&'static str}
#[derive(Clone,Debug)]pub struct Plan{pub retrieval_url:Url,pub document_base:Url,pub first_base_node:Option<usize>,pub resources:Vec<Resource>,pub inline_images:Vec<InlineImage>,pub styles:Vec<StyleSlot>,pub notes:Vec<Note>}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct PlanError{pub node:usize,pub code:&'static str}
impl fmt::Display for PlanError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"{} at source node {}",self.code,self.node)}}
type Result<T>=std::result::Result<T,PlanError>;
fn attr<'a>(attrs:&'a[Attribute],name:&str)->Option<&'a Attribute>{attrs.iter().find(|a|a.name.eq_ignore_ascii_case(name))}
fn text(a:&Attribute)->&str{a.value.as_deref().unwrap_or("")}
fn trim(s:&str)->&str{s.trim_matches(|c|matches!(c,' '| '\t'|'\r'|'\n'|'\x0c'))}
fn ancestor(doc:&Document,mut node:usize,name:&str)->bool{for _ in 0..65{let Some(p)=doc.nodes.get(node).and_then(|n|n.parent)else{return false};if let Some(Kind::Element{name:n,..})=doc.nodes.get(p).map(|n|&n.kind){if n==name{return true;}}node=p;}true}
impl Plan{
 fn note(&mut self,node:usize,code:&'static str)->Result<()>{if self.notes.len()==128{return Err(PlanError{node,code:"resource-note-bound"});}self.notes.push(Note{node,code});Ok(())}
 fn style(&mut self,node:usize,slot:StyleSlot)->Result<()>{if self.styles.len()==32{return Err(PlanError{node,code:"stylesheet-count-bound"});}self.styles.push(slot);Ok(())}
 fn resource(&mut self,node:usize,role:Role,a:&Attribute)->Result<Option<usize>>{let value=trim(text(a));if value.is_empty(){self.note(node,"empty-resource-url")?;return Ok(None);}let url=match resolve(&self.document_base,value){Ok(v)=>v,Err(e)=>{self.note(node,e.code)?;return Ok(None)}};if self.resources.len()+self.inline_images.len()==128{return Err(PlanError{node,code:"resource-count-bound"});}let id=self.resources.len();self.resources.push(Resource{role,node,resolved:url});Ok(Some(id))}
}
pub fn plan(doc:&Document,final_retrieval_url:&Url)->Result<Plan>{
 if !doc.source_complete||doc.stopped_by_bound{return Err(PlanError{node:0,code:"incomplete-document-resource-plan"});}
 let mut out=Plan{retrieval_url:final_retrieval_url.clone(),document_base:final_retrieval_url.clone(),first_base_node:None,resources:vec![],inline_images:vec![],styles:vec![],notes:vec![]};
 for(node,n)in doc.nodes.iter().enumerate(){let Kind::Element{name,attributes,..}=&n.kind else{continue};if name=="base"&&!ancestor(doc,node,"template"){if let Some(a)=attr(attributes,"href"){out.first_base_node=Some(node);match resolve(final_retrieval_url,trim(text(a))){Ok(v)=>out.document_base=v,Err(e)=>out.note(node,e.code)?}break;}}}
 for(node,n)in doc.nodes.iter().enumerate(){let Kind::Element{name,attributes,..}=&n.kind else{continue};
 if name=="meta"{if let Some(a)=attr(attributes,"charset"){if !trim(text(a)).eq_ignore_ascii_case("utf-8"){out.note(node,"document-charset-not-implemented")?;}}continue;}
 if !matches!(name.as_str(),"link"|"img"|"style"){continue;}if ancestor(doc,node,"template"){out.note(node,"inert-template-resource")?;continue;}
 if name=="img"{if ancestor(doc,node,"picture")||attr(attributes,"srcset").map(|a|!trim(text(a)).is_empty()).unwrap_or(false){out.note(node,"responsive-image-selection-not-implemented")?;continue;}if let Some(a)=attr(attributes,"src"){let value=trim(text(a));if value.starts_with("data:image/png;base64,"){if out.resources.len()+out.inline_images.len()==128{return Err(PlanError{node,code:"resource-count-bound"});}out.inline_images.push(InlineImage{});}else{out.resource(node,Role::ImagePng,a)?;}}else{out.note(node,"image-source-missing")?;}continue;}
 if name=="link"{let rel=attr(attributes,"rel").map(text).unwrap_or("");let tokens:Vec<&str>=rel.split_ascii_whitespace().collect();if !tokens.iter().any(|v|v.eq_ignore_ascii_case("stylesheet")){continue;}if tokens.iter().any(|v|v.eq_ignore_ascii_case("alternate")){out.note(node,"alternate-stylesheet-not-selected")?;continue;}}
 if attr(attributes,"disabled").is_some(){out.note(node,"disabled-stylesheet")?;continue;}
 if attr(attributes,"title").map(|a|!text(a).is_empty()).unwrap_or(false){out.note(node,"named-stylesheet-set-not-implemented")?;continue;}
 if attr(attributes,"media").map(|a|{let v=trim(text(a));!v.is_empty()&&!v.eq_ignore_ascii_case("all")}).unwrap_or(false){out.note(node,"conditional-stylesheet-not-implemented")?;continue;}
 if attr(attributes,"type").map(|a|{let v=text(a);!v.is_empty()&&!v.eq_ignore_ascii_case("text/css")}).unwrap_or(false){out.note(node,"stylesheet-type-not-supported")?;continue;}
 if attr(attributes,"charset").map(|a|{let v=trim(text(a));!v.is_empty()&&!v.eq_ignore_ascii_case("utf-8")}).unwrap_or(false){out.note(node,"stylesheet-charset-not-implemented")?;continue;}
 if name=="style"{out.style(node,StyleSlot::Inline{node,span:n.span.clone()})?;}else if let Some(a)=attr(attributes,"href"){if let Some(id)=out.resource(node,Role::Stylesheet,a)?{out.style(node,StyleSlot::External{node,resource:id})?;}}else{out.note(node,"stylesheet-url-missing")?;}
 }Ok(out)
}
