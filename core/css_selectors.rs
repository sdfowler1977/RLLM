//! Original strict selector subset for html_document's lexical source-order tree.
//! This does not manufacture a browser DOM, layout state or dynamic pseudo-state.
use super::css_stylesheet::{Component,ComponentKind};
use super::css_syntax::Kind;
use super::html_document::{Document,Kind as Html};
use std::ops::Range;
const LIST:usize=64;const PARTS:usize=64;const WORK:usize=131072;
pub(super) type Specificity=[u16;3];
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct SelectorError {pub offset:usize,pub code:&'static str}
type Result<T>=std::result::Result<T,SelectorError>;
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) enum Operator {Present,Equal,Word,Dash,Prefix,Suffix,Substring}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Attribute {pub name:String,pub operator:Operator,pub value:String,pub insensitive:bool}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) enum Simple {Type(String),Universal,Id(String),Class(String),Attribute(Attribute),Root,FirstChild,LastChild,OnlyChild,FirstOfType,LastOfType,OnlyOfType,Empty}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) enum Combinator {Descendant,Child,Adjacent,Sibling}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Compound {pub simples:Vec<Simple>}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Selector {pub compounds:Vec<Compound>,pub combinators:Vec<Combinator>,pub specificity:Specificity,pub span:Range<usize>}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct SelectorList {pub selectors:Vec<Selector>}
fn error(c:Option<&Component>,code:&'static str)->SelectorError{SelectorError{offset:c.map(|c|c.span.start).unwrap_or(0),code}}
fn ident(c:&Component)->Option<&str>{if let ComponentKind::Token(Kind::Ident(v))=&c.kind{Some(v)}else{None}}
fn delim(c:&Component,d:char)->bool{matches!(c.kind,ComponentKind::Token(Kind::Delim(v)) if v==d)}
fn enum_attribute(name:&str)->bool{matches!(name,"type"|"dir"|"draggable"|"contenteditable"|"translate"|"autocomplete"|"crossorigin"|"method"|"enctype"|"target"|"rel"|"charset"|"http-equiv"|"lang"|"align"|"valign"|"scope"|"shape"|"wrap"|"preload"|"loading"|"decoding"|"spellcheck")}
fn attribute(values:&[Component])->Result<Attribute>{
 let v:Vec<&Component>=values.iter().filter(|c|!c.trivia()).collect();let name=v.first().and_then(|c|ident(c)).ok_or_else(||error(v.first().copied(),"attribute-name-unsupported"))?.to_ascii_lowercase();
 if v.len()==1{return Ok(Attribute{name,operator:Operator::Present,value:String::new(),insensitive:false});}
 let mut p=1;let operator=if delim(v[p],'='){p+=1;Operator::Equal}else{
  let op=match v[p].kind{ComponentKind::Token(Kind::Delim('~'))=>Operator::Word,ComponentKind::Token(Kind::Delim('|'))=>Operator::Dash,ComponentKind::Token(Kind::Delim('^'))=>Operator::Prefix,ComponentKind::Token(Kind::Delim('$'))=>Operator::Suffix,ComponentKind::Token(Kind::Delim('*'))=>Operator::Substring,_=>return Err(error(Some(v[p]),"attribute-operator-unsupported"))};let operator_end=v[p].span.end;p+=1;if v.get(p).map(|c|delim(c,'=')&&!values.iter().any(|between|between.span.start>=operator_end&&between.span.end<=c.span.start&&between.whitespace()))!=Some(true){return Err(error(v.get(p).copied(),"attribute-operator-missing-equals"));}p+=1;op};
 let value=match v.get(p).map(|c|&c.kind){Some(ComponentKind::Token(Kind::Ident(v)|Kind::String(v)))=>v.clone(),_=>return Err(error(v.get(p).copied(),"attribute-value-required"))};p+=1;
 let explicit=v.get(p).and_then(|c|ident(c));let insensitive=match explicit{Some(v) if v.eq_ignore_ascii_case("i")=>{p+=1;true},Some(v) if v.eq_ignore_ascii_case("s")=>{p+=1;false},Some(_)=>return Err(error(v.get(p).copied(),"attribute-case-flag-unsupported")),None=>{if enum_attribute(&name){return Err(error(v.first().copied(),"html-attribute-default-case-semantics-require-explicit-flag"));}false}};
 if p!=v.len(){return Err(error(v.get(p).copied(),"attribute-trailing-syntax"));}Ok(Attribute{name,operator,value,insensitive})
}
fn compound(v:&[&Component],p:&mut usize)->Result<Compound>{
 let mut simples=Vec::new();if let Some(c)=v.get(*p){if let Some(name)=ident(c){simples.push(Simple::Type(name.to_ascii_lowercase()));*p+=1;}else if delim(c,'*'){simples.push(Simple::Universal);*p+=1;}}
 while *p<v.len(){let c=v[*p];if c.whitespace(){break;}let simple=match &c.kind{
  ComponentKind::Token(Kind::Hash{value,id:true})=>{*p+=1;Simple::Id(value.clone())},
  ComponentKind::Token(Kind::Delim('.'))=>{*p+=1;let name=v.get(*p).and_then(|c|ident(c)).ok_or_else(||error(v.get(*p).copied(),"class-ident-required"))?;*p+=1;Simple::Class(name.to_string())},
  ComponentKind::Block('[',values)=>{if !c.closed{return Err(error(Some(c),"unclosed-attribute-selector"));}*p+=1;Simple::Attribute(attribute(values)?)},
  ComponentKind::Token(Kind::Colon)=>{*p+=1;let name=v.get(*p).and_then(|c|ident(c)).ok_or_else(||error(v.get(*p).copied(),"functional-or-pseudo-element-selector-unsupported"))?;*p+=1;match name.to_ascii_lowercase().as_str(){"root"=>Simple::Root,"first-child"=>Simple::FirstChild,"last-child"=>Simple::LastChild,"only-child"=>Simple::OnlyChild,"first-of-type"=>Simple::FirstOfType,"last-of-type"=>Simple::LastOfType,"only-of-type"=>Simple::OnlyOfType,"empty"=>Simple::Empty,_=>return Err(error(Some(c),"pseudo-class-unsupported"))}},
  ComponentKind::Token(Kind::Comma)|ComponentKind::Token(Kind::Delim('>'|'+'|'~'))=>break,
  _=>return Err(error(Some(c),"selector-syntax-unsupported"))
 };simples.push(simple);if simples.len()>PARTS{return Err(error(Some(c),"compound-bound"));}}
 if simples.is_empty(){return Err(error(v.get(*p).copied(),"empty-compound"));}Ok(Compound{simples})
}
pub(super) fn parse(values:&[Component])->Result<SelectorList>{
 // Comments disappear, but they do not create a descendant combinator.
 let v:Vec<&Component>=values.iter().filter(|c|!matches!(c.kind,ComponentKind::Token(Kind::Comment(_)))).collect();
 if v.len()>4096{return Err(error(v.first().copied(),"selector-token-bound"));}if v.iter().any(|c|!c.closed){return Err(error(v.first().copied(),"selector-unclosed-component"));}
 let mut p=0;let mut selectors=Vec::new();
 loop{
  while p<v.len()&&v[p].whitespace(){p+=1;}if p==v.len(){return Err(error(v.last().copied(),"empty-selector-or-trailing-comma"));}
  let start=v[p].span.start;let mut compounds=vec![compound(&v,&mut p)?];let mut combinators=Vec::new();
  loop{
   let before=p;while p<v.len()&&v[p].whitespace(){p+=1;}let space=p>before;
   if p==v.len()||matches!(v[p].kind,ComponentKind::Token(Kind::Comma)){break;}
   let combinator=if delim(v[p],'>'){p+=1;Combinator::Child}else if delim(v[p],'+'){p+=1;Combinator::Adjacent}else if delim(v[p],'~'){p+=1;Combinator::Sibling}else if space{Combinator::Descendant}else{return Err(error(Some(v[p]),"missing-combinator"));};
   while p<v.len()&&v[p].whitespace(){p+=1;}if compounds.len()>=PARTS{return Err(error(v.get(p).copied(),"selector-chain-bound"));}combinators.push(combinator);compounds.push(compound(&v,&mut p)?);
  }
  let mut specificity=[0u16;3];for c in &compounds{for s in &c.simples{match s{Simple::Id(_)=>specificity[0]+=1,Simple::Type(_)=>specificity[2]+=1,Simple::Universal=>{},_=>specificity[1]+=1}}}
  selectors.push(Selector{compounds,combinators,specificity,span:start..v[p-1].span.end});if selectors.len()>LIST{return Err(error(v.get(p).copied(),"selector-list-bound"));}
  if p==v.len(){break;}p+=1;
 }Ok(SelectorList{selectors})
}
fn element(doc:&Document,node:usize)->bool{matches!(doc.nodes.get(node).map(|n|&n.kind),Some(Html::Element{..}))}
fn parent(doc:&Document,node:usize)->Option<usize>{doc.nodes.get(node)?.parent.filter(|p|element(doc,*p))}
fn siblings(doc:&Document,node:usize)->Vec<usize>{doc.nodes.get(node).and_then(|n|n.parent).and_then(|p|doc.nodes.get(p)).map(|p|p.children.iter().copied().filter(|n|element(doc,*n)).collect()).unwrap_or_default()}
// The lexical HTML tree stores normalized element names. Text/comments and
// descendants are not siblings of this type. Scan borrowed nodes without a
// temporary sibling vector; the caller charges all inspected names first.
fn type_position(doc:&Document,node:usize,name:&str)->(bool,bool){
 let children=doc.nodes.get(node).and_then(|n|n.parent).and_then(|p|doc.nodes.get(p)).map(|p|p.children.as_slice()).unwrap_or(&[]);
 let mut first=None;let mut last=None;
 for &i in children{if matches!(doc.nodes.get(i).map(|n|&n.kind),Some(Html::Element{name:other,..}) if other==name){if first.is_none(){first=Some(i);}last=Some(i);}}
 (first==Some(node),last==Some(node))
}
fn ascii_space(c:char)->bool{matches!(c,' '| '\t'|'\r'|'\n'|'\u{c}')}
fn attr_matches(actual:&str,test:&Attribute)->bool{
 let (a,v)=if test.insensitive{(actual.to_ascii_lowercase(),test.value.to_ascii_lowercase())}else{(actual.to_string(),test.value.clone())};
 match test.operator{Operator::Present=>true,Operator::Equal=>a==v,Operator::Word=>!v.is_empty()&&!v.chars().any(ascii_space)&&a.split(ascii_space).any(|w|w==v),Operator::Dash=>a==v||a.strip_prefix(&v).map(|r|r.starts_with('-')).unwrap_or(false),Operator::Prefix=>!v.is_empty()&&a.starts_with(&v),Operator::Suffix=>!v.is_empty()&&a.ends_with(&v),Operator::Substring=>!v.is_empty()&&a.contains(&v)}
}
fn compound_matches(c:&Compound,doc:&Document,node:usize,work:&mut usize)->Result<bool>{
 let Some(n)=doc.nodes.get(node)else{return Ok(false)};let Html::Element{name,attributes,..}=&n.kind else{return Ok(false)};
 let attr=|key:&str|attributes.iter().find(|a|a.name.eq_ignore_ascii_case(key)).map(|a|a.value.as_deref().unwrap_or(""));
 for s in &c.simples{
  let family=n.parent.and_then(|p|doc.nodes.get(p)).map(|p|p.children.len()).unwrap_or(0);
  let selector_cost=match s{Simple::Type(v)|Simple::Id(v)|Simple::Class(v)=>v.len(),Simple::Attribute(a)=>a.name.len()+a.value.len(),_=>0};
  let cost=1+selector_cost+attributes.iter().map(|a|a.name.len()+a.value.as_ref().map(String::len).unwrap_or(0)).sum::<usize>()+match s{Simple::FirstChild|Simple::LastChild|Simple::OnlyChild=>family,
   Simple::FirstOfType|Simple::LastOfType|Simple::OnlyOfType=>family+ n.parent.and_then(|p|doc.nodes.get(p)).map(|p|p.children.iter().filter_map(|i|doc.nodes.get(*i)).map(|n|if let Html::Element{name:other,..}=&n.kind{name.len().min(other.len())+1}else{0}).sum::<usize>()).unwrap_or(0),Simple::Empty=>n.children.iter().filter_map(|i|doc.nodes.get(*i)).map(|n|if let Html::Text{data,..}=&n.kind{data.len()+1}else{1}).sum(),_=>0};
  *work=work.saturating_add(cost);if *work>WORK{return Err(SelectorError{offset:n.span.start,code:"selector-match-work-bound"});}
  let matched=match s{
  Simple::Type(t)=>name.eq_ignore_ascii_case(t),Simple::Universal=>true,Simple::Id(id)=>attr("id")==Some(id.as_str()),Simple::Class(class)=>attr("class").map(|v|v.split(ascii_space).any(|w|w==class)).unwrap_or(false),Simple::Attribute(a)=>attr(&a.name).map(|v|attr_matches(v,a)).unwrap_or(false),
  Simple::Root=>n.parent.and_then(|p|doc.nodes.get(p)).map(|p|matches!(p.kind,Html::Document)).unwrap_or(false),
  Simple::FirstChild=>siblings(doc,node).first()==Some(&node),Simple::LastChild=>siblings(doc,node).last()==Some(&node),Simple::OnlyChild=>siblings(doc,node)==vec![node],
  Simple::FirstOfType=>type_position(doc,node,name).0,
  Simple::LastOfType=>type_position(doc,node,name).1,
  Simple::OnlyOfType=>type_position(doc,node,name)==(true,true),
  Simple::Empty=>n.children.iter().all(|i|match doc.nodes.get(*i).map(|n|&n.kind){Some(Html::Text{data,..})=>data.chars().all(ascii_space),Some(Html::Element{..})=>false,Some(_)=>true,None=>false})
 };if !matched{return Ok(false);}}Ok(true)
}
fn match_at(selector:&Selector,part:usize,doc:&Document,node:usize,work:&mut usize)->Result<bool>{
 if *work>=WORK{return Err(SelectorError{offset:selector.span.start,code:"selector-match-work-bound"});}*work+=1;
 if !compound_matches(&selector.compounds[part],doc,node,work)?{return Ok(false);}if part==0{return Ok(true);}
 match selector.combinators[part-1]{
  Combinator::Child=>if let Some(p)=parent(doc,node){match_at(selector,part-1,doc,p,work)}else{Ok(false)},
  Combinator::Descendant=>{let mut p=parent(doc,node);while let Some(i)=p{if match_at(selector,part-1,doc,i,work)?{return Ok(true);}p=parent(doc,i);}Ok(false)},
  Combinator::Adjacent|Combinator::Sibling=>{*work=work.saturating_add(doc.nodes.get(node).and_then(|n|n.parent).and_then(|p|doc.nodes.get(p)).map(|p|p.children.len()).unwrap_or(0));if *work>WORK{return Err(SelectorError{offset:selector.span.start,code:"selector-match-work-bound"});}let s=siblings(doc,node);let Some(index)=s.iter().position(|i|*i==node)else{return Ok(false)};for i in s[..index].iter().rev(){if match_at(selector,part-1,doc,*i,work)?{return Ok(true);}if selector.combinators[part-1]==Combinator::Adjacent{break;}}Ok(false)}
 }
}
impl SelectorList {
 /// Maximum specificity among matching list members. No unsupported selector
 /// is treated as a universal selector or a successful non-match.
 pub fn matches(&self,doc:&Document,node:usize)->Result<Option<Specificity>>{self.matches_with_budget(doc,node,&mut 0)}
 pub fn matches_with_budget(&self,doc:&Document,node:usize,work:&mut usize)->Result<Option<Specificity>>{
  if !doc.source_complete||doc.stopped_by_bound{return Err(SelectorError{offset:0,code:"source-order-tree-incomplete"});}
  if !element(doc,node){return Ok(None);}let mut best=None;
  for selector in &self.selectors{if match_at(selector,selector.compounds.len()-1,doc,node,work)?{best=Some(best.map(|b:Specificity|b.max(selector.specificity)).unwrap_or(selector.specificity));}}Ok(best)
 }
}
#[cfg(test)]mod checks{
 use super::*;use super::super::{css_syntax,css_stylesheet,html_document};
 fn selectors(text:&str)->Result<SelectorList>{let sheet=css_stylesheet::parse(&css_syntax::tokenize(format!("{text}{{x:y}}").as_bytes(),true));parse(&sheet.rules[0].prelude)}
 #[test]fn compounds_combinators_and_specificity(){let doc=html_document::parse(b"<section id='root'><p class='a b' data-x='one two'></p>text<span></span><p class='b'></p></section>",true);let ids=doc.nodes.iter().enumerate().filter(|(_,n)|matches!(&n.kind,Html::Element{name,..} if name=="p")).map(|(i,_)|i).collect::<Vec<_>>();let s=selectors("#root > p.a[data-x~=two]:first-child, span + p.b").unwrap();assert_eq!(s.matches(&doc,ids[0]).unwrap(),Some([1,3,1]));assert_eq!(s.matches(&doc,ids[1]).unwrap(),Some([0,1,2]));assert!(selectors("section p:last-child").unwrap().matches(&doc,ids[1]).unwrap().is_some());assert!(selectors("p ~ p").unwrap().matches(&doc,ids[1]).unwrap().is_some());}
 #[test]fn unsupported_is_error_not_broadened(){for text in ["p:hover","p::before","p:not(.a)","svg|p","p,", "p/**/span", "[type=text]","[a?=x]","p >", "#123"]{assert!(selectors(text).is_err(),"{text}");}assert!(selectors("[type=TEXT i]").is_ok());assert!(selectors(".a/**/.b").is_ok());}
 #[test]fn operator_spacing_and_shared_work_bound(){
  assert!(selectors("[data-x ~ = word]").is_err());assert!(selectors("[data-x ~/**/= word]").is_ok());
  let doc=html_document::parse(b"<p class=x></p>",true);let selector=selectors("p.x").unwrap();let mut budget=WORK;assert!(selector.matches_with_budget(&doc,1,&mut budget).is_err());
 }
 #[test]fn attributes_case_empty_and_partial_tree(){let doc=html_document::parse(b"<input TYPE='TEXT' data-v='Abc-def' class='X'><p> \n</p>",true);let input=doc.nodes.iter().position(|n|matches!(&n.kind,Html::Element{name,..} if name=="input")).unwrap();assert!(selectors("[type=text i][data-v^=Abc][data-v$=def][data-v*=c-d]").unwrap().matches(&doc,input).unwrap().is_some());assert!(selectors("[data-v^='']").unwrap().matches(&doc,input).unwrap().is_none());assert!(selectors(".x").unwrap().matches(&doc,input).unwrap().is_none());let partial=html_document::parse(b"<p>",false);assert!(selectors("p").unwrap().matches(&partial,1).is_err());}
}
