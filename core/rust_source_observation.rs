//! Original bounded Rust lexical observations; not a compiler or macro expander.
use std::fmt::Write as _;
const INPUT_LIMIT:usize=128*1024;
const TOKEN_LIMIT:usize=16384;
const COMMENT_DEPTH:usize=64;
const DELIMITER_DEPTH:usize=128;
const RECORD_LIMIT:usize=96;
const OUTPUT_LIMIT:usize=8191;

#[derive(Clone,Copy,PartialEq,Debug)]enum Kind{Ident,RawIdent,Lifetime,Literal,Punct}
#[derive(Clone,Copy)]struct Token{start:usize,end:usize,line:usize,column:usize,kind:Kind}
struct Lexer<'a>{text:&'a str,at:usize,line:usize,line_start:usize,tokens:Vec<Token>,issue:Option<String>,whole:bool,stack:Vec<u8>,comments:usize,literals:usize}
fn ident_start(c:char)->bool{c=='_'||c.is_alphabetic()}
fn ident_continue(c:char)->bool{ident_start(c)||c.is_alphanumeric()}
fn clipped(text:&str,limit:usize)->&str{let mut end=text.len().min(limit);while !text.is_char_boundary(end){end-=1;}&text[..end]}
fn safe(text:&str,limit:usize)->String{let mut out=String::new();for c in text.chars(){let part=if c.is_control()||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {format!("\\u{{{:x}}}",c as u32)}else{c.to_string()};if out.len()+part.len()>limit{out.push_str("…");break;}out.push_str(&part);}out}
impl<'a> Lexer<'a>{
 fn new(text:&'a str,whole:bool)->Self{Self{text,at:0,line:1,line_start:0,tokens:Vec::new(),issue:None,whole,stack:Vec::new(),comments:0,literals:0}}
 fn advance(&mut self,end:usize){let b=self.text.as_bytes();while self.at<end{if b[self.at]==b'\r'{self.line+=1;self.line_start=self.at+1;}else if b[self.at]==b'\n'{if self.at==0||b[self.at-1]!=b'\r'{self.line+=1;}self.line_start=self.at+1;}self.at+=1;}}
 fn stop(&mut self,reason:&str){self.issue=Some(format!("{reason} at byte {} (line {}, UTF-8 byte column {}). {}",self.at,self.line,self.at-self.line_start+1,if self.whole{"This is an observation of the complete supplied input, not a compiler diagnostic."}else{"Only a prefix was supplied/examined; unseen bytes may complete the token."}));}
 fn emit(&mut self,end:usize,kind:Kind)->bool{if self.tokens.len()==TOKEN_LIMIT{self.stop("Token work bound reached; remainder not lexed");return false;}self.tokens.push(Token{start:self.at,end,line:self.line,column:self.at-self.line_start+1,kind});self.advance(end);true}
 fn identifier_end(&self,start:usize)->usize{let mut end=start;for c in self.text[start..].chars(){if !ident_continue(c){break;}end+=c.len_utf8();}end}
 fn ordinary_string(&mut self,start:usize)->Option<usize>{
  let b=self.text.as_bytes();let mut p=start+1;
  while p<b.len(){match b[p]{b'"'=>return Some(p+1),b'\\'=>{p+=1;if p==b.len(){break;}let c=self.text[p..].chars().next().unwrap();p+=c.len_utf8();},_=>{p+=self.text[p..].chars().next().unwrap().len_utf8();}}}
  self.stop("Unterminated ordinary string in examined bytes; its contents were excluded");None
 }
 fn raw_string(&mut self,quote:usize,hashes:usize)->Option<usize>{
  let b=self.text.as_bytes();let mut p=quote+1;
  while p<b.len(){if b[p]==b'"'{p+=1;let mut seen=0;while seen<hashes&&b.get(p)==Some(&b'#'){seen+=1;p+=1;}if seen==hashes{return Some(p);}}else{p+=1;}}
  self.stop("Unterminated raw string in examined bytes; its contents were excluded");None
 }
 fn escaped_char(&self,start:usize,byte:bool)->Option<usize>{
  let b=self.text.as_bytes();let code=*b.get(start)?;
  match code{
   b'n'|b'r'|b't'|b'0'|b'\\'|b'\''|b'"'=>Some(start+1),
   b'x'=>{let h=b.get(start+1..start+3)?;let text=std::str::from_utf8(h).ok()?;let value=u8::from_str_radix(text,16).ok()?;if !byte&&value>127{None}else{Some(start+3)}},
   b'u' if !byte=>{if b.get(start+1)!=Some(&b'{'){return None;}let mut p=start+2;let mut digits=0;let mut value=0u32;while let Some(&c)=b.get(p){if c==b'}'{return if digits>0&&char::from_u32(value).is_some(){Some(p+1)}else{None};}if c!=b'_'{let d=(c as char).to_digit(16)?;digits+=1;if digits>6{return None;}value=value.checked_mul(16)?.checked_add(d)?;}p+=1;}None},
   _=>None
  }
 }
 fn apostrophe(&mut self,quote:usize,byte:bool)->Option<(usize,Kind)>{
  let b=self.text.as_bytes();let start=quote+1;
  if start==b.len(){self.stop("Incomplete apostrophe token");return None;}
  if b[start]==b'\\'{let end=match self.escaped_char(start+1,byte){Some(v)=>v,None=>{self.stop("Invalid or incomplete character escape");return None;}};if b.get(end)==Some(&b'\''){return Some((end+1,Kind::Literal));}self.stop("Character escape has no closing apostrophe in examined bytes");return None;}
  let c=self.text[start..].chars().next().unwrap();let end=start+c.len_utf8();
  if c!='\''&&!matches!(c,'\n'|'\r'|'\t')&&b.get(end)==Some(&b'\''){
   if byte&&!c.is_ascii(){self.stop("Non-ASCII byte-character literal");return None;}return Some((end+1,Kind::Literal));
  }
  if !byte&&ident_start(c){let end=self.identifier_end(start);if b.get(end)==Some(&b'\''){self.stop("Multiple characters in a character-shaped token");return None;}if end==b.len()&&!self.whole{self.stop("Apostrophe identifier reaches the sample boundary; lifetime/character classification withheld");return None;}return Some((end,Kind::Lifetime));}
  self.stop("Invalid or incomplete character/apostrophe token");None
 }
 fn run(&mut self){
  let b=self.text.as_bytes();
  while self.at<b.len(){
   let at=self.at;let c=self.text[at..].chars().next().unwrap();
   if c.is_whitespace(){self.advance(at+c.len_utf8());continue;}
   // A script-style first-line shebang is non-source text, unlike #![attributes].
   if at==0&&self.text.starts_with("#!")&&!self.text.starts_with("#!["){let end=self.text.find(['\n','\r']).unwrap_or(b.len());self.comments+=1;self.advance(end);continue;}
   if b.get(at..at+2)==Some(b"//"){let end=self.text[at..].find(['\n','\r']).map(|p|at+p).unwrap_or(b.len());self.comments+=1;self.advance(end);continue;}
   if b.get(at..at+2)==Some(b"/*"){
    let mut p=at+2;let mut depth=1;self.comments+=1;
    while p<b.len()&&depth>0{if b.get(p..p+2)==Some(b"/*"){depth+=1;if depth>COMMENT_DEPTH{self.stop("Nested block-comment depth bound reached; remainder excluded");return;}p+=2;}else if b.get(p..p+2)==Some(b"*/"){depth-=1;p+=2;}else{p+=1;}}
    if depth!=0{self.stop("Unterminated nested block comment in examined bytes; contents excluded");return;}self.advance(p);continue;
   }
   // Recognize prefixes before identifier lexing: r/br/cr + hashes + quote.
   let raw_start=if b[at]==b'r'{Some(at+1)}else if matches!(b[at],b'b'|b'c')&&b.get(at+1)==Some(&b'r'){Some(at+2)}else{None};
   if let Some(mut p)=raw_start{let begin=p;while b.get(p)==Some(&b'#'){p+=1;}let hashes=p-begin;
    if hashes>255{self.stop("Raw-string hash delimiter exceeds 255; remainder excluded");return;}
    if b.get(p)==Some(&b'"'){if let Some(end)=self.raw_string(p,hashes){self.literals+=1;if !self.emit(end,Kind::Literal){return;}continue;}return;}
    if b[at]==b'r'&&hashes==1{if let Some(c)=self.text[p..].chars().next(){if ident_start(c){let end=self.identifier_end(p);if end==b.len()&&!self.whole{self.stop("Raw identifier reaches sample boundary; incomplete name withheld");return;}if !self.emit(end,Kind::RawIdent){return;}continue;}}}
    if hashes>0{self.stop("Incomplete or invalid raw-string/raw-identifier prefix");return;}
   }
   let string_quote=if b[at]==b'"'{Some(at)}else if matches!(b[at],b'b'|b'c')&&b.get(at+1)==Some(&b'"'){Some(at+1)}else{None};
   if let Some(quote)=string_quote{if let Some(end)=self.ordinary_string(quote){self.literals+=1;if !self.emit(end,Kind::Literal){return;}continue;}return;}
   let apostrophe=if b[at]==b'\''{Some((at,false))}else if b[at]==b'b'&&b.get(at+1)==Some(&b'\''){Some((at+1,true))}else{None};
   if let Some((quote,byte))=apostrophe{if let Some((end,kind))=self.apostrophe(quote,byte){if kind==Kind::Literal{self.literals+=1;}if !self.emit(end,kind){return;}continue;}return;}
   if ident_start(c){let end=self.identifier_end(at);if end==b.len()&&!self.whole{self.stop("Identifier reaches sample boundary; incomplete name withheld");return;}if !self.emit(end,Kind::Ident){return;}continue;}
   if c.is_ascii_digit(){let mut end=at+1;while end<b.len()&&(b[end].is_ascii_alphanumeric()||b[end]==b'_'){end+=1;}self.literals+=1;if !self.emit(end,Kind::Literal){return;}continue;}
   if matches!(b[at],b'('|b'['|b'{'){if self.stack.len()==DELIMITER_DEPTH{self.stop("Delimiter nesting bound reached; remainder not lexed");return;}self.stack.push(b[at]);}
   if matches!(b[at],b')'|b']'|b'}'){let open=match b[at]{b')'=>b'(',b']'=>b'[',_=>b'{'};if self.stack.pop()!=Some(open){self.stop("Mismatched closing delimiter; remainder not lexed");return;}}
   if !self.emit(at+c.len_utf8(),Kind::Punct){return;}
  }
  if !self.stack.is_empty(){self.stop("Open delimiters remain at the examined boundary");}
 }
}
fn token_text<'a>(text:&'a str,t:&Token)->&'a str{&text[t.start..t.end]}
fn identifier(t:&Token)->bool{matches!(t.kind,Kind::Ident|Kind::RawIdent)}
fn header(text:&str,tokens:&[Token],start:usize,stop_at_brace:bool)->(String,bool){
 let mut out=String::new();let mut complete=false;let mut used=0;
 for token in tokens.iter().skip(start).take(64){let value=token_text(text,token);if value==";"||(stop_at_brace&&value=="{"){complete=true;break;}let value=if token.kind==Kind::Literal{"<literal>"}else{value};let piece=safe(value,192);if out.len()+piece.len()+1>320{break;}if !out.is_empty(){out.push(' ');}out.push_str(&piece);used+=1;}
 if used==64{complete=false;}(out,complete)
}

