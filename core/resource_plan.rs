//! Bounded resource planning over the existing lexical source-order tree.
//! Plans are data, not permission to fetch. No network or browser execution.
use std::{fmt,ops::Range};
use super::{html_document::{Document,Kind,Attribute},resource_url::{Url,resolve}};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Role{Stylesheet,ImagePng}
#[derive(Clone,Debug)]pub struct Resource{pub id:usize,pub role:Role,pub node:usize,pub target_span:Range<usize>,pub declared:String,pub resolved:Url,pub wire_url:String}
#[derive(Clone,Debug)]pub struct InlineImage{pub node:usize,pub declared:String,pub target_span:Range<usize>}
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
 fn resource(&mut self,node:usize,role:Role,a:&Attribute)->Result<Option<usize>>{let value=trim(text(a));if value.is_empty(){self.note(node,"empty-resource-url")?;return Ok(None);}let url=match resolve(&self.document_base,value){Ok(v)=>v,Err(e)=>{self.note(node,e.code)?;return Ok(None)}};if self.resources.len()+self.inline_images.len()==128{return Err(PlanError{node,code:"resource-count-bound"});}let id=self.resources.len();self.resources.push(Resource{id,role,node,target_span:a.value_span.clone().unwrap_or_else(||a.span.clone()),declared:value.to_owned(),wire_url:url.wire_url(),resolved:url});Ok(Some(id))}
}
pub fn plan(doc:&Document,final_retrieval_url:&Url)->Result<Plan>{
 if !doc.source_complete||doc.stopped_by_bound{return Err(PlanError{node:0,code:"incomplete-document-resource-plan"});}
 let mut out=Plan{retrieval_url:final_retrieval_url.clone(),document_base:final_retrieval_url.clone(),first_base_node:None,resources:vec![],inline_images:vec![],styles:vec![],notes:vec![]};
 for(node,n)in doc.nodes.iter().enumerate(){let Kind::Element{name,attributes,..}=&n.kind else{continue};if name=="base"&&!ancestor(doc,node,"template"){if let Some(a)=attr(attributes,"href"){out.first_base_node=Some(node);match resolve(final_retrieval_url,trim(text(a))){Ok(v)=>out.document_base=v,Err(e)=>out.note(node,e.code)?}break;}}}
 for(node,n)in doc.nodes.iter().enumerate(){let Kind::Element{name,attributes,..}=&n.kind else{continue};
 if name=="meta"{if let Some(a)=attr(attributes,"charset"){if !trim(text(a)).eq_ignore_ascii_case("utf-8"){out.note(node,"document-charset-not-implemented")?;}}continue;}
 if !matches!(name.as_str(),"link"|"img"|"style"){continue;}if ancestor(doc,node,"template"){out.note(node,"inert-template-resource")?;continue;}
 if name=="img"{if ancestor(doc,node,"picture")||attr(attributes,"srcset").map(|a|!trim(text(a)).is_empty()).unwrap_or(false){out.note(node,"responsive-image-selection-not-implemented")?;continue;}if let Some(a)=attr(attributes,"src"){let value=trim(text(a));if value.starts_with("data:image/png;base64,"){if out.resources.len()+out.inline_images.len()==128{return Err(PlanError{node,code:"resource-count-bound"});}out.inline_images.push(InlineImage{node,declared:value.to_owned(),target_span:a.value_span.clone().unwrap_or_else(||a.span.clone())});}else{out.resource(node,Role::ImagePng,a)?;}}else{out.note(node,"image-source-missing")?;}continue;}
 if name=="link"{let rel=attr(attributes,"rel").map(text).unwrap_or("");let tokens:Vec<&str>=rel.split_ascii_whitespace().collect();if !tokens.iter().any(|v|v.eq_ignore_ascii_case("stylesheet")){continue;}if tokens.iter().any(|v|v.eq_ignore_ascii_case("alternate")){out.note(node,"alternate-stylesheet-not-selected")?;continue;}}
 if attr(attributes,"disabled").is_some(){out.note(node,"disabled-stylesheet")?;continue;}
 if attr(attributes,"title").map(|a|!text(a).is_empty()).unwrap_or(false){out.note(node,"named-stylesheet-set-not-implemented")?;continue;}
 if attr(attributes,"media").map(|a|{let v=trim(text(a));!v.is_empty()&&!v.eq_ignore_ascii_case("all")}).unwrap_or(false){out.note(node,"conditional-stylesheet-not-implemented")?;continue;}
 if attr(attributes,"type").map(|a|{let v=text(a);!v.is_empty()&&!v.eq_ignore_ascii_case("text/css")}).unwrap_or(false){out.note(node,"stylesheet-type-not-supported")?;continue;}
 if attr(attributes,"charset").map(|a|{let v=trim(text(a));!v.is_empty()&&!v.eq_ignore_ascii_case("utf-8")}).unwrap_or(false){out.note(node,"stylesheet-charset-not-implemented")?;continue;}
 if name=="style"{out.style(node,StyleSlot::Inline{node,span:n.span.clone()})?;}else if let Some(a)=attr(attributes,"href"){if let Some(id)=out.resource(node,Role::Stylesheet,a)?{out.style(node,StyleSlot::External{node,resource:id})?;}}else{out.note(node,"stylesheet-url-missing")?;}
 }Ok(out)
}
#[cfg(test)]mod checks{
 use super::*;use super::super::{html_document,resource_url};
 fn run(source:&str,base:&str)->Plan{plan(&html_document::parse(source.as_bytes(),true),&resource_url::parse(base).unwrap()).unwrap()}
 #[test]fn final_retrieval_and_first_base(){let p=run("<base href='../assets/'><base href='https://wrong.invalid/'><img src='x.png'>","https://site.invalid/redirected/dir/page?q");assert_eq!(p.document_base.serialize(),"https://site.invalid/redirected/assets/");assert_eq!(p.resources[0].wire_url,"https://site.invalid/redirected/assets/x.png");let p=run("<base href='javascript:x'><base href='https://wrong.invalid/'><img src='x.png'>","http://100.76.22.36:8080/final/page");assert_eq!(p.resources[0].wire_url,"http://100.76.22.36:8080/final/x.png");assert_eq!(p.notes.len(),1);}
 #[test]fn relative_queries_network_path_and_fragments(){let p=run("<img src='?x=2'><img src='//[fd00::1]:8080/i.png#p'><img src='#a'>","http://localhost:8000/path/doc?q=1");assert_eq!(p.resources[0].wire_url,"http://localhost:8000/path/doc?x=2");assert_eq!(p.resources[1].wire_url,"http://[fd00::1]:8080/i.png");assert_eq!(p.resources[1].resolved.fragment(),Some("p"));assert_eq!(p.resources[2].wire_url,"http://localhost:8000/path/doc?q=1");assert_eq!(p.resources[2].resolved.fragment(),Some("a"));}
 #[test]fn exact_occurrences_spans_and_style_order(){let source="<link rel='stylesheet' href='a.css'><style>p{color:red}</style><img src='x.png'><link rel='stylesheet' href='b.css'><img src='x.png'>";let p=run(source,"https://host.invalid/");assert_eq!(p.resources.len(),4);assert_eq!(p.styles.len(),3);assert!(matches!(p.styles[0],StyleSlot::External{resource:0,..}));assert!(matches!(p.styles[1],StyleSlot::Inline{..}));assert!(matches!(p.styles[2],StyleSlot::External{resource:2,..}));assert_ne!(p.resources[1].node,p.resources[3].node);assert_eq!(p.resources[1].wire_url,p.resources[3].wire_url);for r in &p.resources{assert_eq!(&source[r.target_span.clone()],r.declared);}}
 #[test]fn unsupported_and_disabled_are_explicit(){let p=run("<link rel='alternate stylesheet' href='a'><link rel='stylesheet' disabled href='b'><style media='screen'>p{}</style><img src='c' srcset='d 2x'><picture><img src='e'></picture><link rel='stylesheet' charset='latin1' href='f'><template><img src='g'></template><img src='ok.png'>","https://host.invalid/");assert_eq!(p.resources.len(),1);assert_eq!(p.notes.len(),7);assert!(p.styles.is_empty());}
 #[test]fn hard_counts_and_incomplete_input(){let doc=html_document::parse("<img src='x'>".repeat(129).as_bytes(),true);let base=resource_url::parse("http://127.0.0.1/").unwrap();assert_eq!(plan(&doc,&base).unwrap_err().code,"resource-count-bound");let doc=html_document::parse("<style>x{}</style>".repeat(33).as_bytes(),true);assert_eq!(plan(&doc,&base).unwrap_err().code,"stylesheet-count-bound");let doc=html_document::parse(b"<img src='x'>",false);assert_eq!(plan(&doc,&base).unwrap_err().code,"incomplete-document-resource-plan");}
 #[test]fn inline_png_is_not_a_network_fetch_and_shares_bound(){let p=run("<img src='data:image/png;base64,YWJj'><img src='x.png'><img src='data:image/jpeg;base64,YWJj'>","https://host.invalid/");assert_eq!(p.inline_images.len(),1);assert_eq!(p.inline_images[0].declared,"data:image/png;base64,YWJj");assert_eq!(p.resources.len(),1);assert_eq!(p.notes.len(),1);let source=format!("{}<img src='x.png'>","<img src='data:image/png;base64,YQ=='>".repeat(128));let doc=html_document::parse(source.as_bytes(),true);assert_eq!(plan(&doc,&resource_url::parse("http://localhost/").unwrap()).unwrap_err().code,"resource-count-bound");}

}
