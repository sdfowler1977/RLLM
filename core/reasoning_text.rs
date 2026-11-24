//! Display projection for explicit leading reasoning blocks. Raw model output is
//! retained by the caller and remains the input to automatic proposal recognition.
//! This is deliberately not a prose classifier or an HTML/Markdown parser.
#[derive(Debug,PartialEq,Eq)]
pub(super) struct Projection<'a> {
    pub text: &'a str,
    pub hidden: bool,
    pub pending: bool,
}
/// Recognize only exact, lowercase `<think>` blocks at the start of a model
/// response, after optional whitespace. Quoted, fenced and explanatory examples
/// therefore stay literal. Consecutive leading blocks are supported. Once answer
/// text begins, later tags are literal. An unfinished recognized block stays
/// hidden even after an interrupted/final reply; no invented answer replaces it.
/// Streaming may end midway through the opening marker: withhold that prefix
/// until the cumulative input resolves it. No transformed string is executable.
pub(super) fn project(raw:&str, streaming:bool)->Projection<'_> {
    const OPEN:&str="<think>"; const CLOSE:&str="</think>";
    let mut cursor=0;let mut hidden=false;
    loop {
        let rest=&raw[cursor..];let trimmed=rest.trim_start();let start=raw.len()-trimmed.len();
        if trimmed.starts_with(OPEN) {
            hidden=true;
            // Nesting is only marker balancing, never executable markup. Scan
            // once, with constant memory and at most input.len() byte work.
            let mut depth=1usize;let mut p=start+OPEN.len();let b=raw.as_bytes();
            while p<b.len() {
                if b[p]==b'<' && raw[p..].starts_with(OPEN) {depth+=1;p+=OPEN.len();}
                else if b[p]==b'<' && raw[p..].starts_with(CLOSE) {depth-=1;p+=CLOSE.len();if depth==0{break;}}
                else {p+=1;}
            }
            if depth!=0{return Projection{text:"",hidden:true,pending:true};}
            cursor=p;continue;
        }
        if streaming && !trimmed.is_empty() && OPEN.starts_with(trimmed) {
            return Projection{text:"",hidden,pending:true};
        }
        return Projection{text:if hidden{trimmed}else{raw},hidden,pending:false};
    }
}