/// `complete` refers to the caller's entire selected source. The local byte,
/// token and nesting bounds can reduce observations to a lexical prefix.
pub(super) fn analyze(data:&[u8],complete:bool)->String{
 let cap=data.len().min(INPUT_LIMIT);
 let (text,examined,partial_utf8)=match std::str::from_utf8(&data[..cap]){
  Ok(text)=>(text,cap,false),
  Err(error) if error.error_len().is_none()&&(!complete||cap<data.len())=>{let end=error.valid_up_to();(std::str::from_utf8(&data[..end]).unwrap(),end,true)},
  Err(error)=>return format!("Rust lexical observations\nSupplied {} bytes; examined at most {cap}. Invalid UTF-8 at byte {}; source tokenization was not performed. This is not binary-format identification or a compiler verdict.",data.len(),error.valid_up_to()),
 };
 let whole=complete&&examined==data.len();let text=text.strip_prefix('\u{feff}').unwrap_or(text);
 let mut lexer=Lexer::new(text,whole);lexer.run();
 let mut out=format!("Rust lexical source observations\nSupplied {} bytes; examined {examined} bytes (byte cap {INPUT_LIMIT}). Scope: {}. Locations are one-based lines and UTF-8 byte columns in the examined text (initial UTF-8 BOM excluded).\n",data.len(),if whole{"complete supplied input"}else{"prefix only; unexamined source unknown"});
 if partial_utf8{out.push_str("A split UTF-8 scalar at the sample boundary was excluded.\n");}
 let _=writeln!(out,"Lexed {} tokens; skipped {} comments and {} literals. Bounds: {TOKEN_LIMIT} tokens, {COMMENT_DEPTH} nested block comments, {DELIMITER_DEPTH} nested delimiters.",lexer.tokens.len(),lexer.comments,lexer.literals);
 if let Some(issue)=&lexer.issue{let _=writeln!(out,"Lexical limitation: {issue}");}
 let mut total=0usize;let mut reported=0usize;
 for (index,token) in lexer.tokens.iter().enumerate(){
  if token.kind!=Kind::Ident{continue;}let keyword=token_text(text,token);
  if !matches!(keyword,"fn"|"struct"|"enum"|"trait"|"mod"|"use"|"impl"){continue;}
  let next=match lexer.tokens.get(index+1){Some(v)=>v,None=>continue};
  let description=if keyword=="use"||keyword=="impl"{
   let (value,ended)=header(text,&lexer.tokens,index+1,keyword=="impl");if value.is_empty(){continue;}format!("{keyword} {}{}",if keyword=="use"{"unresolved import tokens: "}else{"unresolved implementation header tokens: "},format!("{value}{}",if ended{""}else{" [header prefix only]"}))
  }else{
   if !identifier(next){continue;}let name=safe(token_text(text,next),192);
   if keyword=="fn"&&!lexer.tokens.get(index+2).map(|t|matches!(token_text(text,t),"("|"<")).unwrap_or(false){continue;}
   let relation=if keyword=="mod"{match lexer.tokens.get(index+2).map(|t|token_text(text,t)){Some(";")=>"; external module declaration, file/path attributes unresolved",Some("{")=>"; inline module body begins",_=>"; module form incomplete or not resolved"}}else{""};format!("{keyword} {name}{relation}")
  };
  total+=1;if reported<RECORD_LIMIT{let _=writeln!(out,"Line {}, byte column {}: {description}",token.line,token.column);reported+=1;}
 }
 let _=writeln!(out,"Observed {total} declaration-shaped token sequences in the lexed scope; displayed {reported} (record cap {RECORD_LIMIT}).");
 out.push_str("Names and import/header tokens are untrusted source text. Comments and literals are excluded, but macro input/body tokens and cfg-disabled code can still appear. No macro expansion, cfg evaluation, module resolution, call graph, type checking, compilation, execution or declaration-validity claim. Unicode identifier boundaries use standard-library alphabetic/alphanumeric classification, not a full Rust XID implementation. Ordinary string escapes are skipped for token boundaries, not semantically validated.\n");
 if out.len()>OUTPUT_LIMIT{let mut result=clipped(&out,OUTPUT_LIMIT-1400).to_string();result.push_str("\nReport abbreviated; middle records omitted. Final observations follow:\n");let mut tail=out.len().saturating_sub(1200);while !out.is_char_boundary(tail){tail+=1;}if let Some(line)=out[tail..].find('\n'){tail+=line+1;}result.push_str(&out[tail..]);result}else{out}
}

