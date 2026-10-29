//! Original bounded source-order HTML document foundation. Not a browser DOM.
//! No network, JavaScript, CSS evaluation, layout or rendering occurs here.
//! HTML insertion modes, implied elements/end tags, foreign namespaces, script
//! escaped states and the full named-character-reference table remain unimplemented.
use std::ops::Range;
const INPUT: usize = 256 * 1024;
const NODES: usize = 4096;
const DEPTH: usize = 64;
const ATTRS: usize = 32;
const NAME: usize = 256;
const VALUE: usize = 8192;
const ISSUES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Attribute {
    pub name: String,
    pub value: Option<String>,
    pub span: Range<usize>,
    pub value_span: Option<Range<usize>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TextMode { Normal, RcData, Script, Style, Raw, Plain }
// Raw script/style data is deliberately available only in the structured tree;
// the observation summary below never emits node data or attribute values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Document,
    Element { name: String, attributes: Vec<Attribute>, void: bool },
    Text { mode: TextMode, data: String },
    Comment { data: String },
    Doctype { declaration: String },
    UnknownMarkup,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub kind: Kind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Original UTF-8 source byte range. Element span is its start tag only.
    pub span: Range<usize>,
    pub closing_span: Option<Range<usize>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Issue { pub offset: usize, pub code: &'static str }
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResourceKind { Link, Image, Form, Control, Script, Stylesheet, LinkElement, Base }
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Resource {
    pub node: usize,
    pub kind: ResourceKind,
    /// Declared attribute only: not a resolved URL or authorization to fetch.
    pub target: Option<String>,
    pub target_span: Option<Range<usize>>,
}
#[derive(Clone, Debug)]
pub(super) struct Document {
    pub nodes: Vec<Node>,
    pub resources: Vec<Resource>,
    pub issues: Vec<Issue>,
    pub omitted_issues: usize,
    pub supplied_bytes: usize,
    pub examined_bytes: usize,
    pub parsed_bytes: usize,
    pub source_complete: bool,
    pub stopped_by_bound: bool,
    allocation_limit: usize,
    allocation_used: usize,
    allocation_stopped: bool,
}
// Conservative per-owned-item envelopes cover vector minimum/geometric growth,
// temporary realloc overlap and allocation metadata without a separate byte scan.
pub(super) fn node_bytes(kind:&Kind)->usize{
 let payload=match kind{Kind::Element{name,..}=>name.capacity(),Kind::Text{data,..}|Kind::Comment{data}=>data.capacity(),Kind::Doctype{declaration}=>declaration.capacity(),_=>0};
 4*std::mem::size_of::<Node>()+128+payload
}
impl Document {
    fn admit(&mut self,bytes:usize)->bool{
        if self.allocation_stopped{return false;}
        if let Some(total)=self.allocation_used.checked_add(bytes).filter(|n|*n<=self.allocation_limit){self.allocation_used=total;true}else{self.allocation_stopped=true;self.stopped_by_bound=true;false}
    }
    pub(super) fn allocation_bytes(&self)->usize{self.allocation_used}
    pub(super) fn allocation_refused(&self)->bool{self.allocation_stopped}
    fn issue(&mut self, offset: usize, code: &'static str) {
        if self.issues.len() < ISSUES { if !self.admit(4*std::mem::size_of::<Issue>()+64){self.omitted_issues+=1;return;} self.issues.push(Issue { offset, code }); }
        else { self.omitted_issues += 1; }
    }
    fn add(&mut self, parent: usize, kind: Kind, span: Range<usize>) -> Option<usize> {
        if self.nodes.len() >= NODES { self.issue(span.start, "node-limit"); self.stopped_by_bound = true; return None; }
        if !self.admit(node_bytes(&kind)){return None;}
        let index = self.nodes.len();
        self.nodes.push(Node {kind,parent:Some(parent),children:Vec::new(),span,closing_span:None});
        self.nodes[parent].children.push(index); Some(index)
    }
    /// Counts only: no script contents, URL credentials, form values or text data.
    pub fn summary(&self) -> String {
        let mut elements=0;let mut texts=0;let mut scripts=0;let mut styles=0;
        for node in &self.nodes {match &node.kind {
            Kind::Element{..}=>elements+=1,
            Kind::Text{mode,..}=>{texts+=1;if *mode==TextMode::Script{scripts+=1;}if *mode==TextMode::Style{styles+=1;}},_=>{}
        }}
        format!("Native HTML source-order tree: {} supplied bytes; {} examined; {} parsed; source complete: {}; stopped by bound: {}. {} elements, {} text nodes, {} script-data nodes, {} style-data nodes, {} declared resources. {} recorded issues; {} further issues omitted. UTF-8 only; numeric references and amp/lt/gt/quot/apos/nbsp with semicolons decoded. This is a lexical nesting tree, not the HTML insertion-mode DOM. This source-tree parsing stage executes no scripts, fetches no URLs, evaluates no styles and produces no layout or screenshots; preceding program stages, if selected, are described separately.",self.supplied_bytes,self.examined_bytes,self.parsed_bytes,self.source_complete,self.stopped_by_bound,elements,texts,scripts,styles,self.resources.len(),self.issues.len(),self.omitted_issues)
    }
}
fn space(b:u8)->bool { matches!(b,b' '|b'\t'|b'\n'|b'\r'|12) }
fn starts(bytes:&[u8],at:usize,needle:&[u8])->bool { bytes.get(at..at.saturating_add(needle.len())).map(|s|s.eq_ignore_ascii_case(needle)).unwrap_or(false) }
fn void(name:&str)->bool {matches!(name,"area"|"base"|"br"|"col"|"embed"|"hr"|"img"|"input"|"link"|"meta"|"param"|"source"|"track"|"wbr")}
fn raw_mode(name:&str)->Option<TextMode>{match name{"script"=>Some(TextMode::Script),"style"=>Some(TextMode::Style),"title"|"textarea"=>Some(TextMode::RcData),"xmp"|"iframe"|"noembed"|"noframes"=>Some(TextMode::Raw),"plaintext"=>Some(TextMode::Plain),_=>None}}
fn end_raw(bytes:&[u8],from:usize,name:&str)->Option<usize>{
    let mut p=from;
    while p<bytes.len(){if bytes[p]==b'<'&&starts(bytes,p,b"</")&&starts(bytes,p+2,name.as_bytes()){
        let end=p+2+name.len();if bytes.get(end).map(|b|space(*b)||matches!(*b,b'>'|b'/')).unwrap_or(false){return Some(p);}
    }p+=1;}None
}
/// Semicolon-terminated subset only. Unknown references are retained verbatim.
fn decode(text:&str,base:usize,doc:&mut Document)->String{
    let bytes=text.as_bytes();let mut result=String::with_capacity(text.len());let mut p=0;let mut copied=0;
    while p<bytes.len(){if bytes[p]!=b'&'{p+=1;continue;}
        let limit=bytes.len().min(p+34);let end=bytes[p+1..limit].iter().position(|b|*b==b';').map(|n|p+1+n);
        if let Some(end)=end {let token=&text[p+1..end];let value=match token {
            "amp"=>Some('&'),"lt"=>Some('<'),"gt"=>Some('>'),"quot"=>Some('"'),"apos"=>Some('\''),"nbsp"=>Some('\u{a0}'),
            _=>if let Some(num)=token.strip_prefix('#'){
                let (digits,radix)=if let Some(hex)=num.strip_prefix('x').or_else(||num.strip_prefix('X')){(hex,16)}else{(num,10)};
                let number=if !digits.is_empty()&&digits.bytes().all(|b|if radix==16{b.is_ascii_hexdigit()}else{b.is_ascii_digit()}){u32::from_str_radix(digits,radix).ok()}else{None};
                match number {Some(n) if n!=0 && !(0xd800..=0xdfff).contains(&n) && n<=0x10ffff=>{
                    if (0x80..=0x9f).contains(&n){doc.issue(base+p,"numeric-reference-legacy-remapping-unimplemented");None}else{char::from_u32(n)}
                },_=>{doc.issue(base+p,"invalid-numeric-reference");Some('\u{fffd}')}}
            }else{None},
        };
        if let Some(value)=value {result.push_str(&text[copied..p]);result.push(value);p=end+1;copied=p;continue;}
        doc.issue(base+p,"unsupported-character-reference-retained");
        }else {doc.issue(base+p,"unterminated-or-overlong-reference-retained");}p+=1;
    }result.push_str(&text[copied..]);result
}
fn metadata(doc:&mut Document,node:usize){
    let (name,attributes)=match &doc.nodes[node].kind{Kind::Element{name,attributes,..}=>(name,attributes),_=>return};
    let (kind,key)=match name.as_str(){"a"|"area"=>(ResourceKind::Link,"href"),"img"|"source"=>(ResourceKind::Image,"src"),"form"=>(ResourceKind::Form,"action"),"input"|"button"|"select"|"textarea"=>(ResourceKind::Control,"name"),"script"=>(ResourceKind::Script,"src"),"link"=>(if attributes.iter().any(|a|a.name=="rel"&&a.value.as_deref().map(|v|v.split_ascii_whitespace().any(|r|r.eq_ignore_ascii_case("stylesheet"))).unwrap_or(false)){ResourceKind::Stylesheet}else{ResourceKind::LinkElement},"href"),"base"=>(ResourceKind::Base,"href"),_=>return};
    // Declared relationship only; no link or stylesheet has been fetched.
    let attr=attributes.iter().find(|a|a.name==key);
    let resource=Resource{node,kind,target:attr.and_then(|a|a.value.clone()),target_span:attr.and_then(|a|a.value_span.clone())};
    if doc.admit(4*std::mem::size_of::<Resource>()+128+resource.target.as_ref().map(String::capacity).unwrap_or(0)){doc.resources.push(resource);}
}

/// Parse a bounded prefix into explicit source-order nesting. `complete` is the
/// caller's observation, not proof of a fetched resource's authenticity.
pub(super) fn parse(data:&[u8],complete:bool)->Document{parse_with_room(data,complete,usize::MAX)}
/// Bounds retained owned tree envelopes; callers reserve input and bounded
/// decoding/construction scratch separately before invoking this parser.
pub(super) fn parse_with_room(data:&[u8],complete:bool,room:usize)->Document{
    let mut count=data.len().min(INPUT);
    let utf=std::str::from_utf8(&data[..count]);
    let mut invalid=None;
    if let Err(error)=utf{count=error.valid_up_to();invalid=Some((count,error.error_len().is_none()));}
    let source=std::str::from_utf8(&data[..count]).unwrap();let bytes=source.as_bytes();
    let mut doc=Document{nodes:Vec::new(),resources:Vec::new(),issues:Vec::new(),omitted_issues:0,supplied_bytes:data.len(),examined_bytes:count,parsed_bytes:0,source_complete:complete&&count==data.len(),stopped_by_bound:data.len()>INPUT,allocation_limit:room,allocation_used:0,allocation_stopped:false};
    if !doc.admit(4*std::mem::size_of::<Node>()+128){return doc;}
    doc.nodes.push(Node{kind:Kind::Document,parent:None,children:Vec::new(),span:0..count,closing_span:None});
    if data.len()>INPUT{doc.issue(count,"input-limit-prefix-only");}
    if let Some((offset,incomplete))=invalid{doc.issue(offset,if incomplete{"partial-utf8-codepoint"}else{"invalid-utf8-stopped"});}
    if !complete{doc.issue(count,"caller-supplied-prefix");}
    if let Some(n)=source.find('\0'){doc.issue(n,"nul-retained-preprocessing-unimplemented");}
    if let Some(n)=source.find('\r'){doc.issue(n,"cr-retained-preprocessing-unimplemented");}
    let mut stack=vec![0usize];let mut p=0;
    while p<count&&!doc.allocation_stopped {
        let parent=*stack.last().unwrap();
        let raw=match &doc.nodes[parent].kind{Kind::Element{name,..}=>raw_mode(name).map(|mode|(name.clone(),mode)),_=>None};
        if let Some((name,mode))=raw{
            let end=if mode==TextMode::Plain{None}else{end_raw(bytes,p,&name)}.unwrap_or(count);
            if end>p{
                let original=&source[p..end];
                if mode==TextMode::Script&&(original.contains("<!--")||original.as_bytes().windows(7).any(|w|w.eq_ignore_ascii_case(b"<script"))){doc.issue(p,"script-escaped-states-unimplemented");}
                let content=if mode==TextMode::RcData{decode(original,p,&mut doc)}else{original.to_string()};
                if doc.add(parent,Kind::Text{mode:mode.clone(),data:content},p..end).is_none(){break;}p=end;
            }
            if p==count{if mode!=TextMode::Plain{doc.issue(p,"raw-element-end-not-observed");}break;}
            // A matching closing token begins at p; ordinary closing handling follows.
        }
        if bytes[p]!=b'<'{
            let end=bytes[p..].iter().position(|b|*b==b'<').map(|n|p+n).unwrap_or(count);
            let content=decode(&source[p..end],p,&mut doc);
            if doc.add(parent,Kind::Text{mode:TextMode::Normal,data:content},p..end).is_none(){break;}p=end;continue;
        }
        if starts(bytes,p,b"<!--"){
            let end=bytes[p+4..].windows(3).position(|b|b==b"-->").map(|n|p+4+n);
            let finish=end.map(|n|n+3).unwrap_or(count);
            if end.is_none(){doc.issue(p,"comment-end-not-observed");}
            if doc.add(parent,Kind::Comment{data:source[p+4..end.unwrap_or(count)].to_string()},p..finish).is_none(){break;}p=finish;continue;
        }
        if starts(bytes,p,b"<!")||starts(bytes,p,b"<?"){
            let mut q=p+2;let mut quote=None;
            while q<count{let b=bytes[q];if let Some(c)=quote{if b==c{quote=None;}}else if matches!(b,b'\''|b'"'){quote=Some(b);}else if b==b'>'{break;}q+=1;}
            if q==count{doc.issue(p,"declaration-end-not-observed");break;}
            let is_doctype=starts(bytes,p,b"<!doctype")&&bytes.get(p+9).map(|b|space(*b)||*b==b'>').unwrap_or(false);
            let kind=if is_doctype{Kind::Doctype{declaration:source[p+2..q].to_string()}}else{doc.issue(p,"unsupported-markup-declaration");Kind::UnknownMarkup};
            if doc.add(parent,kind,p..q+1).is_none(){break;}p=q+1;continue;
        }
        let closing=starts(bytes,p,b"</");let start=p+if closing{2}else{1};
        if !bytes.get(start).map(|b|b.is_ascii_alphabetic()).unwrap_or(false){
            if doc.add(parent,Kind::Text{mode:TextMode::Normal,data:"<".into()},p..p+1).is_none(){break;}p+=1;continue;
        }
        let mut q=start;
        while q<count&&!space(bytes[q])&&!matches!(bytes[q],b'/'|b'>'){q+=1;}
        if q-start>NAME{doc.issue(p,"tag-name-limit");doc.stopped_by_bound=true;break;}
        let name=source[start..q].to_ascii_lowercase();
        if closing{
            while q<count&&space(bytes[q]){q+=1;}
            if q==count{doc.issue(p,"closing-tag-end-not-observed");break;}
            if bytes[q]!=b'>'{doc.issue(p,"malformed-closing-tag");while q<count&&bytes[q]!=b'>'{q+=1;}if q==count{break;}}
            if let Some(position)=stack.iter().rposition(|id|matches!(&doc.nodes[*id].kind,Kind::Element{name:n,..} if *n==name)){
                if position+1!=stack.len(){doc.issue(p,"mismatched-nesting-popped");}
                let id=stack[position];doc.nodes[id].closing_span=Some(p..q+1);stack.truncate(position);
            }else{doc.issue(p,"unmatched-closing-tag");}
            p=q+1;continue;
        }
        let mut attributes=Vec::new();let mut slash=false;let mut finished=false;let mut failed=false;
        while q<count{
            while q<count&&space(bytes[q]){q+=1;}
            if q==count{break;}
            if bytes[q]==b'>'{q+=1;finished=true;break;}
            if bytes[q]==b'/'&&bytes.get(q+1)==Some(&b'>'){slash=true;q+=2;finished=true;break;}
            let a=q;while q<count&&!space(bytes[q])&&!matches!(bytes[q],b'='|b'>'|b'/'){q+=1;}
            if q==a{doc.issue(q,"invalid-attribute-delimiter");q+=1;continue;}
            if q-a>NAME||attributes.len()>=ATTRS{doc.issue(a,"attribute-count-or-name-limit");doc.stopped_by_bound=true;failed=true;break;}
            let attr_name=source[a..q].to_ascii_lowercase();
            if attr_name.contains(['\'','"','<']){doc.issue(a,"invalid-attribute-name-character");}
            while q<count&&space(bytes[q]){q+=1;}
            let mut value=None;let mut value_span=None;
            if bytes.get(q)==Some(&b'='){
                q+=1;while q<count&&space(bytes[q]){q+=1;}
                let quote=bytes.get(q).copied().filter(|b|matches!(b,b'\''|b'"'));
                if quote.is_some(){q+=1;}let v=q;
                while q<count{if let Some(c)=quote{if bytes[q]==c{break;}}else if space(bytes[q])||bytes[q]==b'>'{break;}q+=1;}
                if q-v>VALUE{doc.issue(v,"attribute-value-limit");doc.stopped_by_bound=true;failed=true;break;}
                if quote.is_some()&&q==count{doc.issue(v,"attribute-quote-end-not-observed");failed=true;break;}
                if quote.is_none()&&source[v..q].contains(['"','\'','<','=','`']){doc.issue(v,"invalid-unquoted-value-character");}
                value=Some(decode(&source[v..q],v,&mut doc));value_span=Some(v..q);if quote.is_some(){q+=1;}
            }
            if attributes.iter().any(|old:&Attribute|old.name==attr_name){doc.issue(a,"duplicate-attribute-first-retained");}
            else{let bytes=4*std::mem::size_of::<Attribute>()+128+attr_name.capacity()+value.as_ref().map(String::capacity).unwrap_or(0);if !doc.admit(bytes){failed=true;break;}attributes.push(Attribute{name:attr_name,value,span:a..q,value_span});}
        }
        if failed{break;}if !finished{doc.issue(p,"start-tag-end-not-observed");break;}
        let is_void=void(&name);
        if !is_void&&stack.len()>DEPTH{doc.issue(p,"element-depth-limit");doc.stopped_by_bound=true;break;}
        if slash&&!is_void{doc.issue(p,"nonvoid-self-closing-marker-ignored");}
        if matches!(name.as_str(),"svg"|"math"|"template"|"table"|"select"|"noscript"){doc.issue(p,"special-tree-construction-unimplemented");}
        let id=match doc.add(parent,Kind::Element{name,attributes,void:is_void},p..q){Some(id)=>id,None=>break};
        metadata(&mut doc,id);if !is_void{stack.push(id);}p=q;
    }
    doc.parsed_bytes=p;
    if stack.len()>1{doc.issue(p,"open-elements-at-input-end-or-stop");}
    doc
}

#[cfg(test)]mod checks{
    use super::*;
    #[test]fn structured_tree_attributes_and_resources(){
        let input=b"<!doctype html><html><body><a href='a?x=1&amp;y=2'>Hi &#x1f600;</a><img src=x><form action=/send><input name=q></form></body></html>";
        let d=parse(input,true);assert!(d.source_complete);assert_eq!(d.parsed_bytes,input.len());assert!(d.issues.is_empty(),"{:?}",d.issues);
        assert_eq!(d.resources.len(),4);assert_eq!(d.resources[0].target.as_deref(),Some("a?x=1&y=2"));
        let a=d.resources[0].node;assert!(d.nodes[a].closing_span.is_some());let t=d.nodes[a].children[0];assert_eq!(d.nodes[t].parent,Some(a));assert!(matches!(&d.nodes[t].kind,Kind::Text{data,..} if data=="Hi 😀"));
        for n in &d.nodes{assert!(n.span.end<=input.len());if let Some(parent)=n.parent{assert!(d.nodes[parent].children.contains(&d.nodes.iter().position(|x|std::ptr::eq(x,n)).unwrap()));}}
    }
    #[test]fn raw_text_and_rcdata_do_not_create_fake_elements(){
        let d=parse(b"<script>if(x < y) { s='<img src=secret>'; }</SCRIPT ><style>a{content:'<a>'}</style><textarea>&lt;b&gt;<img></textarea>",true);
        assert_eq!(d.resources.len(),2); // script and textarea control only
        assert!(!d.nodes.iter().any(|n|matches!(&n.kind,Kind::Element{name,..} if name=="img"||name=="a")));
        assert!(d.nodes.iter().any(|n|matches!(&n.kind,Kind::Text{mode:TextMode::RcData,data} if data=="<b><img>")));
        assert!(!d.summary().contains("secret"));
    }
    #[test]fn comments_quotes_duplicates_and_self_closing(){
        let d=parse(b"<!-- <img> --><div X='>' x=second/><br><i>a</div>",true);
        assert!(d.issues.iter().any(|i|i.code=="duplicate-attribute-first-retained"));assert!(d.issues.iter().any(|i|i.code=="mismatched-nesting-popped"));
        let div=d.nodes.iter().find(|n|matches!(&n.kind,Kind::Element{name,..} if name=="div")).unwrap();
        assert!(matches!(&div.kind,Kind::Element{attributes,..} if attributes.len()==1&&attributes[0].value.as_deref()==Some(">")));
        let d=parse(b"<div/><p>x",true);assert!(d.issues.iter().any(|i|i.code=="nonvoid-self-closing-marker-ignored"));
    }
    #[test]fn unicode_entities_and_invalid_input(){
        let d=parse("<p title='é'>&amp;&unknown;&#0;&#xD800;&#128;日本</p>".as_bytes(),true);
        assert!(d.nodes.iter().any(|n|matches!(&n.kind,Kind::Text{data,..} if data=="&&unknown;��&#128;日本")));
        assert!(d.issues.iter().any(|i|i.code=="numeric-reference-legacy-remapping-unimplemented"));
        let d=parse(b"<p>x\xff</p>",true);assert!(!d.source_complete);assert_eq!(d.examined_bytes,4);assert_eq!(d.parsed_bytes,4);
        let d=parse(b"abc\xe2\x82",false);assert_eq!(d.examined_bytes,3);assert!(d.issues.iter().any(|i|i.code=="partial-utf8-codepoint"));
    }
    #[test]fn incomplete_tokens_and_prefix_are_explicit(){
        for input in [b"<a href='x".as_slice(),b"<!-- x",b"<!doctype 'x",b"<script>x",b"<a"]{let d=parse(input,false);assert!(!d.source_complete);assert!(!d.issues.is_empty());assert!(d.parsed_bytes<=input.len());}
        let d=parse(b"<script>1</scripture><a>x</a>",true);assert_eq!(d.nodes.len(),3);assert!(d.issues.iter().any(|i|i.code=="raw-element-end-not-observed"));
    }
    #[test]fn resource_and_depth_bounds(){
        let d=parse("<i>".repeat(DEPTH+4).as_bytes(),true);assert!(d.stopped_by_bound);assert!(d.nodes.len()<=DEPTH+1);
        let d=parse("<br>".repeat(NODES+1).as_bytes(),true);assert_eq!(d.nodes.len(),NODES);assert!(d.stopped_by_bound);
        let d=parse(format!("<a href='{}'>", "x".repeat(VALUE+1)).as_bytes(),true);assert!(d.stopped_by_bound);assert_eq!(d.nodes.len(),1);
        let d=parse(&vec![b'a';INPUT+1],true);assert!(!d.source_complete);assert_eq!(d.examined_bytes,INPUT);assert!(d.stopped_by_bound);
    }
    #[test]fn all_prefixes_are_bounded_and_source_spans_remain_valid(){
        let input="<!doctype html><!-- é --><div title='a&amp;b'><script>let x='<a>';</script><p>日本&#35;</div>".as_bytes();
        for end in 0..=input.len(){let d=parse(&input[..end],false);assert!(d.parsed_bytes<=d.examined_bytes);assert!(d.nodes.len()<=NODES);for n in &d.nodes{assert!(n.span.start<=n.span.end&&n.span.end<=d.examined_bytes);assert!(std::str::from_utf8(&input[n.span.clone()]).is_ok());}}
    }
    #[test]fn generated_token_inputs_remain_bounded_and_relationships_match(){
        let pieces=["<a x='", "日本", "'>", "</a>", "<script>", "</script>", "&", "&#x", "1f600;", "<!--", "-->", "<", "\0", ">", " ", "/", "<link href=x>"];
        let mut seed=0x12345678u64;
        for _ in 0..2048 {let mut text=String::new();for _ in 0..48{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);text.push_str(pieces[(seed>>32) as usize%pieces.len()]);}
            let d=parse(text.as_bytes(),true);assert!(d.parsed_bytes<=d.examined_bytes);assert!(d.nodes.len()<=NODES);assert!(d.issues.len()<=ISSUES);
            for (i,n) in d.nodes.iter().enumerate(){assert!(text.get(n.span.clone()).is_some());if let Some(parent)=n.parent{assert!(parent<i);assert!(d.nodes[parent].children.contains(&i));}if let Some(span)=&n.closing_span{assert!(text.get(span.clone()).is_some());}}
        }
        let d=parse(b"<link href=x><link rel='StyleSheet alternate' href=y>",true);assert_eq!(d.resources[0].kind,ResourceKind::LinkElement);assert_eq!(d.resources[1].kind,ResourceKind::Stylesheet);
    }

}
