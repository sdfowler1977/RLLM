//! Original standard-library CSS tokenizer foundation; no cascade or renderer.
//! UTF-8 input only. Tokens retain original byte spans despite CSS preprocessing.
//! Comments are retained as explicit trivia (CSS consumers must skip them).
//! Unicode-range descriptor special parsing and stylesheet grammar are not implemented.
use std::ops::Range;
const INPUT: usize = 256 * 1024;
const TOKENS: usize = 16384;
const DEPTH: usize = 128;
const ISSUES: usize = 128;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Number {
    /// Normalized source spelling; retained even when f64 is not finite.
    pub repr: String,
    /// Approximate binary64 conversion; source spelling is authoritative.
    pub value: Option<f64>,
    pub integer: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Whitespace(String), Comment(String),
    Ident(String), Function(String), AtKeyword(String),
    Hash { value: String, id: bool },
    String(String), BadString(String), Url(String), BadUrl,
    Number(Number), Percentage(Number), Dimension { number: Number, unit: String },
    Delim(char), Open(char), Close(char), Colon, Semicolon, Comma, Cdo, Cdc,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
    /// All tokens from incomplete input are provisional. More bytes require a
    /// full rescan: lookahead can change tokens earlier than the final token.
    pub provisional: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Issue { pub offset: usize, pub code: &'static str }
#[derive(Clone, Debug)]
pub(super) struct SheetTokens {
    pub tokens: Vec<Token>,
    pub issues: Vec<Issue>,
    pub omitted_issues: usize,
    pub supplied_bytes: usize,
    pub examined_bytes: usize,
    pub parsed_bytes: usize,
    pub source_complete: bool,
    pub stopped_by_bound: bool,
}
impl SheetTokens {
    fn issue(&mut self, offset:usize, code:&'static str) {
        if self.issues.len()<ISSUES {self.issues.push(Issue{offset,code});}else{self.omitted_issues+=1;}
    }
    /// Count-only summary never exposes string, URL or declaration contents.
    pub fn summary(&self)->String {
        let trivia=self.tokens.iter().filter(|t|matches!(t.kind,Kind::Comment(_)|Kind::Whitespace(_))).count();
        format!("Native CSS tokens: {} supplied bytes, {} examined, {} parsed; source complete: {}; bound stop: {}. {} tokens including {} trivia tokens. {} recorded issues, {} omitted. UTF-8 only; CSS newline/NUL preprocessing retains original byte spans. Unicode-range descriptor parsing, stylesheet grammar, selectors, cascade, fonts, layout, paint and JavaScript are not implemented here. No resource fetched or screenshot captured.",self.supplied_bytes,self.examined_bytes,self.parsed_bytes,self.source_complete,self.stopped_by_bound,self.tokens.len(),trivia,self.issues.len(),self.omitted_issues)
    }
}
#[derive(Clone,Copy)]struct Unit { c:char, start:usize, end:usize }
struct Lexer { units:Vec<Unit>, p:usize, out:SheetTokens }
fn whitespace(c:char)->bool {matches!(c,' '| '\t'|'\n')}
fn name_start(c:char)->bool {c.is_ascii_alphabetic()||c=='_'||c as u32>=0x80}
fn name_char(c:char)->bool {name_start(c)||c.is_ascii_digit()||c=='-'}
fn nonprintable(c:char)->bool {matches!(c as u32,0..=8|11|14..=31|127)}
impl Lexer {
    fn ch(&self,p:usize)->Option<char>{self.units.get(p).map(|u|u.c)}
    fn offset(&self,p:usize)->usize{self.units.get(p).map(|u|u.start).unwrap_or(self.out.examined_bytes)}
    fn issue(&mut self,p:usize,code:&'static str){self.out.issue(self.offset(p),code);}
    fn valid_escape(&self,p:usize)->bool {self.ch(p)==Some('\\')&&self.ch(p+1)!=Some('\n')}
    fn ident_start(&self,p:usize)->bool {
        match self.ch(p){Some('-')=>self.ch(p+1).map(|c|name_start(c)||c=='-').unwrap_or(false)||self.valid_escape(p+1),Some('\\')=>self.valid_escape(p),Some(c)=>name_start(c),None=>false}
    }
    fn number_start(&self,p:usize)->bool {
        let mut at=p;if matches!(self.ch(at),Some('+')|Some('-')){at+=1;}
        self.ch(at).map(|c|c.is_ascii_digit()).unwrap_or(false)||(self.ch(at)==Some('.')&&self.ch(at+1).map(|c|c.is_ascii_digit()).unwrap_or(false))
    }
    fn text(&self,start:usize,end:usize)->String{self.units[start..end].iter().map(|u|u.c).collect()}
    fn begins(&self,text:&str)->bool{text.chars().enumerate().all(|(n,c)|self.ch(self.p+n)==Some(c))}
    // Caller has established a valid escape at p. EOF is replaced, not indexed.
    fn escape(&mut self)->char {
        let start=self.p;self.p+=1;
        match self.ch(self.p){
            Some(c) if c.is_ascii_hexdigit()=>{
                let mut value=0;let mut count=0;
                while count<6{match self.ch(self.p).and_then(|c|c.to_digit(16).filter(|_|c.is_ascii_hexdigit())){Some(d)=>{value=value*16+d;self.p+=1;count+=1;},None=>break}}
                if self.ch(self.p).map(whitespace).unwrap_or(false){self.p+=1;}
                if value==0||value>0x10ffff||(0xd800..=0xdfff).contains(&value){self.issue(start,"invalid-escaped-scalar-replaced");'\u{fffd}'}else{char::from_u32(value).unwrap()}
            },
            Some(c)=>{self.p+=1;c},None=>{self.issue(start,"escape-at-input-end-replaced");'\u{fffd}'}
        }
    }
    fn name(&mut self)->String {
        let mut value=String::new();loop {match self.ch(self.p){Some(c) if name_char(c)=>{value.push(c);self.p+=1;},Some('\\') if self.valid_escape(self.p)=>{value.push(self.escape());},_=>break}}value
    }
    fn numeric(&mut self)->Kind {
        let start=self.p;let mut integer=true;
        if matches!(self.ch(self.p),Some('+')|Some('-')){self.p+=1;}
        while self.ch(self.p).map(|c|c.is_ascii_digit()).unwrap_or(false){self.p+=1;}
        if self.ch(self.p)==Some('.')&&self.ch(self.p+1).map(|c|c.is_ascii_digit()).unwrap_or(false){integer=false;self.p+=2;while self.ch(self.p).map(|c|c.is_ascii_digit()).unwrap_or(false){self.p+=1;}}
        if matches!(self.ch(self.p),Some('e')|Some('E')){
            let mut next=self.p+1;if matches!(self.ch(next),Some('+')|Some('-')){next+=1;}
            if self.ch(next).map(|c|c.is_ascii_digit()).unwrap_or(false){integer=false;self.p=next+1;while self.ch(self.p).map(|c|c.is_ascii_digit()).unwrap_or(false){self.p+=1;}}
        }
        let repr=self.text(start,self.p);let value=repr.parse::<f64>().ok().filter(|n|n.is_finite());
        if value.is_none(){self.issue(start,"numeric-f64-range-unavailable-spelling-retained");}
        if value==Some(0.0)&&repr.split(['e','E']).next().unwrap_or("").chars().any(|c|matches!(c,'1'..='9')){self.issue(start,"numeric-f64-underflow-spelling-retained");}
        let number=Number{repr,value,integer};
        if self.ident_start(self.p){let unit=self.name();Kind::Dimension{number,unit}}
        else if self.ch(self.p)==Some('%'){self.p+=1;Kind::Percentage(number)}else{Kind::Number(number)}
    }
    fn string(&mut self,quote:char)->Kind {
        let start=self.p;self.p+=1;let mut value=String::new();
        loop{match self.ch(self.p){
            None=>{self.issue(start,"string-end-not-observed");return Kind::String(value);},
            Some(c) if c==quote=>{self.p+=1;return Kind::String(value);},
            Some('\n')=>{self.issue(self.p,"newline-in-string");return Kind::BadString(value);},
            Some('\\')=>{match self.ch(self.p+1){Some('\n')=>{self.p+=2;},None=>{self.p+=1;},_=>value.push(self.escape())}},
            Some(c)=>{value.push(c);self.p+=1;}
        }}
    }
    fn bad_url_tail(&mut self) {
        while let Some(c)=self.ch(self.p){if c==')'{self.p+=1;return;}if c=='\\'&&self.valid_escape(self.p){self.escape();}else{self.p+=1;}}
    }
    fn url(&mut self)->Kind {
        let start=self.p;let mut value=String::new();
        while self.ch(self.p).map(whitespace).unwrap_or(false){self.p+=1;}
        loop{match self.ch(self.p){
            Some(')')=>{self.p+=1;return Kind::Url(value);},
            None=>{self.issue(start,"url-end-not-observed");return Kind::Url(value);},
            Some(c) if whitespace(c)=>{
                while self.ch(self.p).map(whitespace).unwrap_or(false){self.p+=1;}
                if self.ch(self.p)==Some(')'){self.p+=1;return Kind::Url(value);}
                if self.ch(self.p).is_none(){self.issue(start,"url-end-not-observed");return Kind::Url(value);}
                self.issue(self.p,"nonterminal-url-whitespace");self.bad_url_tail();return Kind::BadUrl;
            },
            Some(c) if matches!(c,'\''|'"'|'(')||nonprintable(c)=>{self.issue(self.p,"invalid-unquoted-url-character");self.bad_url_tail();return Kind::BadUrl;},
            Some('\\')=>{if self.valid_escape(self.p){value.push(self.escape());}else{self.issue(self.p,"invalid-url-escape");self.bad_url_tail();return Kind::BadUrl;}},
            Some(c)=>{value.push(c);self.p+=1;}
        }}
    }
    fn ident_like(&mut self)->Kind {
        let value=self.name();
        if self.ch(self.p)!=Some('('){return Kind::Ident(value);}self.p+=1;
        if value.eq_ignore_ascii_case("url"){
            while self.ch(self.p).map(whitespace).unwrap_or(false)&&self.ch(self.p+1).map(whitespace).unwrap_or(false){self.p+=1;}
            let next=if self.ch(self.p).map(whitespace).unwrap_or(false){self.ch(self.p+1)}else{self.ch(self.p)};
            if !matches!(next,Some('"')|Some('\'')){return self.url();}
        }
        Kind::Function(value)
    }
    fn next(&mut self)->Kind {
        let c=self.ch(self.p).unwrap();
        if whitespace(c){let start=self.p;while self.ch(self.p).map(whitespace).unwrap_or(false){self.p+=1;}return Kind::Whitespace(self.text(start,self.p));}
        if self.begins("/*"){
            let start=self.p;self.p+=2;let body=self.p;while self.ch(self.p).is_some()&&!self.begins("*/"){self.p+=1;}
            let value=self.text(body,self.p);if self.ch(self.p).is_some(){self.p+=2;}else{self.issue(start,"comment-end-not-observed");}return Kind::Comment(value);
        }
        if matches!(c,'"'|'\''){return self.string(c);}
        if self.begins("<!--"){self.p+=4;return Kind::Cdo;}
        if self.begins("-->"){self.p+=3;return Kind::Cdc;}
        if self.number_start(self.p){return self.numeric();}
        if self.ident_start(self.p){return self.ident_like();}
        self.p+=1;
        match c{
            '@' if self.ident_start(self.p)=>Kind::AtKeyword(self.name()),
            '#' if self.ch(self.p).map(name_char).unwrap_or(false)||self.valid_escape(self.p)=>{let id=self.ident_start(self.p);Kind::Hash{value:self.name(),id}},
            '('|'['|'{'=>Kind::Open(c),')'|']'|'}'=>Kind::Close(c),':'=>Kind::Colon,';'=>Kind::Semicolon,','=>Kind::Comma,
            '\\'=>{self.issue(self.p-1,"invalid-escape-delimiter");Kind::Delim(c)},_=>Kind::Delim(c)
        }
    }
}

/// Tokenize a held UTF-8 prefix. Original bytes remain caller-owned. A partial
/// input's entire token stream is provisional and must be rescanned from byte
/// zero when extended. No incremental commit boundary is provided by this API.
pub(super) fn tokenize(data:&[u8],complete:bool)->SheetTokens {
    let cap=data.len().min(INPUT);let mut valid=cap;let mut utf_issue=None;
    if let Err(error)=std::str::from_utf8(&data[..cap]){valid=error.valid_up_to();utf_issue=Some(if error.error_len().is_none(){"partial-utf8-codepoint"}else{"invalid-utf8-stopped"});}
    let source=std::str::from_utf8(&data[..valid]).unwrap();
    let mut out=SheetTokens{tokens:Vec::new(),issues:Vec::new(),omitted_issues:0,supplied_bytes:data.len(),examined_bytes:valid,parsed_bytes:0,source_complete:complete&&valid==data.len(),stopped_by_bound:data.len()>INPUT};
    if cap<data.len(){out.issue(valid,"input-limit-prefix-only");}if let Some(code)=utf_issue{out.issue(valid,code);}if !complete{out.issue(valid,"caller-supplied-prefix");}
    let mut units:Vec<Unit>=Vec::with_capacity(source.chars().count());let mut chars=source.char_indices().peekable();
    while let Some((start,c))=chars.next(){let mut end=start+c.len_utf8();let normalized=match c{
        '\r'=>{if chars.peek().map(|(_,c)|*c)==Some('\n'){let (at,c)=chars.next().unwrap();end=at+c.len_utf8();}'\n'},
        '\u{c}'=>'\n','\0'=>{out.issue(start,"nul-preprocessed-to-replacement");'\u{fffd}'},_=>c};units.push(Unit{c:normalized,start,end});
    }
    let mut lexer=Lexer{units,p:0,out};let mut brackets=Vec::new();
    while lexer.p<lexer.units.len(){
        if lexer.out.tokens.len()>=TOKENS{lexer.issue(lexer.p,"token-limit");lexer.out.stopped_by_bound=true;break;}
        let start=lexer.p;let kind=lexer.next();let mut open=None;
        match &kind{Kind::Function(_)=>open=Some('('),Kind::Open(c)=>open=Some(*c),Kind::Close(c)=>{
            let expected=match c{')'=>'(',']'=>'[',_=>'{'};
            if brackets.last()==Some(&expected){brackets.pop();}else{lexer.issue(start,"unmatched-or-mismatched-closing-bracket");}
        },_=>{}}
        if let Some(c)=open{if brackets.len()>=DEPTH{lexer.issue(start,"bracket-depth-limit");lexer.out.stopped_by_bound=true;lexer.p=start;break;}brackets.push(c);}
        let end=lexer.units[lexer.p-1].end;
        let provisional=!lexer.out.source_complete;
        lexer.out.tokens.push(Token{kind,span:lexer.offset(start)..end,provisional});
    }
    lexer.out.parsed_bytes=lexer.offset(lexer.p);
    if !brackets.is_empty(){lexer.issue(lexer.p,"open-brackets-at-input-end-or-stop");}
    lexer.out
}

#[cfg(test)]mod checks{
    use super::*;
    fn kinds(source:&str)->Vec<Kind>{tokenize(source.as_bytes(),true).tokens.into_iter().filter(|t|!matches!(t.kind,Kind::Whitespace(_)|Kind::Comment(_))).map(|t|t.kind).collect()}
    #[test]fn identifiers_escapes_functions_and_hash_flags(){
        let k=kinds(r"@media #x #123 --custom -é f\6f o( .\31 23 ");
        assert_eq!(k[0],Kind::AtKeyword("media".into()));assert_eq!(k[1],Kind::Hash{value:"x".into(),id:true});assert_eq!(k[2],Kind::Hash{value:"123".into(),id:false});assert_eq!(k[3],Kind::Ident("--custom".into()));assert_eq!(k[4],Kind::Ident("-é".into()));assert_eq!(k[5],Kind::Function("foo".into()));assert_eq!(k[7],Kind::Ident("123".into()));
        assert_eq!(kinds(r"\0 \d800 \110000 "),vec![Kind::Ident("���".into())]);
    }
    #[test]fn numeric_values_flags_units_and_overflow(){
        let k=kinds("-1 +.5 1e2 5% 3.5px 2e-2em 1e+ 1e9999");
        assert!(matches!(&k[0],Kind::Number(n) if n.integer&&n.value==Some(-1.0)&&n.repr=="-1"));
        assert!(matches!(&k[1],Kind::Number(n) if !n.integer&&n.value==Some(0.5)));
        assert!(matches!(&k[2],Kind::Number(n) if !n.integer&&n.value==Some(100.0)));
        assert!(matches!(&k[3],Kind::Percentage(n) if n.value==Some(5.0)));
        assert!(matches!(&k[4],Kind::Dimension{number,unit} if number.value==Some(3.5)&&unit=="px"));
        assert!(matches!(&k[5],Kind::Dimension{number,unit} if number.value==Some(0.02)&&unit=="em"));
        assert!(matches!(&k[6],Kind::Dimension{unit,..} if unit=="e"));assert_eq!(k[7],Kind::Delim('+'));
        assert!(matches!(&k[8],Kind::Number(n) if n.value.is_none()&&n.repr=="1e9999"));
    }
    #[test]fn strings_bad_strings_preprocessing_and_comments(){
        let d=tokenize(b"/* comment */ 'a\\\r\nb' \"x\ny\" \0\x0c",true);
        assert!(matches!(&d.tokens[0].kind,Kind::Comment(v) if v==" comment "));
        assert!(d.tokens.iter().any(|t|matches!(&t.kind,Kind::String(v) if v=="ab")));
        assert!(d.tokens.iter().any(|t|matches!(&t.kind,Kind::BadString(v) if v=="x")));
        assert!(d.issues.iter().any(|i|i.code=="nul-preprocessed-to-replacement"));
        let d=tokenize(b"a\r\nb\rc\x0cd",true);assert_eq!(d.tokens[1].span,1..3);assert_eq!(d.tokens[1].kind,Kind::Whitespace("\n".into()));
    }
    #[test]fn quoted_and_unquoted_urls_are_not_confused(){
        let k=kinds(r#"url(foo\ bar) url( "secret") URL(a b) url(a\)b) url("#);
        assert_eq!(k[0],Kind::Url("foo bar".into()));assert_eq!(k[1],Kind::Function("url".into()));assert_eq!(k[2],Kind::String("secret".into()));assert_eq!(k[3],Kind::Close(')'));assert_eq!(k[4],Kind::BadUrl);assert_eq!(k[5],Kind::Url("a)b".into()));assert_eq!(k[6],Kind::Url("".into()));
        assert!(!tokenize(b"url(secret)",true).summary().contains("secret"));
    }
    #[test]fn punctuation_comments_and_complete_vs_prefix(){
        assert_eq!(kinds("<!-- --> : ; , [ ] { } ( )"),vec![Kind::Cdo,Kind::Cdc,Kind::Colon,Kind::Semicolon,Kind::Comma,Kind::Open('['),Kind::Close(']'),Kind::Open('{'),Kind::Close('}'),Kind::Open('('),Kind::Close(')')]);
        for text in ["/*x","'x","url(x","\\", "abc", "1e"]{let d=tokenize(text.as_bytes(),false);assert!(!d.source_complete);assert!(d.tokens.last().unwrap().provisional);}
        let d=tokenize(b"a\xffb",true);assert_eq!(d.examined_bytes,1);assert!(!d.source_complete);assert!(d.tokens[0].provisional);
    }
    #[test]fn bounds_and_original_unicode_spans(){
        let source="/*é*/ 日本{--é: '\\1f600 '; width:1.25em}\r\n";
        for end in 0..=source.len(){let d=tokenize(&source.as_bytes()[..end],false);assert!(d.parsed_bytes<=d.examined_bytes);let mut last=0;for t in &d.tokens{assert_eq!(t.span.start,last);assert!(source.get(t.span.clone()).is_some());last=t.span.end;}assert_eq!(last,d.parsed_bytes);}
        let d=tokenize("(".repeat(DEPTH+2).as_bytes(),true);assert!(d.stopped_by_bound);assert_eq!(d.tokens.len(),DEPTH);
        let d=tokenize(";".repeat(TOKENS+1).as_bytes(),true);assert!(d.stopped_by_bound);assert_eq!(d.tokens.len(),TOKENS);
        let d=tokenize(&vec![b'x';INPUT+1],true);assert_eq!(d.examined_bytes,INPUT);assert!(!d.source_complete);assert!(d.tokens[0].provisional);
    }
    #[test]fn generated_token_sequences_and_underflow(){
        let pieces=["/*", "*/", "é", "\\", "\n", "\r\n", "\0", "url(", "'", "\"", ")", "#", "--", "1e-", "9999", " ", "{", "[", ";", "\\31 ", "a"];
        let mut seed=0xa55a5aa5u64;
        for _ in 0..2048 {let mut source=String::new();for _ in 0..48{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);source.push_str(pieces[(seed>>32) as usize%pieces.len()]);}
            let d=tokenize(source.as_bytes(),true);let mut end=0;for token in &d.tokens{assert_eq!(end,token.span.start);assert!(source.get(token.span.clone()).is_some());assert!(token.span.end>token.span.start);end=token.span.end;}assert_eq!(end,d.parsed_bytes);assert!(d.tokens.len()<=TOKENS);assert!(d.issues.len()<=ISSUES);assert!(d.summary().len()<8192);
        }
        let d=tokenize(b"1e-9999",true);assert!(d.issues.iter().any(|i|i.code=="numeric-f64-underflow-spelling-retained"));
    }

    #[test]fn partial_input_requires_rescanning_every_token(){
        for (prefix,extended) in [("1.","1.2"),("1e+","1e+2"),("<!","<!--"),("-", "--x")]{
            let partial=tokenize(prefix.as_bytes(),false);
            assert!(!partial.tokens.is_empty());assert!(partial.tokens.iter().all(|t|t.provisional));
            let full=tokenize(extended.as_bytes(),true);
            assert!(full.tokens.iter().all(|t|!t.provisional));
            assert_ne!(partial.tokens[0].kind,full.tokens[0].kind);
        }
        let truncated=tokenize(b"a ; 1.\xff",true);
        assert!(!truncated.source_complete);assert!(truncated.tokens.iter().all(|t|t.provisional));
        let capped=tokenize(&vec![b';';INPUT+1],true);
        assert!(!capped.source_complete);assert!(capped.tokens.iter().all(|t|t.provisional));
    }

}
