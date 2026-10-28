//! Display projection for explicit leading reasoning blocks. Raw model output is
//! retained by the caller and remains the sole input to proposal recognition.
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
#[cfg(test)]mod checks {
    use super::*;
    #[test]fn leading_blocks_and_unicode(){
        let raw=" \n<think>秘密 🦀</think>\n<think>more</think>\nAnswer 🦀";
        assert_eq!(project(raw,false),Projection{text:"Answer 🦀",hidden:true,pending:false});
        assert_eq!(project("<think>a<think>b</think>c</think>done",false).text,"done");
        assert_eq!(project("<think>x</think>",false).text,"");
    }
    #[test]fn every_stream_boundary(){
        let raw="<think>SECRET 🦀</think>Final";
        for i in (0..=raw.len()).filter(|i|raw.is_char_boundary(*i)){
            let p=project(&raw[..i],true);assert!(!p.text.contains("SECRET"));assert!(!p.text.contains("<think>"));
            if i>0&&i<"<think>SECRET 🦀</think>".len(){assert!(p.pending);}
        }
        assert_eq!(project("<thin",false).text,"<thin");
        assert_eq!(project("<thing",true).text,"<thing");
        assert_eq!(project("<think>unfinished",false),Projection{text:"",hidden:true,pending:true});
    }
    #[test]fn literal_examples_and_prose_are_not_guessed(){
        for raw in ["`<think>example</think>`","```xml\n<think>example</think>\n```","> <think>example</think>","\"<think>example</think>\"","'<think>example</think>'","“<think>example</think>”","Use <think>example</think> tags.","I should consider the question first.","<THINK>literal</THINK>","<think class='x'>literal</think>","Answer\n<think>literal</think>"] {assert_eq!(project(raw,false),Projection{text:raw,hidden:false,pending:false});}
    }
    #[test]fn no_tool_extraction_and_bounded_work(){
        let raw="<think>{\"tool\":\"write\",\"args\":{}}</think>Answer";
        assert_eq!(project(raw,false).text,"Answer");assert!(raw.starts_with("<think>"));
        let large=format!("<think>{}</think>done","🦀".repeat(262144));assert_eq!(project(&large,false).text,"done");
    }
}
