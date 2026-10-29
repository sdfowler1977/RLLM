//! Original bounded inline page-program runtime over an owned source-order tree.
//! This implemented language/DOM subset is not ECMAScript or browser conformance.
use super::*;
use std::{collections::BTreeMap,rc::Rc,time::{Duration,Instant}};
use html_document::{Document,Kind,Node,Attribute,TextMode};
#[derive(Clone,Debug)]pub(super) enum Error{Input(String),Resource(String)}
impl Error{pub(super) fn message(self)->String{match self{Self::Input(v)|Self::Resource(v)=>v}}}
type PResult<T>=std::result::Result<T,Error>;
fn input(s:impl Into<String>)->Error{Error::Input(s.into())}
fn resource(s:impl Into<String>)->Error{Error::Resource(s.into())}
pub(super) fn enabled(args:&Json)->Result<bool>{match get(args,"scripts"){Null=>Ok(false),Str(v)if v.is_empty()||v=="static"=>Ok(false),Str(v)if v=="inline"=>Ok(true),_=>Err("Page scripts setting must be static or inline".into())}}
#[derive(Clone,Debug,PartialEq)]enum Token{Id(String),Number(f64),Text(String),Op(String),End}
fn line_terminator(c:char)->bool{matches!(c,'\n'|'\r'|'\u{2028}'|'\u{2029}')}
struct Lexer<'a>{source:&'a str,p:usize,tokens:Vec<Token>,line_before:Vec<bool>,line:bool}
impl<'a> Lexer<'a>{
 fn run(source:&'a str,limit:usize)->PResult<(Vec<Token>,Vec<bool>)>{let mut l=Self{source,p:0,tokens:Vec::new(),line_before:Vec::new(),line:false};while l.p<source.len(){l.one()?;if l.tokens.len()>limit.min(16384){return Err(resource("Page program token bound exceeded"));}}l.push(Token::End);Ok((l.tokens,l.line_before))}
 fn push(&mut self,token:Token){self.line_before.push(self.line);self.tokens.push(token);self.line=false;}
 fn one(&mut self)->PResult<()>{let b=self.source.as_bytes();let c=b[self.p];let ch=self.source[self.p..].chars().next().unwrap();if c.is_ascii_whitespace()||matches!(ch,'\u{2028}'|'\u{2029}'){self.p+=ch.len_utf8();self.line|=line_terminator(ch);return Ok(());}
  if b.get(self.p..self.p+2)==Some(b"//"){self.p+=2;while let Some(ch)=self.source[self.p..].chars().next(){if line_terminator(ch){break;}self.p+=ch.len_utf8();}return Ok(());}
  if b.get(self.p..self.p+2)==Some(b"/*"){self.p+=2;let n=self.source[self.p..].find("*/").ok_or_else(||input("Unterminated page program comment"))?;self.line|=self.source[self.p..self.p+n].chars().any(line_terminator);self.p+=n+2;return Ok(());}
  if c.is_ascii_alphabetic()||matches!(c,b'_'|b'$'){let start=self.p;self.p+=1;while b.get(self.p).is_some_and(|v|v.is_ascii_alphanumeric()||matches!(*v,b'_'|b'$')){self.p+=1;}let s=&self.source[start..self.p];if s.len()>128{return Err(resource("Page program identifier bound exceeded"));}self.push(Token::Id(s.into()));return Ok(());}
  if c.is_ascii_digit()||(c==b'.'&&b.get(self.p+1).is_some_and(u8::is_ascii_digit)){let start=self.p;while b.get(self.p).is_some_and(u8::is_ascii_digit){self.p+=1;}if b.get(self.p)==Some(&b'.'){self.p+=1;while b.get(self.p).is_some_and(u8::is_ascii_digit){self.p+=1;}}if b.get(self.p).is_some_and(|v|matches!(*v,b'e'|b'E')){self.p+=1;if b.get(self.p).is_some_and(|v|matches!(*v,b'+'|b'-')){self.p+=1;}let digits=self.p;while b.get(self.p).is_some_and(u8::is_ascii_digit){self.p+=1;}if digits==self.p{return Err(input("Invalid page program numeric exponent"));}}let v=self.source[start..self.p].parse::<f64>().map_err(|_|input("Invalid page program number"))?;if !v.is_finite(){return Err(input("Page program numeric literal exceeds finite range"));}self.push(Token::Number(v));return Ok(());}
  if matches!(c,b'\''|b'"'){self.p+=1;let mut out=String::new();loop{let ch=self.source[self.p..].chars().next().ok_or_else(||input("Unterminated page program string"))?;self.p+=ch.len_utf8();if ch as u32==c as u32{break;}if matches!(ch,'\n'|'\r'){return Err(input("Unescaped line break in page program string"));}if ch=='\\'{let e=self.source[self.p..].chars().next().ok_or_else(||input("Incomplete page program string escape"))?;self.p+=e.len_utf8();match e{'n'=>out.push('\n'),'r'=>out.push('\r'),'t'=>out.push('\t'),'b'=>out.push('\u{0008}'),'f'=>out.push('\u{000c}'),'\\'|'\''|'"'=>out.push(e),'u'=>{let hex=self.source.get(self.p..self.p+4).ok_or_else(||input("Incomplete page program Unicode escape"))?;let n=u32::from_str_radix(hex,16).map_err(|_|input("Invalid page program Unicode escape"))?;self.p+=4;if (0xd800..=0xdbff).contains(&n){if self.source.as_bytes().get(self.p..self.p+2)!=Some(b"\\u"){return Err(input("Unpaired page program Unicode surrogate"));}self.p+=2;let low=self.source.get(self.p..self.p+4).ok_or_else(||input("Incomplete page program surrogate"))?;let low=u32::from_str_radix(low,16).map_err(|_|input("Invalid page program surrogate"))?;self.p+=4;if !(0xdc00..=0xdfff).contains(&low){return Err(input("Invalid page program surrogate pair"));}out.push(char::from_u32(0x10000+((n-0xd800)<<10)+(low-0xdc00)).unwrap());}else{out.push(char::from_u32(n).ok_or_else(||input("Unpaired page program Unicode surrogate"))?);}},_=>return Err(input("Unsupported page program string escape"))}}else{out.push(ch);}if out.len()>16384{return Err(resource("Page program string bound exceeded"));}}self.push(Token::Text(out));return Ok(());}
  for op in ["===","!==","==","!=","<=",">=","&&","||","++","--","+=","-=","*=","/="]{if self.source[self.p..].starts_with(op){self.p+=op.len();self.push(Token::Op(op.into()));return Ok(());}}
  if b"{}();,.[]+-*/%<>=!?:".contains(&c){self.p+=1;self.push(Token::Op((c as char).to_string()));return Ok(());}Err(input(format!("Unsupported page program syntax at byte {}",self.p)))
 }
}
#[derive(Clone,Debug)]enum Expr{Literal(Value),Name(String),Unary(String,Box<Expr>),Binary(String,Box<Expr>,Box<Expr>),Assign(String,Box<Expr>,Box<Expr>),Member(Box<Expr>,String),Index(Box<Expr>,Box<Expr>),Array(Vec<Expr>),Object(Vec<(Expr,Expr)>),Call(Box<Expr>,Vec<Expr>),Update(Box<Expr>,i32,bool),Conditional(Box<Expr>,Box<Expr>,Box<Expr>)}
/// Every child was checked before attachment, so even rejected trees stay at
/// most one node above the depth bound and can be safely destroyed recursively.
fn bounded_expression(value:Expr)->PResult<Expr>{
 fn within(e:&Expr,depth:usize)->bool{if depth>64{return false;}match e{
  Expr::Literal(_)|Expr::Name(_)=>true,
  Expr::Unary(_,a)|Expr::Member(a,_)|Expr::Update(a,_,_)=>within(a,depth+1),
  Expr::Binary(_,a,b)|Expr::Assign(_,a,b)|Expr::Index(a,b)=>within(a,depth+1)&&within(b,depth+1),
  Expr::Array(items)=>items.iter().all(|e|within(e,depth+1)),Expr::Object(items)=>items.iter().all(|(k,v)|within(k,depth+1)&&within(v,depth+1)),
  Expr::Call(a,args)=>within(a,depth+1)&&args.iter().all(|v|within(v,depth+1)),
  Expr::Conditional(a,b,c)=>within(a,depth+1)&&within(b,depth+1)&&within(c,depth+1)
 }}
 if within(&value,1){Ok(value)}else{Err(resource("Page program expression-tree depth bound exceeded"))}
}
/// Conservative envelopes for compiler-owned allocations; distinct temporary
/// lexer storage is released from the reservation once its parser is dropped.
/// These are logical allocation bounds, not measured peak-RSS certification.
fn lexical_bytes(source:usize,tokens:usize)->PResult<usize>{
 source.min(tokens.saturating_add(1).min(16385)).checked_add(4).and_then(|n|n.checked_mul(2*(std::mem::size_of::<Token>()+std::mem::size_of::<bool>()))).and_then(|n|n.checked_add(source.saturating_mul(4))).and_then(|n|n.checked_add(2048)).ok_or_else(||resource("Page compilation size overflow"))
}
fn ast_bytes(tokens:usize,source:usize)->PResult<usize>{
 let node=2*(std::mem::size_of::<Expr>()+std::mem::size_of::<Stmt>())+128;
 tokens.checked_mul(node).and_then(|n|n.checked_add(source.saturating_mul(2))).and_then(|n|n.checked_add(2048)).ok_or_else(||resource("Page AST size overflow"))
}
#[derive(Clone,Debug)]enum Stmt{Empty,Block(Vec<Stmt>),Decl(String,bool,Option<Expr>),Expr(Expr),If(Expr,Box<Stmt>,Option<Box<Stmt>>),While(Expr,Box<Stmt>),For(Option<Box<Stmt>>,Option<Expr>,Option<Expr>,Box<Stmt>),Function(String,Rc<Function>),Return(Option<Expr>),Break,Continue}
#[derive(Clone,Debug)]struct Function{params:Vec<String>,body:Vec<Stmt>}
struct Parser{tokens:Vec<Token>,line_before:Vec<bool>,p:usize,nodes:usize,depth:usize,functions:usize,loops:usize,node_limit:usize}
impl Parser{
 fn parse(source:&str,room:usize,token_room:usize,node_room:usize)->PResult<(Vec<Stmt>,usize,usize,usize)>{
  let lexical=lexical_bytes(source.len(),token_room)?;if lexical>room{return Err(resource("Current memory allowance cannot admit page lexer storage"));}
  let (tokens,line_before)=Lexer::run(source,token_room)?;let used_tokens=tokens.len().saturating_sub(1);let retained=ast_bytes(tokens.len(),source.len())?;
  if lexical.checked_add(retained).filter(|n|*n<=room).is_none(){return Err(resource("Current memory allowance cannot admit retained page tokens and AST storage"));}
  let mut p=Self{tokens,line_before,p:0,nodes:0,depth:0,functions:0,loops:0,node_limit:node_room.min(8192)};let mut out=Vec::new();while p.peek()!=&Token::End{out.push(p.stmt(true)?);}Ok((out,retained,used_tokens,p.nodes))
 }
 fn peek(&self)->&Token{&self.tokens[self.p]}
 fn op(&mut self,s:&str)->bool{if self.peek()==&Token::Op(s.into()){self.p+=1;true}else{false}}
 fn keyword(&mut self,s:&str)->bool{if self.peek()==&Token::Id(s.into()){self.p+=1;true}else{false}}
 fn expect(&mut self,s:&str)->PResult<()>{if self.op(s){Ok(())}else{Err(input(format!("Expected {s} in page program")))}}
 fn name(&mut self)->PResult<String>{if let Token::Id(s)=self.peek().clone(){if ["document","true","false","null","undefined","if","else","while","for","let","const","function","return","break","continue","var","new","this","class","try","throw","switch","import","export","async","await","with"].contains(&s.as_str()){return Err(input("Reserved page program binding name"));}self.p+=1;Ok(s)}else{Err(input("Expected page program binding name"))}}
 fn node(&mut self)->PResult<()>{self.nodes+=1;if self.nodes>self.node_limit{return Err(resource("Page program aggregate AST node bound exceeded"));}Ok(())}
 fn enter(&mut self)->PResult<()>{self.node()?;self.depth+=1;if self.depth>64{return Err(resource("Page program AST/depth bound exceeded"));}Ok(())}
 fn semi(&mut self)->PResult<()>{if self.op(";")||self.peek()==&Token::Op("}".into())||self.peek()==&Token::End{Ok(())}else{Err(input("This page program subset requires an explicit statement semicolon"))}}
 fn stmt(&mut self,top:bool)->PResult<Stmt>{self.enter()?;let r=self.statement(top);self.depth-=1;r}
 fn block(&mut self)->PResult<Vec<Stmt>>{self.expect("{")?;let mut body=Vec::new();while !self.op("}"){if self.peek()==&Token::End{return Err(input("Unterminated page program block"));}body.push(self.stmt(false)?);}Ok(body)}
 fn statement(&mut self,top:bool)->PResult<Stmt>{
  if self.op(";"){return Ok(Stmt::Empty);}if self.peek()==&Token::Op("{".into()){return Ok(Stmt::Block(self.block()?));}
  let constant=if self.keyword("const"){Some(true)}else if self.keyword("let"){Some(false)}else{None};if let Some(c)=constant{let n=self.name()?;let v=if self.op("="){Some(self.expr(0)?)}else{None};if c&&v.is_none(){return Err(input("Page program const requires an initializer"));}self.semi()?;return Ok(Stmt::Decl(n,c,v));}
  if self.keyword("function"){if !top{return Err(input("Nested page function declarations are not implemented"));}let name=self.name()?;self.expect("(")?;let mut params=Vec::new();if !self.op(")"){loop{let n=self.name()?;if params.contains(&n)||params.len()>=16{return Err(input("Duplicate or excessive page function parameters"));}params.push(n);if self.op(")"){break;}self.expect(",")?;}}self.functions+=1;let old=self.loops;self.loops=0;let body=self.block()?;self.loops=old;self.functions-=1;return Ok(Stmt::Function(name,Rc::new(Function{params,body})));}
  if self.keyword("if"){self.expect("(")?;let c=self.expr(0)?;self.expect(")")?;let a=self.stmt(false)?;let b=if self.keyword("else"){Some(Box::new(self.stmt(false)?))}else{None};return Ok(Stmt::If(c,Box::new(a),b));}
  if self.keyword("while"){self.expect("(")?;let c=self.expr(0)?;self.expect(")")?;self.loops+=1;let body=self.stmt(false)?;self.loops-=1;return Ok(Stmt::While(c,Box::new(body)));}
  if self.keyword("for"){self.expect("(")?;let init=if self.op(";"){None}else if self.peek()==&Token::Id("let".into())||self.peek()==&Token::Id("const".into()){Some(Box::new(self.stmt(false)?))}else{let e=self.expr(0)?;self.expect(";")?;self.node()?;Some(Box::new(Stmt::Expr(e)))};let c=if self.op(";"){None}else{let e=self.expr(0)?;self.expect(";")?;Some(e)};let u=if self.op(")"){None}else{let e=self.expr(0)?;self.expect(")")?;Some(e)};self.loops+=1;let body=self.stmt(false)?;self.loops-=1;return Ok(Stmt::For(init,c,u,Box::new(body)));}
  if self.keyword("return"){if self.functions==0{return Err(input("Page return is outside a function"));}let newline=self.line_before.get(self.p).copied().unwrap_or(false);let value=if newline||self.peek()==&Token::Op(";".into())||self.peek()==&Token::Op("}".into()){None}else{Some(self.expr(0)?)};if !newline{self.semi()?;}return Ok(Stmt::Return(value));}
  if self.keyword("break"){if self.loops==0{return Err(input("Page break is outside a loop"));}self.semi()?;return Ok(Stmt::Break);}if self.keyword("continue"){if self.loops==0{return Err(input("Page continue is outside a loop"));}self.semi()?;return Ok(Stmt::Continue);}
  if let Token::Id(n)=self.peek(){if ["var","new","this","class","try","throw","switch","import","export","async","await","with","do","delete","typeof","void","yield"].contains(&n.as_str()){return Err(input(format!("Page program language feature {n} is not implemented")));}}
  let e=self.expr(0)?;self.semi()?;Ok(Stmt::Expr(e))
 }
 fn expr(&mut self,min:u8)->PResult<Expr>{self.enter()?;let result=self.expression(min);self.depth-=1;result}
 fn expression(&mut self,min:u8)->PResult<Expr>{let mut left=if self.op("!"){Expr::Unary("!".into(),Box::new(self.expr(13)?))}else if self.op("-"){Expr::Unary("-".into(),Box::new(self.expr(13)?))}else if self.op("+"){Expr::Unary("+".into(),Box::new(self.expr(13)?))}else if self.op("++"){Expr::Update(Box::new(self.expr(13)?),1,true)}else if self.op("--"){Expr::Update(Box::new(self.expr(13)?),-1,true)}else if self.op("["){let mut items=Vec::new();if !self.op("]"){loop{if items.len()>=512{return Err(resource("Page array literal element bound exceeded"));}if self.peek()==&Token::Op(",".into()){return Err(input("Array literal elisions are not implemented"));}items.push(self.expr(0)?);if self.op("]"){break;}self.expect(",")?;if self.op("]"){break;}}}Expr::Array(items)}else if self.op("{"){let mut items=Vec::new();if !self.op("}"){loop{if items.len()>=512{return Err(resource("Page object literal property bound exceeded"));}let (key,shorthand)=if self.op("["){let key=self.expr(0)?;self.expect("]")?;self.expect(":")?;(key,None)}else{let (key,short)=match self.peek().clone(){Token::Id(n)=>{self.p+=1;(n.clone(),Some(n))},Token::Text(t)=>{self.p+=1;(t,None)},Token::Number(n)=>{self.p+=1;(number_text(n),None)},_=>return Err(input("Object literal spread, accessors and methods are not implemented"))};if self.op(":"){if key=="__proto__"{return Err(input("Object literal prototype setters are not implemented"));}self.node()?;(Expr::Literal(Value::Text(key)),None)}else{let n=short.ok_or_else(||input("Object literal property requires a colon"))?;self.node()?;(Expr::Literal(Value::Text(key)),Some(n))}};let value=if let Some(n)=shorthand{self.node()?;Expr::Name(n)}else{self.expr(0)?};items.push((key,value));if self.op("}"){break;}self.expect(",")?;if self.op("}"){break;}}}Expr::Object(items)}else if self.op("("){let e=self.expr(0)?;self.expect(")")?;e}else{match self.peek().clone(){Token::Number(n)=>{self.p+=1;Expr::Literal(Value::Number(n))},Token::Text(s)=>{self.p+=1;Expr::Literal(Value::Text(s))},Token::Id(n)=>{self.p+=1;match n.as_str(){"true"=>Expr::Literal(Value::Bool(true)),"false"=>Expr::Literal(Value::Bool(false)),"null"=>Expr::Literal(Value::Null),"undefined"=>Expr::Literal(Value::Undefined),_=>Expr::Name(n)}},_=>return Err(input("Expected a page program expression"))}};
  left=bounded_expression(left)?;
  loop{
   if min<=15&&self.op("."){let n=if let Token::Id(n)=self.peek().clone(){self.p+=1;n}else{return Err(input("Expected page property name"));};self.node()?;left=bounded_expression(Expr::Member(Box::new(left),n))?;continue;}
   if min<=15&&self.op("["){let key=self.expr(0)?;self.expect("]")?;self.node()?;left=bounded_expression(Expr::Index(Box::new(left),Box::new(key)))?;continue;}
   if min<=15&&self.op("("){let mut args=Vec::new();if !self.op(")"){loop{if args.len()>=16{return Err(resource("Page call argument bound exceeded"));}args.push(self.expr(0)?);if self.op(")"){break;}self.expect(",")?;}}self.node()?;left=bounded_expression(Expr::Call(Box::new(left),args))?;continue;}
   if min<=14&&(self.peek()==&Token::Op("++".into())||self.peek()==&Token::Op("--".into())){let n=if self.op("++"){1}else{self.expect("--")?;-1};self.node()?;left=bounded_expression(Expr::Update(Box::new(left),n,false))?;continue;}
   if min<=2&&self.op("?"){let a=self.expr(0)?;self.expect(":")?;let b=self.expr(2)?;self.node()?;left=bounded_expression(Expr::Conditional(Box::new(left),Box::new(a),Box::new(b)))?;continue;}
   let op=match self.peek(){Token::Op(v)=>v.clone(),_=>break};let prec=match op.as_str(){"="|"+="|"-="|"*="|"/="=>1,"||"=>3,"&&"=>4,"=="|"!="|"==="|"!=="=>7,"<"|">"|"<="|">="=>8,"+"|"-"=>10,"*"|"/"|"%"=>11,_=>break};if prec<min{break;}self.p+=1;let rhs=self.expr(if prec==1{prec}else{prec+1})?;
   if prec==1{if !matches!(left,Expr::Name(_)|Expr::Member(_,_)|Expr::Index(_,_)){return Err(input("Invalid page assignment target"));}self.node()?;left=bounded_expression(Expr::Assign(op,Box::new(left),Box::new(rhs)))?;}else{self.node()?;left=bounded_expression(Expr::Binary(op,Box::new(left),Box::new(rhs)))?;}
  }Ok(left)
 }
}
#[derive(Clone,Debug)]enum Value{Undefined,Null,Bool(bool),Number(f64),Text(String),Document,Node(usize),Style(usize),Function(Rc<Function>),Builtin(&'static str),Heap(usize)}
/// Decimal digits come from the standard formatter's shortest round-trip
/// representation; this layer applies the language's notation thresholds.
fn number_text(n:f64)->String{
 if n.is_nan(){return "NaN".into();}if n==f64::INFINITY{return "Infinity".into();}if n==f64::NEG_INFINITY{return "-Infinity".into();}if n==0.0{return "0".into();}
 if n.abs()>=1e-6&&n.abs()<1e21{return n.to_string();}
 let raw=format!("{n:e}");let (digits,exponent)=raw.split_once('e').expect("finite lower-exponent number");let exponent: i32=exponent.parse().expect("bounded floating exponent");
 format!("{digits}e{}{exponent}",if exponent>=0{"+"}else{""})
}
// StringNumericLiteral accepts language whitespace rather than Rust's broader
// Unicode trim set. Radix integers are rounded from significant bits with a
// guard/sticky bit, without allocating an input-sized integer or digit buffer.
fn numeric_text(source:&str)->f64{
 fn space(c:char)->bool{matches!(c,'\u{0009}'|'\u{000b}'|'\u{000c}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}'|'\n'|'\r'|'\u{2028}'|'\u{2029}')}
 let s=source.trim_matches(space);if s.is_empty(){return 0.0;}if s=="Infinity"||s=="+Infinity"{return f64::INFINITY;}if s=="-Infinity"{return f64::NEG_INFINITY;}
 let b=s.as_bytes();let radix=if b.len()>=2&&b[0]==b'0'{match b[1]{b'x'|b'X'=>Some((16u8,4u8)),b'o'|b'O'=>Some((8,3)),b'b'|b'B'=>Some((2,1)),_=>None}}else{None};
 if let Some((radix,width))=radix{if b.len()==2{return f64::NAN;}let mut significant=0u64;let mut bits=0usize;let mut guard=false;let mut sticky=false;
  for c in &b[2..]{let digit=match c{b'0'..=b'9'=>*c-b'0',b'a'..=b'f'=>*c-b'a'+10,b'A'..=b'F'=>*c-b'A'+10,_=>return f64::NAN};if digit>=radix{return f64::NAN;}for shift in (0..width).rev(){let bit=digit>>shift&1;if bits==0&&bit==0{continue;}bits+=1;if bits<=53{significant=(significant<<1)|bit as u64;}else if bits==54{guard=bit!=0;}else{sticky|=bit!=0;}}}
  if bits<=53{return significant as f64;}if guard&&(sticky||significant&1!=0){significant+=1;}if bits>1024{return f64::INFINITY;}return significant as f64*2.0f64.powi((bits-53)as i32);
 }
 if !b.iter().all(|c|c.is_ascii_digit()||matches!(c,b'+'|b'-'|b'.'|b'e'|b'E')){return f64::NAN;}s.parse::<f64>().unwrap_or(f64::NAN)
}
impl Value{
 fn text(&self)->String{match self{Self::Undefined=>"undefined".into(),Self::Null=>"null".into(),Self::Bool(v)=>v.to_string(),Self::Number(n)=>number_text(*n),Self::Text(s)=>s.clone(),Self::Document=>"[object Document]".into(),Self::Node(_)=>"[object Element]".into(),Self::Style(_)=>"[object CSSStyleDeclaration]".into(),Self::Function(_)|Self::Builtin(_)=>"[page function]".into(),Self::Heap(_)=>unreachable!("heap conversion uses admitted runtime traversal")}}
 fn number(&self)->f64{match self{Self::Number(n)=>*n,Self::Null=>0.0,Self::Bool(v)=>if *v{1.0}else{0.0},Self::Text(s)=>numeric_text(s),_=>f64::NAN}}
 fn truth(&self)->bool{match self{Self::Undefined|Self::Null=>false,Self::Bool(v)=>*v,Self::Number(n)=>*n!=0.0&&!n.is_nan(),Self::Text(s)=>!s.is_empty(),_=>true}}
}
// Public document references are canonical across DOM navigation.
fn node_value(n:usize)->Value{if n==0{Value::Document}else{Value::Node(n)}}
// Values refer to an invocation-owned arena by index, so self/cross references
// cannot create Rc cycles or recursive destruction. All entries die together.
struct HeapObject{array:bool,length:u32,properties:BTreeMap<String,Value>}
fn array_index(k:&str)->Option<u32>{if k.is_empty()||(k.len()>1&&k.starts_with('0'))||!k.bytes().all(|c|c.is_ascii_digit()){return None;}let n=k.parse::<u32>().ok()?;if n<u32::MAX{Some(n)}else{None}}
fn inherited_property(k:&str)->bool{["__proto__","constructor","toString","toLocaleString","valueOf","hasOwnProperty","isPrototypeOf","propertyIsEnumerable","__defineGetter__","__defineSetter__","__lookupGetter__","__lookupSetter__"].contains(&k)}
#[derive(Clone)]struct Binding{value:Value,constant:bool,initialized:bool}
enum Flow{Next,Return(Value),Break,Continue}
enum Reference{Name(String),Member(Value,String)}
struct Runtime<'a>{doc:Document,objects:Vec<HeapObject>,scopes:Vec<BTreeMap<String,Binding>>,functions:BTreeMap<String,Rc<Function>>,fuel:usize,heap:usize,heap_limit:usize,bindings:usize,calls:usize,deadline:Instant,cancelled:&'a dyn Fn()->bool,mutations:usize,frames:usize}
impl<'a> Runtime<'a>{
 fn tick(&mut self)->PResult<()>{if self.fuel==0{return Err(resource("Page program instruction budget exhausted"));}self.fuel-=1;if (self.cancelled)(){return Err(resource("Page program cancelled"));}if Instant::now()>=self.deadline{return Err(resource("Page program deadline expired"));}Ok(())}
 fn charge(&mut self,n:usize)->PResult<()>{self.tick()?;self.heap=self.heap.checked_add(n).ok_or_else(||resource("Page program heap count overflow"))?;if self.heap>self.heap_limit{return Err(resource("Page program allocation budget exhausted"));}Ok(())}
 fn string(&mut self,s:String)->PResult<Value>{if s.len()>16384{return Err(resource("Page program string value bound exceeded"));}self.charge(s.len()+32)?;Ok(Value::Text(s))}
 fn copy_value(&mut self,v:&Value)->PResult<Value>{if let Value::Text(s)=v{self.charge(s.len()+32)?;}Ok(v.clone())}
 fn new_object(&mut self,array:bool)->PResult<usize>{if self.objects.len()>=512{return Err(resource("Page object arena bound exceeded"));}self.charge(4*std::mem::size_of::<HeapObject>()+128)?;let id=self.objects.len();self.objects.push(HeapObject{array,length:0,properties:BTreeMap::new()});Ok(id)}
 fn key(&mut self,v:&Value)->PResult<String>{let k=self.text(v)?;if k.len()>16384{return Err(resource("Page property key bound exceeded"));}Ok(k)}
 fn own(&mut self,id:usize,k:&str)->PResult<Option<Value>>{self.tick()?;if let Some(Value::Text(s))=self.objects[id].properties.get(k){let n=s.len();self.charge(n+32)?;}Ok(self.objects[id].properties.get(k).cloned())}
 fn heap_set(&mut self,id:usize,k:String,v:Value,define:bool)->PResult<()>{self.tick()?;
  if self.objects[id].array&&k=="length"{let n=self.number(&v)?;if !n.is_finite()||n<0.0||n>u32::MAX as f64||n.fract()!=0.0{return Err(input("Invalid page array length"));}let n=n as u32;let count=self.objects[id].properties.len();for _ in 0..count{self.tick()?;}self.objects[id].properties.retain(|k,_|array_index(k).is_none_or(|i|i<n));self.objects[id].length=n;return Ok(());}
  if !define&&k=="__proto__"&&!self.objects[id].properties.contains_key(&k){return Err(input("Page inherited prototype setters are not implemented"));}
  if !self.objects[id].properties.contains_key(&k){if self.objects[id].properties.len()>=512{return Err(resource("Page own-property bound exceeded"));}self.charge(k.len()+16*(std::mem::size_of::<Value>()+std::mem::size_of::<String>())+128)?;}
  if let Value::Text(t)=&v{self.charge(t.len()+32)?;}
  if self.objects[id].array{if let Some(i)=array_index(&k){self.objects[id].length=self.objects[id].length.max(i+1);}}
  self.objects[id].properties.insert(k,v);Ok(())
 }
 fn append_text(&mut self,out:&mut String,s:&str)->PResult<()>{let n=out.len().checked_add(s.len()).filter(|n|*n<=16384).ok_or_else(||resource("Page converted string bound exceeded"))?;self.charge(s.len()*2+32)?;out.reserve(n-out.len());out.push_str(s);Ok(())}
 fn text(&mut self,v:&Value)->PResult<String>{self.convert_text(v,&mut Vec::new())}
 fn convert_text(&mut self,v:&Value,path:&mut Vec<usize>)->PResult<String>{self.enter_frame()?;let r=(||{self.tick()?;if !path.is_empty()&&matches!(v,Value::Function(_)|Value::Builtin(_)){return Err(input("Function elements in page array string conversion are not implemented"));}if let Value::Heap(id)=v{if self.objects[*id].properties.contains_key("toString")||self.objects[*id].properties.contains_key("valueOf"){return Err(input("Page custom object conversion methods are not implemented"));}if self.objects[*id].array{if self.objects[*id].properties.contains_key("join"){return Err(input("Page array conversion with an own join override is not implemented"));}return self.join_array(*id,",",path);}self.charge(64)?;Ok("[object Object]".into())}else{let n=if let Value::Text(t)=v{t.len()}else{64};self.charge(n+32)?;Ok(v.text())}})();self.frames-=1;r}
 fn join_array(&mut self,id:usize,separator:&str,path:&mut Vec<usize>)->PResult<String>{if path.contains(&id){return Err(input("Cyclic page array string conversion is not implemented"));}if path.len()>=64{return Err(resource("Page conversion nesting bound exceeded"));}self.charge(2*std::mem::size_of::<usize>()+32)?;path.push(id);let r=(||{let length=self.objects[id].length;let mut out=String::new();for i in 0..length{self.tick()?;if i>0{self.append_text(&mut out,separator)?;}self.charge(64)?;let key=i.to_string();let v=self.own(id,&key)?.unwrap_or(Value::Undefined);if !matches!(v,Value::Null|Value::Undefined){let text=self.convert_text(&v,path)?;self.append_text(&mut out,&text)?;}}Ok(out)})();path.pop();r}
 fn primitive(&mut self,v:Value)->PResult<Value>{if matches!(v,Value::Heap(_)){let text=self.text(&v)?;self.string(text)}else{Ok(v)}}
 fn number(&mut self,v:&Value)->PResult<f64>{let copy=self.copy_value(v)?;let v=self.primitive(copy)?;if let Value::Text(t)=&v{for _ in 0..t.len().saturating_mul(8){self.tick()?;}}Ok(v.number())}
 fn name(&mut self,n:&str)->PResult<Value>{self.tick()?;if n=="document"{return Ok(Value::Document);}let mut value=None;for scope in self.scopes.iter().rev(){if let Some(b)=scope.get(n){if !b.initialized{return Err(input("Page lexical binding accessed before initialization"));}value=Some(b.value.clone());break;}}if let Some(v)=value{if let Value::Text(s)=&v{self.charge(s.len()+32)?;}return Ok(v);}if let Some(f)=self.functions.get(n){return Ok(Value::Function(f.clone()));}for builtin in ["String","Number","Boolean"]{if n==builtin{return Ok(Value::Builtin(builtin));}}Err(input(format!("Page program identifier {n} is unavailable")))}
 fn set_name(&mut self,n:&str,v:Value)->PResult<()>{self.tick()?;for scope in self.scopes.iter_mut().rev(){if let Some(b)=scope.get_mut(n){if !b.initialized{return Err(input("Page lexical binding assigned before initialization"));}if b.constant{return Err(input("Page program cannot assign a const binding"));}b.value=v;return Ok(());}}Err(input("Page assignment requires an existing binding"))}
 fn enter_frame(&mut self)->PResult<()>{if self.frames>=64{return Err(resource("Page combined execution-frame bound exceeded"));}self.frames+=1;Ok(())}
 fn stmt(&mut self,s:&Stmt)->PResult<Flow>{self.enter_frame()?;let result=self.stmt_inner(s);self.frames-=1;result}
 fn stmt_inner(&mut self,s:&Stmt)->PResult<Flow>{self.tick()?;match s{
  Stmt::Empty|Stmt::Function(_,_)=>Ok(Flow::Next),Stmt::Expr(e)=>{self.eval(e)?;Ok(Flow::Next)},Stmt::Decl(n,c,e)=>{if self.scopes.last().unwrap().get(n).is_some_and(|b|b.initialized){return Err(input("Duplicate page lexical binding"));}if !self.scopes.last().unwrap().contains_key(n){self.declare(n,*c)?;}let v=if let Some(e)=e{self.eval(e)?}else{Value::Undefined};let b=self.scopes.last_mut().unwrap().get_mut(n).unwrap();b.value=v;b.initialized=true;Ok(Flow::Next)},
  Stmt::Block(body)=>{self.scopes.push(BTreeMap::new());let r=self.body(body);self.pop_scope()?;r},Stmt::If(c,a,b)=>{if self.eval(c)?.truth(){self.stmt(a)}else if let Some(b)=b{self.stmt(b)}else{Ok(Flow::Next)}},
  Stmt::While(c,b)=>{while self.eval(c)?.truth(){match self.stmt(b)?{Flow::Break=>break,Flow::Return(v)=>return Ok(Flow::Return(v)),_=>{}}}Ok(Flow::Next)},
  Stmt::For(init,c,u,b)=>{self.scopes.push(BTreeMap::new());let r=(||{if let Some(i)=init{self.stmt(i)?;}loop{self.tick()?;if let Some(c)=c{if !self.eval(c)?.truth(){break;}}match self.stmt(b)?{Flow::Break=>break,Flow::Return(v)=>return Ok(Flow::Return(v)),_=>{}}if let Some(u)=u{self.eval(u)?;}}Ok(Flow::Next)})();self.pop_scope()?;r},
  Stmt::Return(v)=>Ok(Flow::Return(if let Some(v)=v{self.eval(v)?}else{Value::Undefined})),Stmt::Break=>Ok(Flow::Break),Stmt::Continue=>Ok(Flow::Continue)
 }}
 // Only scope-owned binding/key/container reservations are reclaimed here.
 // Values and DOM storage retain their separate conservative charges.
 fn binding_bytes(n:&str)->usize{n.len()+16*(std::mem::size_of::<Binding>()+std::mem::size_of::<String>())+128}
 fn declare(&mut self,n:&str,c:bool)->PResult<()>{
  if self.scopes.last().unwrap().contains_key(n){return Err(input("Duplicate page lexical binding"));}
  if self.bindings>=512{return Err(resource("Page program live binding bound exceeded"));}
  self.charge(Self::binding_bytes(n))?;
  self.scopes.last_mut().unwrap().insert(n.into(),Binding{value:Value::Undefined,constant:c,initialized:false});self.bindings+=1;Ok(())
 }
 fn pop_scope(&mut self)->PResult<()>{
  if self.scopes.len()<=1{return Err(resource("Page scope ownership mismatch"));}
  let scope=self.scopes.pop().unwrap();let count=scope.len();let released=scope.keys().map(|n|Self::binding_bytes(n)).sum::<usize>();
  // Release only after the owning tree and its keys have actually been dropped.
  drop(scope);
  self.heap=self.heap.checked_sub(released).ok_or_else(||resource("Page scope allocation ownership mismatch"))?;
  self.bindings=self.bindings.checked_sub(count).ok_or_else(||resource("Page scope binding ownership mismatch"))?;Ok(())
 }
 fn body(&mut self,body:&[Stmt])->PResult<Flow>{for stmt in body{if let Stmt::Decl(n,c,_)=stmt{self.declare(n,*c)?;}}for s in body{let r=self.stmt(s)?;if !matches!(r,Flow::Next){return Ok(r);}}Ok(Flow::Next)}
 fn eval(&mut self,e:&Expr)->PResult<Value>{self.enter_frame()?;let result=self.eval_inner(e);self.frames-=1;result}
 fn eval_inner(&mut self,e:&Expr)->PResult<Value>{self.tick()?;match e{
  Expr::Literal(v)=>{if let Value::Text(s)=v{self.charge(s.len()+32)?;}Ok(v.clone())},Expr::Name(n)=>self.name(n),Expr::Member(a,k)=>{let v=self.eval(a)?;self.member(v,k)},Expr::Index(a,k)=>{let v=self.eval(a)?;let key=self.eval(k)?;let key=self.key(&key)?;self.member(v,&key)},
  Expr::Array(items)=>{let id=self.new_object(true)?;for(i,e)in items.iter().enumerate(){let v=self.eval(e)?;self.charge(64)?;self.heap_set(id,i.to_string(),v,true)?;}Ok(Value::Heap(id))},Expr::Object(items)=>{let id=self.new_object(false)?;for(k,e)in items{let key=self.eval(k)?;let key=self.key(&key)?;let value=self.eval(e)?;self.heap_set(id,key,value,true)?;}Ok(Value::Heap(id))},
  Expr::Unary(op,e)=>{let v=self.eval(e)?;Ok(match op.as_str(){"!"=>Value::Bool(!v.truth()),"-"=>Value::Number(-self.number(&v)?),"+"=>Value::Number(self.number(&v)?),_=>return Err(input("Unsupported unary operator"))})},
  Expr::Conditional(c,a,b)=>if self.eval(c)?.truth(){self.eval(a)}else{self.eval(b)},
  Expr::Binary(op,a,b)=>{let a=self.eval(a)?;if op=="&&"&&!a.truth()||op=="||"&&a.truth(){return Ok(a);}let b=self.eval(b)?;self.binary(op,a,b)},
  Expr::Assign(op,a,b)=>{let target=self.reference(a)?;let previous=if op=="="{None}else{Some(self.reference_value(&target)?)};let v=self.eval(b)?;let v=if let Some(previous)=previous{self.binary(&op[..1],previous,v)?}else{v};if let Value::Text(text)=&v{self.charge(text.len()+32)?;}self.assign_reference(target,v.clone())?;Ok(v)},
  Expr::Update(a,d,pre)=>{let target=self.reference(a)?;let previous=self.reference_value(&target)?;let old=self.number(&previous)?;let next=Value::Number(old+*d as f64);self.assign_reference(target,next.clone())?;Ok(if *pre{next}else{Value::Number(old)})},
  Expr::Call(target,args)=>{
   let member=if let Expr::Member(base,key)=target.as_ref(){Some((self.eval(base)?,key.clone()))}else if let Expr::Index(base,key)=target.as_ref(){let base=self.eval(base)?;let key=self.eval(key)?;Some((base,self.key(&key)?))}else{None};let function=if member.is_none(){Some(self.eval(target)?)}else{None};if let Some((Value::Heap(id),key))=&member{if self.objects[*id].properties.contains_key(key){return Err(input("Calling page own data properties as methods is not implemented"));}if !["toString","valueOf"].contains(&key.as_str())&&!(self.objects[*id].array&&["join","push","pop"].contains(&key.as_str())){return Err(input("This page array/object method is not implemented"));}}self.charge(2*args.len()*std::mem::size_of::<Value>()+32)?;let mut values=Vec::new();for a in args{values.push(self.eval(a)?);}if let Some((base,key))=member{return self.method(base,&key,values);}
   if let Some(Value::Builtin(n))=function{if values.len()>1{return Err(input("Page conversion accepts at most one argument"));}let v=values.first().cloned().unwrap_or(Value::Undefined);return match n{"String"=>{let text=if values.is_empty(){String::new()}else{self.text(&v)?};self.string(text)},"Number"=>Ok(Value::Number(if values.is_empty(){0.0}else{self.number(&v)?})),_=>Ok(Value::Bool(v.truth()))};}
   let Some(Value::Function(f))=function else{return Err(input("Page value is not callable"));};if self.calls>=32{return Err(resource("Page function call-stack bound exceeded"));}let globals=std::mem::take(&mut self.scopes[0]);let old=std::mem::replace(&mut self.scopes,vec![globals,BTreeMap::new()]);self.calls+=1;let result=(||{for(i,n)in f.params.iter().enumerate(){self.declare(n,false)?;if let Some(Value::Text(text))=values.get(i){self.charge(text.len()+32)?;}let b=self.scopes.last_mut().unwrap().get_mut(n).unwrap();b.value=values.get(i).cloned().unwrap_or(Value::Undefined);b.initialized=true;}self.body(&f.body)})();while self.scopes.len()>1{self.pop_scope()?;}let globals=self.scopes.pop().unwrap();self.scopes=old;self.scopes[0]=globals;self.calls-=1;match result?{Flow::Return(v)=>Ok(v),_=>Ok(Value::Undefined)}
  }
 }}
 fn strict_equal(a:&Value,b:&Value)->bool{match(a,b){(Value::Undefined,Value::Undefined)|(Value::Null,Value::Null)=>true,(Value::Bool(a),Value::Bool(b))=>a==b,(Value::Number(a),Value::Number(b))=>a==b,(Value::Text(a),Value::Text(b))=>a==b,(Value::Heap(a),Value::Heap(b))|(Value::Node(a),Value::Node(b))|(Value::Style(a),Value::Style(b))=>a==b,(Value::Document,Value::Document)=>true,(Value::Function(a),Value::Function(b))=>Rc::ptr_eq(a,b),(Value::Builtin(a),Value::Builtin(b))=>a==b,_=>false}}
 fn loose_equal(&mut self,a:Value,b:Value)->PResult<bool>{self.tick()?;if Self::strict_equal(&a,&b){return Ok(true);}if matches!((&a,&b),(Value::Null,Value::Undefined)|(Value::Undefined,Value::Null)){return Ok(true);}if matches!(&a,Value::Null|Value::Undefined)||matches!(&b,Value::Null|Value::Undefined){return Ok(false);}let object=|v:&Value|matches!(v,Value::Heap(_)|Value::Document|Value::Node(_)|Value::Style(_)|Value::Function(_)|Value::Builtin(_));if object(&a)&&object(&b){return Ok(false);}if let Value::Bool(v)=a{return self.loose_equal(Value::Number(if v{1.0}else{0.0}),b);}if let Value::Bool(v)=b{return self.loose_equal(a,Value::Number(if v{1.0}else{0.0}));}if matches!(a,Value::Heap(_)){let a=self.primitive(a)?;return self.loose_equal(a,b);}if matches!(b,Value::Heap(_)){let b=self.primitive(b)?;return self.loose_equal(a,b);}Ok(matches!((&a,&b),(Value::Text(_),Value::Number(_))|(Value::Number(_),Value::Text(_)))&&self.number(&a)?==self.number(&b)?)}
 fn binary(&mut self,op:&str,a:Value,b:Value)->PResult<Value>{self.tick()?;
  if matches!(op,"==="|"!=="){let eq=Self::strict_equal(&a,&b);return Ok(Value::Bool(if op=="==="{eq}else{!eq}));}if matches!(op,"=="|"!="){let eq=self.loose_equal(a,b)?;return Ok(Value::Bool(if op=="=="{eq}else{!eq}));}if matches!(op,"&&"|"||"){return Ok(b);}
  let a=self.primitive(a)?;let b=self.primitive(b)?;Ok(match op{
   "+" if matches!(a,Value::Text(_))||matches!(b,Value::Text(_))=>{let mut out=self.text(&a)?;let tail=self.text(&b)?;self.append_text(&mut out,&tail)?;return self.string(out)},"+"=>Value::Number(self.number(&a)?+self.number(&b)?),"-"=>Value::Number(self.number(&a)?-self.number(&b)?),"*"=>Value::Number(self.number(&a)?*self.number(&b)?),"/"=>Value::Number(self.number(&a)?/self.number(&b)?),"%"=>Value::Number(self.number(&a)?%self.number(&b)?),
   "<"|">"|"<="|">="=>{let yes=if let(Value::Text(x),Value::Text(y))=(&a,&b){let order=x.encode_utf16().cmp(y.encode_utf16());match op{"<"=>order.is_lt(),">"=>order.is_gt(),"<="=>!order.is_gt(),_=>!order.is_lt()}}else{let(x,y)=(self.number(&a)?,self.number(&b)?);match op{"<"=>x<y,">"=>x>y,"<="=>x<=y,_=>x>=y}};Value::Bool(yes)},_=>return Err(input("Unsupported page binary operator"))})}
 fn attr(&self,n:usize,k:&str)->Option<String>{match &self.doc.nodes[n].kind{Kind::Element{attributes,..}=>attributes.iter().find(|a|a.name==k).and_then(|a|a.value.clone()),_=>None}}
 fn set_attr(&mut self,n:usize,k:&str,v:String)->PResult<()>{if !["id","class","title","style","width","height","hidden"].contains(&k){return Err(input("This page DOM subset cannot change URL, executable or unsupported attributes"));}self.charge(4*std::mem::size_of::<Attribute>()+128+v.len()+k.len())?;let Kind::Element{attributes,..}=&mut self.doc.nodes[n].kind else{return Err(input("Page attribute target is not an element"));};if let Some(a)=attributes.iter_mut().find(|a|a.name==k){a.value=Some(v);}else{if attributes.len()>=32{return Err(resource("Page element attribute bound exceeded"));}attributes.push(Attribute{name:k.into(),value:Some(v),span:0..0,value_span:None});}self.mutations+=1;Ok(())}
 fn is_template(&self,n:usize)->bool{matches!(&self.doc.nodes[n].kind,Kind::Element{name,..} if name=="template")}
 fn reachable(&mut self)->PResult<Vec<usize>>{let mut out=Vec::new();let mut stack=vec![0];while let Some(n)=stack.pop(){self.tick()?;out.push(n);if self.is_template(n){continue;}for c in self.doc.nodes[n].children.iter().rev(){stack.push(*c);}}Ok(out)}
 fn lookup(&mut self,selector:&str,id_only:bool)->PResult<Value>{if selector.len()>256{return Err(resource("Page selector bound exceeded"));}let selector=if id_only{selector}else{selector.trim()};if selector.is_empty(){return if id_only{Ok(Value::Null)}else{Err(input("Page selector is empty"))};}let(kind,name)=if id_only{('i',selector)}else if let Some(v)=selector.strip_prefix('#'){('i',v)}else if let Some(v)=selector.strip_prefix('.') {('c',v)}else{('t',selector)};if !id_only&&(name.is_empty()||!name.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'))){return Err(input("Page querySelector supports one tag, id or class selector in this subset"));}for n in self.reachable()?{if let Kind::Element{name:tag,..}=&self.doc.nodes[n].kind{let yes=match kind{'i'=>self.attr(n,"id").as_deref()==Some(name),'c'=>self.attr(n,"class").is_some_and(|v|v.split_ascii_whitespace().any(|v|v==name)),_=>tag==name};if yes{return Ok(Value::Node(n));}}}Ok(Value::Null)}
 fn content(&mut self,n:usize)->PResult<String>{if self.is_template(n){return Ok(String::new());}if let Kind::Comment{data}=&self.doc.nodes[n].kind{return Ok(data.clone());}if matches!(self.doc.nodes[n].kind,Kind::Document){return Ok(String::new());}let mut stack=vec![n];let mut s=String::new();while let Some(i)=stack.pop(){self.tick()?;if self.is_template(i){continue;}if let Kind::Text{data,..}=&self.doc.nodes[i].kind{s.push_str(data);if s.len()>16384{return Err(resource("Page textContent value bound exceeded"));}}else{for c in self.doc.nodes[i].children.iter().rev(){stack.push(*c);}}}Ok(s)}
 fn add(&mut self,kind:Kind)->PResult<usize>{if self.doc.nodes.len()>=4096{return Err(resource("Page DOM node bound exceeded"));}self.charge(html_document::node_bytes(&kind))?;let n=self.doc.nodes.len();self.doc.nodes.push(Node{kind,parent:None,children:Vec::new(),span:0..0,closing_span:None});Ok(n)}
 fn text_node(&mut self,s:String)->PResult<usize>{if s.len()>16384{return Err(resource("Page DOM text bound exceeded"));}self.charge(s.len()+32)?;self.add(Kind::Text{mode:TextMode::Normal,data:s})}
 fn detach(&mut self,n:usize){if let Some(p)=self.doc.nodes[n].parent.take(){self.doc.nodes[p].children.retain(|v|*v!=n);}}
 fn append(&mut self,parent:usize,child:usize)->PResult<()>{self.tick()?;self.container(parent)?;if !matches!(self.doc.nodes[parent].kind,Kind::Element{..}|Kind::Document)||!matches!(self.doc.nodes[child].kind,Kind::Element{..}|Kind::Text{..}|Kind::Comment{..}){return Err(input("Invalid page appendChild node type"));}let mut p=Some(parent);while let Some(i)=p{self.tick()?;if i==child{return Err(input("Page appendChild would create a cycle"));}p=self.doc.nodes[i].parent;}self.detach(child);self.doc.nodes[child].parent=Some(parent);self.doc.nodes[parent].children.push(child);self.mutations+=1;Ok(())}
 fn clear(&mut self,n:usize){for c in std::mem::take(&mut self.doc.nodes[n].children){self.doc.nodes[c].parent=None;}}
 fn member(&mut self,v:Value,k:&str)->PResult<Value>{self.tick()?;match v{
  Value::Document=>match k{"body"=>self.lookup("body",false),"documentElement"=>self.lookup("html",false),"textContent"|"parentNode"=>Ok(Value::Null),"firstChild"=>Ok(self.doc.nodes[0].children.first().copied().map(node_value).unwrap_or(Value::Null)),_=>Err(input(format!("Page document property {k} is not implemented")))},
  Value::Node(n)=>match k{"textContent"=>{if matches!(self.doc.nodes[n].kind,Kind::Document|Kind::Doctype{..}){return Ok(Value::Null);}let s=self.content(n)?;self.string(s)},"id"=>{let s=self.attr(n,"id").unwrap_or_default();self.string(s)},"className"=>{let s=self.attr(n,"class").unwrap_or_default();self.string(s)},"style"=>Ok(Value::Style(n)),"parentNode"=>Ok(self.doc.nodes[n].parent.map(node_value).unwrap_or(Value::Null)),"firstChild"=>Ok(if self.is_template(n){Value::Null}else{self.doc.nodes[n].children.first().copied().map(node_value).unwrap_or(Value::Null)}),"tagName"=>if let Kind::Element{name,..}=&self.doc.nodes[n].kind{let s=name.to_ascii_uppercase();self.string(s)}else{Ok(Value::Undefined)},_=>Err(input(format!("Page node property {k} is not implemented")))},
  Value::Heap(id)=>{if self.objects[id].array&&k=="length"{return Ok(Value::Number(self.objects[id].length as f64));}if let Some(v)=self.own(id,k)?{return Ok(v);}if inherited_property(k)||(self.objects[id].array&&["at","concat","copyWithin","entries","every","fill","filter","find","findIndex","findLast","findLastIndex","flat","flatMap","forEach","includes","indexOf","join","keys","lastIndexOf","map","pop","push","reduce","reduceRight","reverse","shift","slice","some","sort","splice","toReversed","toSorted","toSpliced","unshift","values","with"].contains(&k)){return Err(input("Page inherited method/property values are not implemented"));}Ok(Value::Undefined)},
  Value::Text(s) if k=="length"=>Ok(Value::Number(s.encode_utf16().count()as f64)),Value::Style(n)=>{let prop=css_name(k)?;let value=self.attr(n,"style").unwrap_or_default().split(';').filter_map(|v|v.split_once(':')).filter(|(a,_)|a.trim()==prop).map(|(_,v)|v.trim().to_string()).last().unwrap_or_default();self.string(value)},_=>Err(input("Page property target is unavailable or unsupported"))}}
 fn container(&self,n:usize)->PResult<()>{match &self.doc.nodes[n].kind{Kind::Element{name,void:false,..}if !["script","style","title","textarea","xmp","iframe","noembed","noframes","plaintext","template"].contains(&name.as_str())=>Ok(()),_=>Err(input("This page DOM subset cannot mutate raw-text, void or document containers"))}}
 fn reference(&mut self,e:&Expr)->PResult<Reference>{match e{Expr::Name(n)=>Ok(Reference::Name(n.clone())),Expr::Member(a,k)=>Ok(Reference::Member(self.eval(a)?,k.clone())),Expr::Index(a,k)=>{let v=self.eval(a)?;let key=self.eval(k)?;Ok(Reference::Member(v,self.key(&key)?))},_=>Err(input("Invalid page assignment target"))}}
 fn reference_value(&mut self,r:&Reference)->PResult<Value>{match r{Reference::Name(n)=>self.name(n),Reference::Member(v,k)=>self.member(v.clone(),k)}}
 fn nullable_dom_text(&mut self,v:&Value)->PResult<String>{let text=if matches!(v,Value::Null|Value::Undefined){String::new()}else{self.text(v)?};if text.len()>16384{return Err(resource("Page DOM text bound exceeded"));}self.charge(text.len())?;Ok(text)}
 fn assign_reference(&mut self,r:Reference,v:Value)->PResult<()>{match r{Reference::Name(n)=>self.set_name(&n,v),Reference::Member(target,k)=>match target{
  Value::Heap(id)=>self.heap_set(id,k,v,false),
  Value::Document if k=="textContent"=>{let _text=self.nullable_dom_text(&v)?;Ok(())},
  Value::Node(n)=>match k.as_str(){
   "textContent"=>{let text=self.nullable_dom_text(&v)?;if matches!(self.doc.nodes[n].kind,Kind::Text{..})&&self.doc.nodes[n].parent.is_some_and(|p|matches!(&self.doc.nodes[p].kind,Kind::Element{name,..}if ["script","style","title","textarea","xmp","iframe","noembed","noframes","plaintext","template"].contains(&name.as_str()))){return Err(input("This page DOM subset cannot mutate raw-text children"));}match &mut self.doc.nodes[n].kind{Kind::Text{data,..}|Kind::Comment{data}=>{*data=text;self.mutations+=1;Ok(())},Kind::Document|Kind::Doctype{..}=>Ok(()),_=>{self.container(n)?;self.clear(n);if !text.is_empty(){let c=self.text_node(text)?;self.append(n,c)?;}self.mutations+=1;Ok(())}}},
   "id"=>{let text=self.text(&v)?;self.set_attr(n,"id",text)},"className"=>{let text=self.text(&v)?;self.set_attr(n,"class",text)},_=>Err(input("Page node assignment is unsupported"))},
  Value::Style(n)=>{let prop=css_name(&k)?;let mut css=self.attr(n,"style").unwrap_or_default();let text=self.text(&v)?;css.push_str(&format!(";{prop}:{text};"));self.set_attr(n,"style",css)},_=>Err(input("Page assignment target is not a writable DOM value"))}}}

 fn method(&mut self,v:Value,k:&str,args:Vec<Value>)->PResult<Value>{self.tick()?;match v{
  Value::Heap(id)=>match k{
   "valueOf"=>Ok(Value::Heap(id)),
   "toString"=>{let text=self.text(&Value::Heap(id))?;self.string(text)},
   "join" if self.objects[id].array=>{if args.len()>1{return Err(input("Page array join accepts at most one separator"));}let separator=if args.first().is_none_or(|v|matches!(v,Value::Undefined)){",".into()}else{self.text(&args[0])?};let text=self.join_array(id,&separator,&mut Vec::new())?;self.string(text)},
   "push" if self.objects[id].array=>{let n=self.objects[id].length;let length=(n as u64).checked_add(args.len()as u64).filter(|v|*v<=u32::MAX as u64).ok_or_else(||input("Page array push would exceed array length"))?;for(i,v)in args.into_iter().enumerate(){self.charge(64)?;self.heap_set(id,(n as u64+i as u64).to_string(),v,false)?;}Ok(Value::Number(length as f64))},
   "pop" if self.objects[id].array=>{if !args.is_empty(){return Err(input("Page array pop arguments are not implemented"));}let n=self.objects[id].length;if n==0{return Ok(Value::Undefined);}self.charge(64)?;let key=(n-1).to_string();let v=self.own(id,&key)?.unwrap_or(Value::Undefined);self.objects[id].properties.remove(&key);self.objects[id].length=n-1;Ok(v)},
   _=>Err(input("This page array/object method is not implemented"))},
  Value::Document=>match k{"getElementById"|"querySelector"=>{if args.len()!=1{return Err(input("Page DOM lookup requires one argument"));}{let selector=self.text(&args[0])?;self.lookup(&selector,k=="getElementById")}},"createElement"=>{if args.len()!=1{return Err(input("Page createElement requires one argument"));}let name=self.text(&args[0])?.to_ascii_lowercase();if name.is_empty()||name.len()>64||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')||["script","iframe","object","embed","link","base"].contains(&name.as_str()){return Err(input("Unsupported page element creation name"));}let void=["area","br","col","hr","img","input","meta","source","track","wbr"].contains(&name.as_str());Ok(Value::Node(self.add(Kind::Element{name,attributes:Vec::new(),void})?))},"createTextNode"=>{if args.len()!=1{return Err(input("Page createTextNode requires one argument"));}{let text=self.text(&args[0])?;Ok(Value::Node(self.text_node(text)?))}},_=>Err(input(format!("Page document method {k} is not implemented")))},
  Value::Node(n)=>match k{"appendChild"=>{if args.len()!=1{return Err(input("Page appendChild requires one node"));}let Value::Node(c)=args[0]else{return Err(input("Page appendChild requires a node value"));};self.append(n,c)?;Ok(Value::Node(c))},"append"|"replaceChildren"=>{self.container(n)?;if k=="replaceChildren"{self.clear(n);}for a in args{let c=match a{Value::Node(c)=>c,Value::Document=>return Err(input("Page append cannot insert the document root")),_=>{let text=self.text(&a)?;self.text_node(text)?}};self.append(n,c)?;}Ok(Value::Undefined)},"setAttribute"=>{if args.len()!=2{return Err(input("Page setAttribute requires name and value"));}let key=self.text(&args[0])?.to_ascii_lowercase();let value=self.text(&args[1])?;self.set_attr(n,&key,value)?;Ok(Value::Undefined)},"getAttribute"=>{if args.len()!=1{return Err(input("Page getAttribute requires one name"));}let key=self.text(&args[0])?.to_ascii_lowercase();match self.attr(n,&key){Some(s)=>self.string(s),None=>Ok(Value::Null)}},"remove"=>{if !args.is_empty(){return Err(input("Page remove accepts no arguments"));}self.detach(n);self.mutations+=1;Ok(Value::Undefined)},_=>Err(input(format!("Page node method {k} is not implemented")))},_=>Err(input("Page method receiver is unsupported"))}}
}
fn css_name(k:&str)->PResult<String>{if k.is_empty()||k.len()>128||!k.bytes().all(|v|v.is_ascii_alphanumeric()||v==b'-'){return Err(input("Invalid page style property"));}let mut out=String::new();for c in k.chars(){if c.is_ascii_uppercase(){out.push('-');out.push(c.to_ascii_lowercase());}else{out.push(c);}}Ok(out)}
fn attribute<'a>(node:&'a Node,k:&str)->Option<&'a str>{if let Kind::Element{attributes,..}=&node.kind{attributes.iter().find(|a|a.name==k).map(|a|a.value.as_deref().unwrap_or(""))}else{None}}
fn escape(s:&str,out:&mut String,attribute:bool){for c in s.chars(){match c{'&'=>out.push_str("&amp;"),'<' =>out.push_str("&lt;"),'>' =>out.push_str("&gt;"),'"' if attribute=>out.push_str("&quot;"),_=>out.push(c)}}}
fn serialize(rt:&mut Runtime<'_>,limit:usize)->PResult<String>{let mut out=String::new();let mut stack=vec![(0usize,false,0usize)];while let Some((n,close,depth))=stack.pop(){rt.tick()?;if depth>64{return Err(resource("Page DOM serialization depth bound exceeded"));}match &rt.doc.nodes[n].kind{
 Kind::Document=>for c in rt.doc.nodes[n].children.iter().rev(){stack.push((*c,false,depth));},Kind::Element{name,attributes,void}=>{if close{out.push_str("</");out.push_str(name);out.push('>');}else{out.push('<');out.push_str(name);for a in attributes{out.push(' ');out.push_str(&a.name);if let Some(v)=&a.value{out.push_str("=\"");escape(v,&mut out,true);out.push('"');}}out.push('>');if !*void{stack.push((n,true,depth));for c in rt.doc.nodes[n].children.iter().rev(){stack.push((*c,false,depth+1));}}}},Kind::Text{data,..}=>{let raw=rt.doc.nodes[n].parent.is_some_and(|p|matches!(&rt.doc.nodes[p].kind,Kind::Element{name,..}if ["script","style","xmp","iframe","noembed","noframes","plaintext"].contains(&name.as_str())));if raw{out.push_str(data);}else{escape(data,&mut out,false);}},Kind::Doctype{declaration}=>{out.push_str("<!");out.push_str(declaration);out.push('>');},Kind::Comment{..}|Kind::UnknownMarkup=>{}}
 if out.len()>limit{return Err(resource("Page program serialized DOM exceeds its representation bound"));}}Ok(out)}

fn validate_scope(body:&[Stmt],parameters:&[String])->PResult<(std::collections::BTreeSet<String>,std::collections::BTreeSet<String>)>{
 let mut lexical=std::collections::BTreeSet::new();let mut functions=std::collections::BTreeSet::new();
 for stmt in body{match stmt{Stmt::Decl(n,_,_)=>{if !lexical.insert(n.clone())||parameters.contains(n){return Err(input("Duplicate page lexical declaration or parameter collision"));}},Stmt::Function(n,f)=>{functions.insert(n.clone());validate_scope(&f.body,&f.params)?;},_=>{}}}
 if lexical.iter().any(|n|functions.contains(n)){return Err(input("Page function and lexical declarations collide"));}
 fn nested(s:&Stmt)->PResult<()>{match s{Stmt::Block(v)=>{validate_scope(v,&[])?;},Stmt::If(_,a,b)=>{if matches!(a.as_ref(),Stmt::Decl(..)){return Err(input("A page lexical declaration requires a block"));}nested(a)?;if let Some(b)=b{if matches!(b.as_ref(),Stmt::Decl(..)){return Err(input("A page lexical declaration requires a block"));}nested(b)?;}},Stmt::While(_,b)|Stmt::For(_,_,_,b)=>{if matches!(b.as_ref(),Stmt::Decl(..)){return Err(input("A page lexical declaration requires a block"));}nested(b)?;},_=>{}}Ok(())}
 for s in body{nested(s)?;}Ok((lexical,functions))
}
fn validate_programs(programs:&[Vec<Stmt>])->PResult<()>{let mut lexical=std::collections::BTreeSet::new();let mut functions=std::collections::BTreeSet::new();for body in programs{let(l,f)=validate_scope(body,&[])?;if l.iter().any(|n|lexical.contains(n)||functions.contains(n))||f.iter().any(|n|lexical.contains(n)){return Err(input("Page script global declarations collide"));}lexical.extend(l);functions.extend(f);}if functions.len()>256{return Err(resource("Page function table bound exceeded"));}Ok(())}
fn ascii_space(c:char)->bool{matches!(c,'\t'|'\n'|'\u{000c}'|'\r'|' ')}
const CLASSIC_TYPES:[&str;16]=["application/ecmascript","application/javascript","application/x-ecmascript","application/x-javascript","text/ecmascript","text/javascript","text/javascript1.0","text/javascript1.1","text/javascript1.2","text/javascript1.3","text/javascript1.4","text/javascript1.5","text/jscript","text/livescript","text/x-ecmascript","text/x-javascript"];
fn classic_script(node:&Node)->PResult<bool>{
 if attribute(node,"src").is_some()||attribute(node,"nomodule").is_some(){return Ok(false);}
 if let Some(kind)=attribute(node,"type"){
  if kind.is_empty(){return Ok(true);}if kind.eq_ignore_ascii_case("module"){return Err(input("Module page scripts are not implemented"));}
  let kind=kind.trim_matches(ascii_space);return Ok(CLASSIC_TYPES.iter().any(|mime|kind.eq_ignore_ascii_case(mime)));
 }
 match attribute(node,"language"){
  None|Some("")=>Ok(true),Some(language)=>{let language=language.trim_end_matches(ascii_space);Ok(CLASSIC_TYPES.iter().filter_map(|mime|mime.strip_prefix("text/")).any(|mime|language.eq_ignore_ascii_case(mime)))}
 }
}
pub(super) struct Output{pub html:String,pub summary:String}
pub(super) fn run(html:&[u8],room:usize,cancelled:&impl Fn()->bool,remaining:Duration)->PResult<Output>{
 if html.len()>262144{return Err(resource("Page program HTML input exceeds 256 KiB"));}if cancelled(){return Err(resource("Page program cancelled before compilation"));}let deadline=Instant::now().checked_add(remaining.min(Duration::from_secs(2))).ok_or_else(||resource("Page program deadline overflow"))?;
 let parse_floor=html.len().saturating_mul(24).saturating_add(2*1024*1024);if parse_floor>room{return Err(resource("Current memory allowance cannot admit page program parsing"));}
 let doc=html_document::parse_with_room(html,true,room.saturating_sub(parse_floor));if doc.allocation_refused(){return Err(resource("Current memory allowance cannot admit owned page document storage"));}if doc.stopped_by_bound||!doc.source_complete{return Err(resource("Page program requires a complete bounded HTML tree"));}let estimate=parse_floor.saturating_add(doc.allocation_bytes());if estimate>room{return Err(resource("Current memory allowance cannot admit page program compilation and DOM"));}
 let mut programs=Vec::new();let mut compilation_reserved=estimate;let mut token_room=16384usize;let mut node_room=8192usize;let mut source_bytes=0usize;let mut ignored=0usize;
 for node in &doc.nodes{let Kind::Element{name,..}=&node.kind else{continue};if name!="script"{continue;}let mut parent=node.parent;let mut inert=false;while let Some(p)=parent{if matches!(&doc.nodes[p].kind,Kind::Element{name,..}if ["template","noscript","svg","math"].contains(&name.as_str())){inert=true;}parent=doc.nodes[p].parent;}if inert{ignored+=1;continue;}if !classic_script(node)?{ignored+=1;continue;}
  let mut source=String::new();for child in &node.children{if let Kind::Text{data,..}=&doc.nodes[*child].kind{source.push_str(data);}}source_bytes=source_bytes.checked_add(source.len()).ok_or_else(||resource("Page program source count overflow"))?;if source_bytes>65536||programs.len()>=32{return Err(resource("Page inline-program source/count bound exceeded"));}let (program,retained,used_tokens,used_nodes)=Parser::parse(&source,room.saturating_sub(compilation_reserved),token_room,node_room)?;token_room=token_room.checked_sub(used_tokens).ok_or_else(||resource("Page token count overflow"))?;node_room=node_room.checked_sub(used_nodes).ok_or_else(||resource("Page AST count overflow"))?;compilation_reserved=compilation_reserved.checked_add(retained).ok_or_else(||resource("Page compilation reservation overflow"))?;programs.push(program);if cancelled()||Instant::now()>=deadline{return Err(resource("Page program compilation interrupted"));}
 }
 let mut rt=Runtime{doc,objects:Vec::new(),scopes:vec![BTreeMap::new()],functions:BTreeMap::new(),fuel:100000,heap:0,heap_limit:(room/8).min(1024*1024).min(room.saturating_sub(compilation_reserved)),bindings:0,calls:0,deadline,cancelled,mutations:0,frames:0};
 // Parse and declaration-validate every source before the first DOM mutation.
 validate_programs(&programs)?;
 for program in &programs{for stmt in program{if let Stmt::Function(name,f)=stmt{if rt.functions.len()>=256&&!rt.functions.contains_key(name){return Err(resource("Page function table bound exceeded"));}rt.charge(name.len()+16*(std::mem::size_of::<Rc<Function>>()+std::mem::size_of::<String>())+128)?;rt.functions.insert(name.clone(),f.clone());}}rt.body(program)?;}let result=serialize(&mut rt,262144)?;let summary=format!("Owned inline page programs: {} compiled/executed; {} script elements left inert; {} DOM mutations; {} of100000 instruction units used. Implemented language/DOM subset, not full ECMAScript/browser semantics. External scripts, events, timers, eval and network APIs were not executed.",programs.len(),ignored,rt.mutations,100000-rt.fuel);Ok(Output{html:result,summary})
}
/// Process-held ownership is derived from the approved current operation,
/// never from a development report or an unrelated local-tool cancellation.
use std::sync::{Arc,Mutex,OnceLock,atomic::{AtomicBool,AtomicU64,Ordering}};
pub(super) type WebCancellation=Arc<AtomicBool>;
struct WebOwner{token:u64,full:String,stable:String,ticket:String,cancelled:WebCancellation,deadline:Instant}
static WEB_SERIAL:AtomicU64=AtomicU64::new(1);
fn web_owners()->&'static Mutex<Vec<WebOwner>>{static OWNERS:OnceLock<Mutex<Vec<WebOwner>>>=OnceLock::new();OWNERS.get_or_init(||Mutex::new(Vec::new()))}
fn prune_owners(owners:&mut Vec<WebOwner>){let now=Instant::now();owners.retain(|owner|{let live=!owner.cancelled.load(Ordering::SeqCst)&&now<owner.deadline;if !live{owner.cancelled.store(true,Ordering::SeqCst);}live});}
fn owner_digest(owner:&Json,stable:bool)->Result<String>{
 fn bytes(d:&mut super::incremental_digest::Digest,v:&[u8],work:&mut usize)->Result<()>{*work=work.checked_sub(v.len().saturating_add(8)).ok_or("Web ownership work bound exceeded")?;d.update(&(v.len()as u64).to_le_bytes())?;d.update(v)}
 fn walk(d:&mut super::incremental_digest::Digest,v:&Json,depth:usize,work:&mut usize)->Result<()>{if depth>128{return Err("Web ownership nesting bound exceeded".into());}*work=work.checked_sub(1).ok_or("Web ownership work bound exceeded")?;match v{
  Null=>d.update(&[0]),Bool(v)=>d.update(&[1,*v as u8]),Num(v)=>{d.update(&[2])?;bytes(d,v.as_bytes(),work)},Str(v)=>{d.update(&[3])?;bytes(d,v.as_bytes(),work)},
  Arr(values)=>{d.update(&[4])?;d.update(&(values.len()as u64).to_le_bytes())?;for v in values{walk(d,v,depth+1,work)?;}Ok(())},
  Obj(values)=>{d.update(&[5])?;d.update(&(values.len()as u64).to_le_bytes())?;for(k,v)in values{bytes(d,k.as_bytes(),work)?;walk(d,v,depth+1,work)?;}Ok(())}
 }}
 let Obj(fields)=owner else{return Err("Missing captured web ownership object".into());};let mut digest=super::incremental_digest::Digest::new();let mut work=24*1024*1024usize;
 for(k,v)in fields{if stable&&["ticket","phase","capture_token","redirect_chain","search_chain","program_summary"].contains(&k.as_str()){continue;}bytes(&mut digest,k.as_bytes(),&mut work)?;walk(&mut digest,v,0,&mut work)?;}
 Ok(digest.finish())
}
fn owner_token(owner:&Json)->Result<u64>{match get(owner,"program_token"){Num(v)=>v.parse::<u64>().map_err(|_|"Invalid page program token".into()),_=>Err("Missing page program token".into())}}
pub(super) fn clock_ms()->u64{static START:OnceLock<Instant>=OnceLock::new();START.get_or_init(Instant::now).elapsed().as_millis().min(u64::MAX as u128)as u64}
pub(super) fn bind_web(owner:&mut Json)->Result<()>{
 let selected=enabled(get(owner,"args"))?;if !selected&&get(owner,"program_token")==&Null{return Ok(());}
 let mut owners=web_owners().lock().map_err(|_|"Page program ownership unavailable")?;prune_owners(&mut owners);
 if get(owner,"program_token")!=&Null{
  let token=owner_token(owner)?;let stable=owner_digest(owner,true)?;let entry=owners.iter_mut().find(|v|v.token==token).ok_or("Page program owner expired or was cancelled")?;
  if !selected||entry.stable!=stable{return Err("Captured page program ownership changed".into());}entry.full=owner_digest(owner,false)?;entry.ticket=text(owner,"ticket");return Ok(());
 }
 if owners.len()>=4{return Err("Four page program owners are already pending".into());}
 let token=WEB_SERIAL.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|v|v.checked_add(1)).map_err(|_|"Page program token space exhausted")?;
 let deadline=Instant::now().checked_add(Duration::from_secs(120)).ok_or("Page program deadline overflow")?;let stamp=clock_ms().checked_add(120000).ok_or("Page program clock exhausted")?;
 put(owner,"program_token",Num(token.to_string()));put(owner,"program_deadline_ms",Num(stamp.to_string()));
 let full=owner_digest(owner,false)?;let stable=owner_digest(owner,true)?;let ticket=text(owner,"ticket");owners.push(WebOwner{token,full,stable,ticket,cancelled:Arc::new(AtomicBool::new(false)),deadline});Ok(())
}
/// Adopt only the summary produced by the already admitted inline stage,
/// before the same owner moves into resource or rendering callbacks.
pub(super) fn set_web_summary(owner:&mut Json,summary:&str)->Result<()>{
 let token=owner_token(owner)?;let previous=owner_digest(owner,false)?;let mut owners=web_owners().lock().map_err(|_|"Page program ownership unavailable")?;prune_owners(&mut owners);
 let entry=owners.iter_mut().find(|v|v.token==token&&v.full==previous).ok_or("Page program owner changed before capture staging")?;
 put(owner,"program_summary",s(summary));entry.full=owner_digest(owner,false)?;Ok(())
}
pub(super) fn valid_web_binding(owner:&Json)->bool{
 let Ok(mut owners)=web_owners().lock()else{return false;};prune_owners(&mut owners);let ticket=text(owner,"ticket");
 if let Some(entry)=owners.iter().find(|v|v.ticket==ticket){return owner_digest(owner,false).is_ok_and(|v|v==entry.full);}
 get(owner,"program_token")==&Null&&matches!(enabled(get(owner,"args")),Ok(false))
}
pub(super) fn web_limits(owner:&Json)->PResult<(WebCancellation,Duration)>{
 let token=owner_token(owner).map_err(resource)?;let full=owner_digest(owner,false).map_err(resource)?;let mut owners=web_owners().lock().map_err(|_|resource("Page program ownership unavailable"))?;prune_owners(&mut owners);
 let entry=owners.iter().find(|v|v.token==token&&v.full==full).ok_or_else(||resource("Page program owner changed, expired or was cancelled"))?;
 let remaining=entry.deadline.checked_duration_since(Instant::now()).filter(|v|!v.is_zero()).ok_or_else(||resource("Original page operation deadline expired"))?;Ok((entry.cancelled.clone(),remaining))
}
pub(super) fn web_cancelled(token:&WebCancellation)->bool{token.load(Ordering::SeqCst)}
pub(super) fn finish_web(owner:&Json){finish_owner(owner,false);}
/// Only an already authenticated terminal callback may close its original
/// operation after an internally prepared next dispatch was refused.
pub(super) fn finish_web_authenticated(owner:&Json){finish_owner(owner,true);}
fn finish_owner(owner:&Json,authenticated:bool){
 let Ok(token)=owner_token(owner)else{return;};let Ok(digest)=owner_digest(owner,authenticated)else{return;};
 if let Ok(mut owners)=web_owners().lock(){if let Some(i)=owners.iter().position(|v|v.token==token&&if authenticated{v.stable==digest}else{v.full==digest}){let entry=owners.remove(i);entry.cancelled.store(true,Ordering::SeqCst);}}
}
