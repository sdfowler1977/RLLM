//! A bounded planning projection; raw source observations remain in the journal.
use super::*;

const LIMIT:usize=256*1024;
struct Document{body:String}
impl Document{
 fn add(&mut self,value:&str)->Result<()>{if self.body.len().saturating_add(value.len())>LIMIT{return Err("Design brief exceeds256KiB; use the complete journal or reduce authored text".into());}self.body.push_str(value);Ok(())}
 fn character(&mut self,value:char)->Result<()>{if self.body.len().saturating_add(value.len_utf8())>LIMIT{return Err("Design brief exceeds256KiB; use the complete journal or reduce authored text".into());}self.body.push(value);Ok(())}
 fn quote(&mut self,value:&str)->Result<()>{
  for line in value.split('\n'){
   self.add("> ")?;
   for ch in line.chars(){match ch{
    '&'=>self.add("&amp;")?,'<'=>self.add("&lt;")?,'>'=>self.add("&gt;")?,
    '\\'|'`'|'*'|'_'|'['|']'|'!'|'#'|'|'=>{self.add("\\")?;self.character(ch)?;},
    c if c.is_control()=>self.add(&format!("\\u{{{:x}}}",c as u32))?,
    c=>self.character(c)?
   }}
   self.add("\n")?;
  }
  self.add("\n")
 }
 fn list(&mut self,values:&[Json])->Result<()>{if values.is_empty(){self.add("No items recorded.\n\n")?;}else{for(i,value)in values.iter().enumerate(){self.add(&format!("Item {}:\n\n",i+1))?;self.quote(value.text()?)?;}}Ok(())}
}

pub(super) fn content(c:&Json)->Result<String>{
 let mut out=Document{body:String::new()};
 out.add("# Design brief\n\nThis planning projection contains recorded user/model-authored text and references. It omits raw returned source/output bodies and transport arguments. Authored text may itself quote source material; this document does not establish separate analysis and implementation contexts. Review the actual content before using it for an original implementation.\n\n## Goal\n\n")?;
 out.quote(&text(c,"question"))?;
 if !boolean(c,"question_complete"){out.add(&format!("The retained goal is an excerpt of an original {}-byte question. Unretained text remains unavailable here.\n\n",number(c,"question_bytes")))?;}
 let plan=get(c,"plan");
 for(key,label)in[("questions","Questions"),("requirements","Requirements"),("constraints","Constraints and implementation language")]{out.add(&format!("## {label}\n\n"))?;out.list(&array(plan,key))?;}
 if plan==&Null{out.add("No authored plan is recorded yet. This is not an inferred or completed implementation plan.\n\n")?;}
 out.add("## Findings and design\n\nThese entries are authored interpretations, not automatically proven behavior.\n\n")?;
 let notes=array(c,"notes");let mut shown=0;
 for note in &notes{
  let kind=text(note,"kind");if kind=="previous_plan"{continue;}shown+=1;
  out.add(&format!("### Entry {}: {kind}\n\n",number(note,"id")))?;out.quote(&text(note,"text"))?;
  out.add("Recorded origin:\n\n")?;out.quote(&text(note,"origin"))?;
  let refs=array(note,"citations");if !refs.is_empty(){out.add(&format!("Returned-evidence reference IDs: {}.\n\n",Arr(refs).string()))?;}
  if get(note,"resolves")!=&Null{out.add(&format!("Authored resolution links to entry {}.\n\n",number(note,"resolves")))?;}
 }
 if shown==0{out.add("No findings or design entries recorded.\n\n")?;}
 let current=summary(c);
 out.add("## Plan review and unresolved work\n\nCurrent authored item-review counts (not independent acceptance):\n\n")?;
 out.quote(&get(&current,"item_review").string())?;
 out.add(&format!("Unknown entries without an authored resolution: {}. Omitted returned observations: {}. Tracking omissions: {}.\n\n",number(&current,"unknowns_without_authored_resolution"),number(c,"omitted_evidence"),number(c,"tracking_omissions")))?;
 out.add("## Returned-evidence references\n\nThese describe retained returned text, not complete source behavior. Source/output bodies and request arguments are not included.\n\n")?;
 let evidence=array(c,"evidence");
 for row in &evidence{out.add(&format!("- Receipt {}: operation {}; returned bytes {}; retained bytes {}; complete returned text: {}; returned-text SHA256 {}.\n",number(row,"id"),text(row,"tool"),number(row,"bytes"),text(row,"output").len(),boolean(row,"complete"),text(row,"sha256")))?;}
 if evidence.is_empty(){out.add("No returned evidence recorded.\n")?;}
 out.add("\n## Finite returned-text checks\n\nThese comparisons do not prove an authored design or full program behavior. Expected source/output text is not included.\n\n")?;
 let checks=array(c,"checks");
 for check in &checks{out.add(&format!("- Check {}: receipt {}; method {}; verdict {}.\n",number(check,"id"),number(check,"evidence"),text(check,"method"),assess(c,check)))?;}
 if checks.is_empty(){out.add("No finite checks recorded.\n")?;}
 out.add("\nThe complete journal retains prior plans, assessments, answers and raw returned observations separately. Missing evidence remains unresolved. The requested implementation language belongs in the recorded constraints; no compiler or language is selected by this export.\n")?;
 Ok(out.body)
}

#[cfg(test)]mod checks{
 use super::*;
 #[test]fn unknown_work_is_not_filled_with_invented_findings(){let c=obj(vec![("question",s("Describe the behavior")),("question_complete",Bool(true))]);let brief=content(&c).unwrap();assert!(brief.contains("No authored plan is recorded"));assert!(brief.contains("No findings or design entries recorded"));assert!(brief.contains("No returned evidence recorded"));assert!(!brief.contains("All requirements completed"));}
 #[test]fn authored_markup_cannot_forge_sections(){let c=obj(vec![("question",s("量\n# Forged\n<script>[link](private)</script>")),("question_complete",Bool(true))]);let brief=content(&c).unwrap();assert!(brief.contains("> 量\n> \\# Forged"));assert!(brief.contains("&lt;script&gt;\\[link\\]"));assert!(!brief.contains("<script>"));assert!(!brief.contains("\n# Forged"));}
 #[test]fn escaping_capacity_refuses_only_the_projection(){let c=obj(vec![("question",s(&"&".repeat(60000))),("question_complete",Bool(true))]);let original=c.clone();assert!(content(&c).unwrap_err().contains("exceeds256KiB"));assert_eq!(c,original);}
}