#[cfg(test)]mod checks{
 use super::*;
 #[test]fn comments_and_strings_do_not_create_declarations(){let source=r####"// fn ghost() {}
/* struct Fake; /* fn nested(){} */ enum Bad{} */
const TEXT:&str="fn fake_string() { struct Wrong; }";
const RAW:&str=r###"fn raw_fake(){} \" struct No;"###;
const BYTES:&[u8]=br##"enum NotReal{}"##;
const C=&c"trait NotThere{}";
pub fn real<'a>(s:&'a str)->&'a str{s}
struct Actual;
"####;let out=analyze(source.as_bytes(),true);for name in ["ghost","Fake","nested","Bad","fake_string","Wrong","raw_fake","NotReal","NotThere"]{assert!(!out.lines().filter(|line|line.starts_with("Line ")).any(|line|line.contains(name)),"{name}: {out}");}assert!(out.contains("fn real"));assert!(out.contains("struct Actual"));}
 #[test]fn chars_lifetimes_raw_identifiers_and_unicode(){let source="fn café<'a>(x: &'a str) { let a='雪'; let b=b'\\x7f'; let c='\\u{1f600}'; let d='\\''; 'label: loop {break 'label;} }\nfn r#type() {}\nstruct 世界;";let out=analyze(source.as_bytes(),true);assert!(out.contains("fn café"));assert!(out.contains("fn r#type"));assert!(out.contains("struct 世界"));assert!(!out.contains("Lexical limitation"));}
 #[test]fn imports_modules_and_impl_are_unresolved(){let out=analyze(b"mod disk; mod inline { use crate::thing::{A, B as C}; impl<T> Thing for T { fn run(&self){} } trait Shape {} enum Kind { A } }",true);assert!(out.contains("mod disk; external module"));assert!(out.contains("mod inline; inline"));assert!(out.contains("unresolved import tokens"));assert!(out.contains("unresolved implementation header tokens"));assert!(out.contains("fn run"));assert!(out.contains("trait Shape"));assert!(out.contains("enum Kind"));assert!(out.contains("No macro expansion"));}
 #[test]fn malformed_and_sample_tokens_are_not_rescanned(){for source in ["/* fn hidden(){}", "r###\"fn hidden(){}", "\"fn hidden(){}", "fn real(){ let c='ab'; fn hidden(){} }"]{let out=analyze(source.as_bytes(),true);assert!(out.contains("Lexical limitation"));assert!(!out.contains("fn hidden"));}let sample=analyze(b"fn real(){ let s=r#\"fn hidden",false);assert!(sample.contains("prefix only"));assert!(sample.contains("unseen bytes may complete"));assert!(!sample.contains("fn hidden"));let out=analyze(b"'life",false);assert!(out.contains("classification withheld"));let out=analyze(b"struct Incomplete",false);assert!(!out.contains("struct Incomplete"));assert!(out.contains("incomplete name withheld"));}
 #[test]fn positions_utf8_and_crlf_are_explicit(){let out=analyze("// 雪\r\n  fn next(){}\n".as_bytes(),true);assert!(out.contains("Line 2, byte column 3: fn next"));assert!(analyze(&[0xff],true).contains("Invalid UTF-8"));assert!(analyze(&[b'f',b'n',b' ',0xe9,0x9b],false).contains("split UTF-8 scalar"));}
 #[test]fn work_and_output_bounds_are_disclosed(){let comments=format!("{}{}","/*".repeat(COMMENT_DEPTH+1),"*/".repeat(COMMENT_DEPTH+1));assert!(analyze(comments.as_bytes(),true).contains("comment depth bound"));let tokens="x;".repeat(TOKEN_LIMIT);let out=analyze(tokens.as_bytes(),true);assert!(out.contains("Token work bound"));assert!(out.len()<8192);let source="fn sample(){}\n".repeat(10000);let out=analyze(source.as_bytes(),true);assert!(out.contains("prefix only"));assert!(out.contains("record cap 96"));assert!(out.len()<8192);let delimiter="(".repeat(DELIMITER_DEPTH+1);assert!(analyze(delimiter.as_bytes(),true).contains("Delimiter nesting bound"));}
}
