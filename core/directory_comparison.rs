//! Positional comparison of actual bounded directory observations.
use super::directory_observation::{Report,Occurrence,Kind,Coverage};
fn key<'a>(report:&Report,item:&'a Occurrence)->&'a [u8]{
    if item.path==report.selected{return b"";}
    let start=report.selected.len()+usize::from(!report.selected.is_empty());
    &item.path[start.min(item.path.len())..]
}
fn name(path:&[u8])->String{match std::str::from_utf8(path){Ok(v)=>format!("{v:?}"),Err(_)=>format!("hex:{}",path.iter().map(|v|format!("{v:02x}")).collect::<String>())}}
fn relation(a:&Occurrence,b:&Occurrence)->String{
    if a.coverage==Coverage::Changed||b.coverage==Coverage::Changed{return "An input changed during observation; comparison unknown".into();}
    if a.kind==Kind::Unknown||b.kind==Kind::Unknown||a.identity.is_none()||b.identity.is_none(){return "Entry metadata was unavailable on one or both sides; comparison unknown".into();}
    if a.kind!=b.kind{return format!("Different observed kinds: {:?} / {:?}",a.kind,b.kind);}
    if a.kind==Kind::Directory{return "Directory observed on both sides".into();}
    if a.kind!=Kind::File{return "Content comparison unavailable for this entry kind".into();}
    if let (Some(x),Some(y))=(&a.identity,&b.identity){if x.size!=y.size{return format!("Different observed file lengths: {} / {}",x.size,y.size);}}
    match (&a.sample,&b.sample){
        (Some(x),Some(y))=>{
            let common=x.bytes.len().min(y.bytes.len());
            let different=x.bytes[..common].iter().zip(&y.bytes[..common]).filter(|(a,b)|a!=b).count();
            if different>0{return format!("Different sampled bytes: {different} positions in {common} common bytes");}
            if x.complete_file&&y.complete_file&&x.bytes==y.bytes{return "Complete captured contents are byte-identical".into();}
            format!("{common} common sampled bytes match; whole-file equality unknown")
        },
        _=>"File content was not available on both sides; equality unknown".into(),
    }
}
pub fn describe(left:&Report,right:&Report,limit:usize,cancelled:&impl Fn()->bool)->Result<String,String>{
    let limit=limit.min(120000);
    let mut out=format!("Selected folder comparison\nLeft {}: {:?}, {} occurrences, {} sampled bytes\nRight {}: {:?}, {} occurrences, {} sampled bytes\nPaths are relative to each selected folder. Observations were captured separately, not as an atomic pair. A matching prefix is not whole-file equality.\n",name(&left.selected),left.coverage,left.occurrences.len(),left.sampled_bytes,name(&right.selected),right.coverage,right.occurrences.len(),right.sampled_bytes);
    if out.len()>limit{out.truncate(out.floor_char_boundary(limit));return Ok(out);}
    let(mut a,mut b)=(0,0);
    while a<left.occurrences.len()||b<right.occurrences.len(){
        if cancelled(){return Err("Folder comparison cancelled".into());}
        let x=left.occurrences.get(a);let y=right.occurrences.get(b);
        let order=match (x,y){(Some(x),Some(y))=>key(left,x).cmp(key(right,y)),(Some(_),None)=>std::cmp::Ordering::Less,_=>std::cmp::Ordering::Greater};
        let row=match order{
            std::cmp::Ordering::Equal=>{let(x,y)=(x.unwrap(),y.unwrap());a+=1;b+=1;format!("{}: {}\n",name(key(left,x)),relation(x,y))},
            std::cmp::Ordering::Less=>{let x=x.unwrap();a+=1;format!("{}: observed only on left; {}\n",name(key(left,x)),if right.coverage==Coverage::Complete{"absent from the complete right observation"}else{"absence on right is unverified because its coverage is incomplete"})},
            std::cmp::Ordering::Greater=>{let y=y.unwrap();b+=1;format!("{}: observed only on right; {}\n",name(key(right,y)),if left.coverage==Coverage::Complete{"absent from the complete left observation"}else{"absence on left is unverified because its coverage is incomplete"})},
        };
        if out.len().saturating_add(row.len()).saturating_add(80)>limit{
            let note="Further comparison rows omitted from this display.\n";
            if out.len()+note.len()<=limit{out.push_str(note);}break;
        }
        out.push_str(&row);
    }
    Ok(out)
}
