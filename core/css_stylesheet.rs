//! Original bounded CSS syntax AST over source-spanned CSS tokens.
//! Grammar only: property values, at-rule semantics, nesting and cascade are not
//! certified here. Preserved unsupported syntax is never silently executable.
use super::css_syntax::{Kind,SheetTokens};
use std::ops::Range;
const DEPTH:usize=64;
const RULES:usize=2048;
const DECLARATIONS:usize=8192;
const ISSUES:usize=128;
#[derive(Clone,Debug,PartialEq)]
pub(super) enum ComponentKind { Token(Kind), Block(char,Vec<Component>), Function(String,Vec<Component>) }
#[derive(Clone,Debug,PartialEq)]
pub(super) struct Component {pub kind:ComponentKind,pub span:Range<usize>,pub closed:bool}
impl Component {
 pub fn trivia(&self)->bool{matches!(self.kind,ComponentKind::Token(Kind::Whitespace(_)|Kind::Comment(_)))}
 pub fn whitespace(&self)->bool{matches!(self.kind,ComponentKind::Token(Kind::Whitespace(_)))}
}
#[derive(Clone,Debug,PartialEq)]
pub(super) struct Declaration {pub name:String,pub value:Vec<Component>,pub important:bool,pub span:Range<usize>,pub valid:bool}
#[derive(Clone,Debug,PartialEq)]
pub(super) enum RuleKind {Qualified,At(String)}
#[derive(Clone,Debug,PartialEq)]
pub(super) struct Rule {
 pub kind:RuleKind,pub prelude:Vec<Component>,pub block:Option<Component>,pub declarations:Vec<Declaration>,pub span:Range<usize>,
 /// Qualified syntax is structurally usable only; selector and property support
 /// must still be checked. All at-rules remain unsupported semantic records.
 pub supported:bool,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Issue {pub offset:usize,pub code:&'static str}
#[derive(Clone,Debug)]
pub(super) struct Stylesheet {pub rules:Vec<Rule>,pub issues:Vec<Issue>,pub omitted_issues:usize,pub source_complete:bool,pub stopped_by_bound:bool,pub parsed_bytes:usize}
impl Stylesheet {
 fn issue(&mut self,offset:usize,code:&'static str){if self.issues.len()<ISSUES{self.issues.push(Issue{offset,code})}else{self.omitted_issues+=1}}
 pub fn summary(&self)->String{format!("Native CSS syntax AST: {} rules; {} declarations; {} unsupported at-rules; {} issues, {} omitted; complete source: {}; bound stop: {}; {} parsed bytes. Exact source spans retained. Property semantics, nested rules and conditional at-rules require separate support; no resource fetch, browser DOM, layout or paint.",self.rules.len(),self.rules.iter().map(|r|r.declarations.len()).sum::<usize>(),self.rules.iter().filter(|r|matches!(r.kind,RuleKind::At(_))).count(),self.issues.len(),self.omitted_issues,self.source_complete,self.stopped_by_bound,self.parsed_bytes)}
}
fn leaf(kind:&Kind)->bool{matches!(kind,Kind::BadString(_)|Kind::BadUrl|Kind::Close(_))}
fn valid(c:&Component)->bool{c.closed&&match &c.kind{ComponentKind::Token(k)=>!leaf(k),ComponentKind::Block(_,v)|ComponentKind::Function(_,v)=>v.iter().all(valid)}}
fn components(tokens:&SheetTokens,at:&mut usize,end:Option<char>,depth:usize,out:&mut Stylesheet)->(Vec<Component>,bool,usize){
 let mut values=Vec::new();let mut last=tokens.tokens.get(*at).map(|t|t.span.start).unwrap_or(tokens.parsed_bytes);
 while *at<tokens.tokens.len(){let token=&tokens.tokens[*at];
  if let Kind::Close(c)=token.kind{if Some(c)==end{*at+=1;return(values,true,token.span.end);}}
  if depth>=DEPTH{out.issue(token.span.start,"component-depth-bound");out.stopped_by_bound=true;return(values,false,last);}
  *at+=1;let (kind,closed,finish)=match &token.kind{
   Kind::Open(c)=>{let expected=match c{'('=>')','['=>']',_=>'}'};let(v,closed,finish)=components(tokens,at,Some(expected),depth+1,out);(ComponentKind::Block(*c,v),closed,finish)},
   Kind::Function(name)=>{let(v,closed,finish)=components(tokens,at,Some(')'),depth+1,out);(ComponentKind::Function(name.clone(),v),closed,finish)},
   k=>{if leaf(k){out.issue(token.span.start,"invalid-component-token");}(ComponentKind::Token(k.clone()),true,token.span.end)}
  };last=finish.max(token.span.end);values.push(Component{kind,span:token.span.start..last,closed});
  if out.stopped_by_bound{break;}
 }
 if end.is_some(){out.issue(last,"component-end-not-observed");}(values,end.is_none(),last)
}
fn trimmed(mut values:&[Component])->&[Component]{while values.first().map(Component::trivia)==Some(true){values=&values[1..];}while values.last().map(Component::trivia)==Some(true){values=&values[..values.len()-1];}values}
fn declaration(values:&[Component],out:&mut Stylesheet)->Option<Declaration>{
 let values=trimmed(values);if values.is_empty(){return None;}let start=values[0].span.start;
 let name=match &values[0].kind{ComponentKind::Token(Kind::Ident(name))=>name.clone(),_=>{out.issue(start,"nondeclaration-block-item-preserved");return None;}};
 let mut p=1;while p<values.len()&&values[p].trivia(){p+=1;}
 if !matches!(values.get(p).map(|c|&c.kind),Some(ComponentKind::Token(Kind::Colon))){out.issue(start,"declaration-missing-colon-or-nested-rule-preserved");return None;}
 let raw=trimmed(&values[p+1..]);let mut stop=raw.len();let mut important=false;
 if let Some(Component{kind:ComponentKind::Token(Kind::Ident(word)),..})=raw.last(){if word.eq_ignore_ascii_case("important"){
  let mut mark=raw.len()-1;while mark>0&&raw[mark-1].trivia(){mark-=1;}
  if mark>0&&matches!(raw[mark-1].kind,ComponentKind::Token(Kind::Delim('!'))){important=true;stop=mark-1;}
 }}
 let value=trimmed(&raw[..stop]).to_vec();let mixed_curly=!name.starts_with("--")&&value.iter().any(|c|matches!(c.kind,ComponentKind::Block('{',_)))&&value.iter().filter(|c|!c.trivia()).count()>1;
 let sound=!mixed_curly&&value.iter().all(valid)&&!value.iter().any(|c|matches!(c.kind,ComponentKind::Token(Kind::Delim('!'))));
 if !sound{out.issue(start,"invalid-declaration-component");}
 Some(Declaration{name:if name.starts_with("--"){name}else{name.to_ascii_lowercase()},value,important,span:start..values.last().unwrap().span.end,valid:sound})
}
fn declarations(values:&[Component],out:&mut Stylesheet,total:&mut usize)->Vec<Declaration>{
 let mut results=Vec::new();let mut start=0;let mut p=0;
 while p<=values.len(){
  if p<values.len()&&matches!(values[p].kind,ComponentKind::Block('{',_)){
   let prefix=trimmed(&values[start..p]);let colon=prefix.iter().position(|c|!c.trivia()&&matches!(c.kind,ComponentKind::Token(Kind::Colon)));
   let declaration_prefix=matches!(prefix.first().map(|c|&c.kind),Some(ComponentKind::Token(Kind::Ident(_))))&&colon.map(|i|prefix[1..i].iter().all(Component::trivia)).unwrap_or(false);
   if !declaration_prefix{out.issue(values.get(start).map(|c|c.span.start).unwrap_or(values[p].span.start),"nested-rule-or-at-rule-preserved-not-evaluated");start=p+1;p+=1;continue;}
  }
  if p==values.len()||matches!(values[p].kind,ComponentKind::Token(Kind::Semicolon)){
   if *total>=DECLARATIONS{out.issue(values.get(start).map(|c|c.span.start).unwrap_or(0),"declaration-count-bound");out.stopped_by_bound=true;break;}
   if let Some(d)=declaration(&values[start..p],out){*total+=1;results.push(d);}start=p+1;
  }p+=1;
 }results
}
/// Parse inline declaration syntax using the same original-byte component AST.
/// Rules and semantic property validation are deliberately separate consumers.
pub(super) fn inline(tokens:&SheetTokens)->(Vec<Declaration>,Stylesheet){
 let mut out=empty(tokens);let mut at=0;let(values,_,_)=components(tokens,&mut at,None,0,&mut out);let mut total=0;let declarations=declarations(&values,&mut out,&mut total);(declarations,out)
}
fn empty(tokens:&SheetTokens)->Stylesheet{Stylesheet{rules:Vec::new(),issues:tokens.issues.iter().map(|i|Issue{offset:i.offset,code:i.code}).take(ISSUES).collect(),omitted_issues:tokens.omitted_issues,source_complete:tokens.source_complete,stopped_by_bound:tokens.stopped_by_bound,parsed_bytes:tokens.parsed_bytes}}
pub(super) fn parse(tokens:&SheetTokens)->Stylesheet{
 let mut out=empty(tokens);let mut at=0;let(values,_,_)=components(tokens,&mut at,None,0,&mut out);let mut p=0;let mut total=0;
 while p<values.len(){
  if values[p].trivia()||matches!(values[p].kind,ComponentKind::Token(Kind::Cdo|Kind::Cdc)){p+=1;continue;}
  if out.rules.len()>=RULES{out.issue(values[p].span.start,"rule-count-bound");out.stopped_by_bound=true;break;}
  let start=p;let kind=match &values[p].kind{ComponentKind::Token(Kind::AtKeyword(name))=>{p+=1;RuleKind::At(name.to_ascii_lowercase())},_=>RuleKind::Qualified};let prelude_start=p;let mut block=None;let mut terminated=false;
  while p<values.len(){
   if matches!(values[p].kind,ComponentKind::Block('{',_)){block=Some(values[p].clone());p+=1;terminated=true;break;}
   if matches!(kind,RuleKind::At(_))&&matches!(values[p].kind,ComponentKind::Token(Kind::Semicolon)){p+=1;terminated=true;break;}
   p+=1;
  }
  let prelude_end=if terminated{p-1}else{p};let prelude=trimmed(&values[prelude_start..prelude_end]).to_vec();
  let mut supported=matches!(kind,RuleKind::Qualified)&&block.as_ref().map(|b|b.closed)==Some(true)&&prelude.iter().all(valid)&&!prelude.is_empty();
  let decls=if matches!(kind,RuleKind::Qualified){if let Some(Component{kind:ComponentKind::Block(_,body),..})=&block{declarations(body,&mut out,&mut total)}else{out.issue(values[start].span.start,"qualified-rule-missing-block");vec![]}}else{out.issue(values[start].span.start,"at-rule-preserved-not-evaluated");vec![]};
  if !out.source_complete||out.stopped_by_bound{supported=false;}
  out.rules.push(Rule{kind,prelude,block,declarations:decls,span:values[start].span.start..values[p-1].span.end,supported});
 }
 // Any bound stop makes this AST provisional, including rules preceding it.
 if out.stopped_by_bound||!out.source_complete{for rule in &mut out.rules{rule.supported=false;}}
 out
}
#[cfg(test)]mod checks{
 use super::*;use super::super::css_syntax::tokenize;
 #[test]fn rules_declarations_nested_components_and_importance(){let source="/*x*/ .a { COLOR: rgb(1, 2, 3) ! /*x*/ IMPORTANT; --X: [a;b] {x:y}; width: 2px; } @media (width>1px){p{color:red}} @import url(x);";let out=parse(&tokenize(source.as_bytes(),true));assert_eq!(out.rules.len(),3);let rule=&out.rules[0];assert!(rule.supported);assert_eq!(rule.declarations.len(),3);assert_eq!(rule.declarations[0].name,"color");assert!(rule.declarations[0].important);assert_eq!(rule.declarations[1].name,"--X");assert!(!rule.declarations[1].important);assert!(out.rules[1..].iter().all(|r|!r.supported));for r in &out.rules{assert!(source.get(r.span.clone()).is_some());}}
 #[test]fn errors_are_preserved_and_recover_at_semicolon(){let out=parse(&tokenize(b"p{broken; color:red; width: func(!important); x: 'bad\n; height:2px} q{a:b",true));assert_eq!(out.rules.len(),2);assert_eq!(out.rules[0].declarations[0].name,"color");assert!(!out.rules[0].declarations[1].important);assert!(!out.rules[1].supported);assert!(!out.issues.is_empty());}
 #[test]fn nested_rule_recovery_and_top_level_bang(){let source="p{color:red;.child{x:y} width:2px; @media x{a{b:c}} height:3px; z: !no; ok:yes}";let out=parse(&tokenize(source.as_bytes(),true));let r=&out.rules[0];assert!(r.supported);assert_eq!(r.declarations.iter().map(|d|d.name.as_str()).collect::<Vec<_>>(),vec!["color","width","height","z","ok"]);assert!(!r.declarations[3].valid);assert!(r.declarations[4].valid);}
 #[test]fn generated_components_have_bounded_valid_spans(){let pieces=["é", "\\", "!important", "\n", "/*", "*/", "{", "}", "[", "]", "(", ")", "'", "x:", ";", "@media", " "];let mut seed=71u64;for _ in 0..512{let mut source=String::new();for _ in 0..48{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);source.push_str(pieces[(seed>>32)as usize%pieces.len()]);}let out=parse(&tokenize(source.as_bytes(),true));assert!(out.rules.len()<=RULES&&out.issues.len()<=ISSUES);for rule in out.rules{assert!(source.get(rule.span).is_some());for d in rule.declarations{assert!(source.get(d.span).is_some());}}}}
 #[test]fn stylesheet_semicolon_remains_in_qualified_prelude(){let out=parse(&tokenize(b";p{color:red}",true));assert_eq!(out.rules.len(),1);assert!(matches!(out.rules[0].prelude[0].kind,ComponentKind::Token(Kind::Semicolon)));assert!(super::super::css_selectors::parse(&out.rules[0].prelude).is_err());}
 #[test]fn inline_exact_spans_and_prefix_no_commit(){let source="é:1;--X: !important; color: red !important ";let(d,out)=inline(&tokenize(source.as_bytes(),true));assert!(!out.stopped_by_bound);assert_eq!(d.len(),3);assert_eq!(&source[d[0].span.clone()],"é:1");assert!(d[1].important&&d[1].value.is_empty());let out=parse(&tokenize(b"p{color:red}",false));assert!(!out.source_complete&&!out.rules[0].supported);}
}
