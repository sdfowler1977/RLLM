//! Original bounded image transport validation. Framing/dimensions are checked;
//! this is not a pixel decoder or a claim that the selected model supports vision.
use super::*;
pub const MAX_IMAGE_BYTES:usize=2*1024*1024;
pub const MAX_TOTAL_BYTES:usize=2*1024*1024;
pub const MAX_IMAGES:usize=4;
fn dimensions(w:u32,h:u32)->Result<(u32,u32)>{
    if w==0||h==0||w>16384||h>16384||u64::from(w)*u64::from(h)>32_000_000{return Err("Image dimensions exceed 16384 per side or 32 million pixels".into());}Ok((w,h))
}
fn be32(b:&[u8])->u32{u32::from_be_bytes([b[0],b[1],b[2],b[3]])}
fn crc(bytes:&[u8])->u32{let mut v=!0u32;for b in bytes{v^=u32::from(*b);for _ in 0..8{v=(v>>1)^(0xedb88320u32.wrapping_mul(v&1));}}!v}
fn png(b:&[u8])->Result<(u32,u32)>{
    if b.len()<33||&b[..8]!=b"\x89PNG\r\n\x1a\n"{return Err("Invalid PNG signature".into());}
    let mut p=8;let mut count=0;let mut dims=None;let mut data=false;
    while p<b.len(){
        count+=1;if count>4096||b.len()-p<12{return Err("Invalid PNG chunk framing".into());}
        let n=be32(&b[p..p+4]) as usize;
        let end=p.checked_add(12).and_then(|v|v.checked_add(n)).ok_or("PNG length overflow")?;
        if end>b.len(){return Err("Truncated PNG chunk".into());}
        let typ=&b[p+4..p+8];let payload=&b[p+8..end-4];
        if crc(&b[p+4..end-4])!=be32(&b[end-4..end]){return Err("PNG checksum mismatch".into());}
        if count==1{
            if typ!=b"IHDR"||n!=13{return Err("PNG must start with IHDR".into());}
            dims=Some(dimensions(be32(payload),be32(&payload[4..]))?);
            let depth=payload[8];let color=payload[9];
            let valid=match color{0=>[1,2,4,8,16].contains(&depth),2|4|6=>[8,16].contains(&depth),3=>[1,2,4,8].contains(&depth),_=>false};
            if !valid||payload[10]!=0||payload[11]!=0||payload[12]>1{return Err("Unsupported PNG header".into());}
        }else if typ==b"IHDR"{return Err("Duplicate PNG header".into());}
        if typ==b"IDAT"{data=true;}
        if typ==b"IEND"{if n!=0||!data||end!=b.len(){return Err("Invalid PNG end".into());}return dims.ok_or("Missing PNG dimensions".into());}
        p=end;
    }Err("PNG is missing IEND".into())
}
fn jpeg(b:&[u8])->Result<(u32,u32)>{
    if b.len()<4||b[..2]!=[255,216]{return Err("Invalid JPEG signature".into());}
    let mut p=2;let mut dims=None;let mut scan=false;let mut entropy=false;let mut count=0;
    while p<b.len(){
        if entropy{while p<b.len()&&b[p]!=255{p+=1;}if p==b.len(){break;}}
        if b[p]!=255{return Err("Invalid JPEG marker".into());}while p<b.len()&&b[p]==255{p+=1;}if p==b.len(){break;}
        let marker=b[p];p+=1;
        if entropy&&(marker==0||(0xd0..=0xd7).contains(&marker)){continue;}
        entropy=false;count+=1;if count>65536{return Err("Too many JPEG segments".into());}
        if marker==0xd9{if !scan||p!=b.len(){return Err("Invalid JPEG end".into());}return dims.ok_or("Missing JPEG dimensions".into());}
        if marker==0||marker==0xd8||(0xd0..=0xd7).contains(&marker){return Err("Unexpected JPEG marker".into());}
        if marker==1{continue;}
        if b.len()-p<2{return Err("Truncated JPEG length".into());}let n=u16::from_be_bytes([b[p],b[p+1]])as usize;
        if n<2||n>b.len()-p{return Err("Truncated JPEG segment".into());}
        if [0xc0,0xc1,0xc2].contains(&marker){
            if n<8||b[p+2]!=8{return Err("Unsupported JPEG frame".into());}
            let components=b[p+7] as usize;if ![1,3,4].contains(&components)||n!=8+3*components{return Err("Invalid JPEG component frame".into());}
            if dims.is_some(){return Err("Multiple JPEG frames unsupported".into());}
            dims=Some(dimensions(u16::from_be_bytes([b[p+5],b[p+6]])as u32,u16::from_be_bytes([b[p+3],b[p+4]])as u32)?);
        }else if (0xc0..=0xcf).contains(&marker)&&![0xc4,0xc8,0xcc].contains(&marker){return Err("Unsupported JPEG encoding".into());}
        if marker==0xda{if dims.is_none()||n<6||n!=6+2*b[p+2]as usize{return Err("Invalid JPEG scan".into());}scan=true;entropy=true;}
        p+=n;
    }Err("JPEG is missing complete end marker".into())
}
pub fn image(input:&Json)->Result<Json>{
    let name=field(input,"name")?;if name.is_empty()||name.len()>256||name.chars().any(char::is_control){return Err("Image name must be 1–256 bytes without control characters".into());}
    if let Ok(kind)=input.get("kind"){if kind!=&s("image"){return Err("Unsupported attachment kind".into());}}
    let mime=field(input,"mime")?;if !["image/png","image/jpeg"].contains(&mime){return Err("Only PNG and JPEG image attachments are supported".into());}
    let declared=match input.get("bytes")?{Num(v)=>v.parse::<usize>().map_err(|_|"Invalid attachment byte count")?,_=>return Err("Attachment byte count must be an integer".into())};
    let data=field(input,"data")?;
    if declared==0||declared>MAX_IMAGE_BYTES||data.len()>((MAX_IMAGE_BYTES+2)/3)*4{return Err("Image exceeds 2 MiB; choose a smaller image".into());}
    let raw=network::unbase64(data)?;
    if raw.len()!=declared||network::b64(&raw)!=data{return Err("Image size or canonical base64 does not match".into());}
    let (w,h)=if mime=="image/png"{png(&raw)?}else{jpeg(&raw)?};
    Ok(obj(vec![("kind",s("image")),("name",s(name)),("mime",s(mime)),("data",s(data)),("bytes",Num(declared.to_string())),("width",Num(w.to_string())),("height",Num(h.to_string()))]))
}
pub fn images(message:&Json)->Result<Vec<Json>>{
    let list=match message.get("attachments"){Ok(v)=>v.array()?,Err(_)=>return Ok(Vec::new())};
    if list.len()>MAX_IMAGES{return Err("At most four images may be attached".into());}
    if !list.is_empty()&&field(message,"role")?!="user"{return Err("Images may only be attached to user messages".into());}
    let mut total=0;let mut out=Vec::new();for value in list{let valid=image(value)?;total+=field_number(&valid,"bytes")?;if total>MAX_TOTAL_BYTES{return Err("Attached images exceed 2 MiB total".into());}out.push(valid);}Ok(out)
}
fn field_number(v:&Json,key:&str)->Result<usize>{match v.get(key)?{Num(n)=>n.parse::<usize>().map_err(|_|"Invalid attachment size".into()),_=>Err("Invalid attachment size".into())}}
/// Base64 transport bytes, not tokenizer counts or image-token estimates.
pub fn encoded_bytes(messages:&[Json])->Result<usize>{
    let mut bytes=0usize;let mut decoded=0usize;let mut count=0;
    for message in messages{for value in images(message)?{count+=1;decoded+=field_number(&value,"bytes")?;bytes=bytes.checked_add(field(&value,"data")?.len()).ok_or("Attachment size overflow")?;}}
    if count>MAX_IMAGES||decoded>MAX_TOTAL_BYTES{return Err("Conversation images exceed four images or 2 MiB; start a new chat or remove images".into());}Ok(bytes)
}
/// Safe UI metadata: never includes the encoded payload.
pub fn metadata(value:&Json)->Result<Json>{let mut value=image(value)?;if let Obj(fields)=&mut value{fields.remove("data");}Ok(value)}

#[cfg(test)]
pub(crate) mod checks{
    use super::*;
    pub fn fixture()->Json{
        // Valid 1x1 grayscale PNG, original bytes preserved across adapters.
        let mut bytes=b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind,payload) in [(b"IHDR".as_slice(),vec![0,0,0,1,0,0,0,1,8,0,0,0,0]),(b"IDAT".as_slice(),vec![120,156,99,96,0,0,0,2,0,1]),(b"IEND".as_slice(),vec![])]{
            bytes.extend_from_slice(&(payload.len()as u32).to_be_bytes());let start=bytes.len();bytes.extend_from_slice(kind);bytes.extend_from_slice(&payload);let sum=crc(&bytes[start..]);bytes.extend_from_slice(&sum.to_be_bytes());
        }
        obj(vec![("name",s("screen.png")),("mime",s("image/png")),("bytes",Num(bytes.len().to_string())),("data",s(&network::b64(&bytes)))])
    }
    #[test]fn attachment_integrity_and_metadata(){
        let original=fixture();let valid=image(&original).unwrap();assert_eq!(valid.get("data"),original.get("data"));assert_eq!(valid.get("width"),Ok(&Num("1".into())));assert!(metadata(&valid).unwrap().get("data").is_err());
        let mut corrupt=original.clone();if let Obj(v)=&mut corrupt{v.insert("mime".into(),s("image/jpeg"));}assert!(image(&corrupt).is_err());
        if let Obj(v)=&mut corrupt{v.insert("mime".into(),s("image/png"));v.insert("data".into(),s(&(field(&original,"data").unwrap().to_string()+"AAAA")));}assert!(image(&corrupt).is_err());
        let mut bytes=network::unbase64(field(&original,"data").unwrap()).unwrap();bytes[20]^=1;
        if let Obj(v)=&mut corrupt{v.insert("data".into(),s(&network::b64(&bytes)));}assert!(image(&corrupt).is_err());
    }
    #[test]fn attachment_jpeg_original_bytes(){
        let data="/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAADAAIDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDx2iiiu04z/9k=";
        let raw=network::unbase64(data).unwrap();
        let value=obj(vec![("name",s("photo.jpg")),("mime",s("image/jpeg")),("data",s(data)),("bytes",Num(raw.len().to_string()))]);
        let valid=image(&value).unwrap();assert_eq!(valid.get("width"),Ok(&Num("2".into())));assert_eq!(valid.get("height"),Ok(&Num("3".into())));assert_eq!(field(&valid,"data").unwrap(),data);
        let mut broken=value.clone();if let Obj(v)=&mut broken{v.insert("data".into(),s(&network::b64(&raw[..raw.len()-2])));v.insert("bytes".into(),Num((raw.len()-2).to_string()));}assert!(image(&broken).is_err());
    }
    #[test]fn attachment_context_admission_counts_encoded_bytes(){
        let image=fixture();let encoded=field(&image,"data").unwrap().len();let messages=vec![obj(vec![("role",s("user")),("content",s("é")),("attachments",Arr(vec![image]))])];assert_eq!(generation::content_bytes(&messages).unwrap(),encoded+2);
        let mut body=obj(vec![("preserve",s("exact"))]);let before=body.clone();assert!(generation::apply(&mut body,&obj(vec![("context_bytes",Num((encoded+1).to_string()))]),"chat",&messages).is_err());assert_eq!(body,before);
    }
    #[test]fn attachment_role_and_count_bounds(){
        let message=|role:&str,n|obj(vec![("role",s(role)),("content",s("question")),("attachments",Arr(vec![fixture();n]))]);
        assert!(images(&message("assistant",1)).is_err());assert!(images(&message("system",1)).is_err());assert!(images(&message("user",5)).is_err());
        assert!(encoded_bytes(&[message("user",3),message("user",2)]).is_err());assert!(encoded_bytes(&[message("user",4)]).unwrap()>0);
    }
    #[test]fn attachment_provider_refusal_is_transactional(){
        let source=vec![obj(vec![("role",s("user")),("content",s("question")),("attachments",Arr(vec![fixture()]))])];
        let mut body=obj(vec![("messages",Arr(vec![]))]);let before=body.clone();
        assert!(provider::apply_attachments(&mut body,&source,"chat").is_err());assert_eq!(body,before);
        assert!(provider::apply_attachments(&mut body,&source,"unknown").is_err());assert_eq!(body,before);
        let huge="x".repeat(4*1024*1024);let mut body=obj(vec![("messages",Arr(vec![obj(vec![("role",s("user")),("content",s("question"))])])),("extra",s(&huge))]);let before=body.clone();
        assert!(provider::apply_attachments(&mut body,&source,"chat").is_err());assert_eq!(body,before);
    }
    #[test]fn attachment_public_cabi_tcp_wire(){
        use std::io::{Read,Write};
        use std::net::{TcpListener,TcpStream,Shutdown};
        use std::time::Duration;
        fn cabi(input:&Json)->Json{
            let input=input.string();unsafe{
                let pointer=client_process(input.as_ptr(),input.len());assert!(!pointer.is_null());
                let result=std::ffi::CStr::from_ptr(pointer).to_str().unwrap().to_string();client_free(pointer);parse(&result).unwrap()
            }
        }
        let image=fixture();let encoded=field(&image,"data").unwrap();
        for protocol in ["chat","local","messages","content"]{
            let input=obj(vec![("op",s("request")),("protocol",s(protocol)),("model",s("selected-model")),("messages",Arr(vec![obj(vec![("role",s("system")),("content",s("protected instructions"))]),obj(vec![("role",s("user")),("content",s("original question")),("attachments",Arr(vec![image.clone()]))])]))]);
            let result=cabi(&input);let body=result.get("value").unwrap().string();
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
            let receive=std::thread::spawn(move||{
                let(socket,_)=listener.accept().unwrap();socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes=Vec::new();socket.take(4*1024*1024+8192).read_to_end(&mut bytes).unwrap();bytes
            });
            let effect=network::wire(&format!("http://{address}/chosen/path?keep=yes"),true,"POST",&body,vec![("Content-Type","application/json".into()),("Authorization","Bearer isolated-test-key".into())]).unwrap();
            let mut socket=TcpStream::connect(address).unwrap();socket.set_write_timeout(Some(Duration::from_secs(5))).unwrap();socket.write_all(field(&effect,"wire").unwrap().as_bytes()).unwrap();socket.shutdown(Shutdown::Write).unwrap();
            let bytes=receive.join().unwrap();let wire=std::str::from_utf8(&bytes).unwrap();let(headers,payload)=wire.split_once("\r\n\r\n").unwrap();
            assert!(headers.starts_with("POST /chosen/path?keep=yes HTTP/1.1\r\n"));assert!(headers.contains("Authorization: Bearer isolated-test-key"));assert!(headers.contains(&format!("Content-Length: {}\r\n",body.len())));assert_eq!(payload,body);
            assert!(payload.contains("original question"));assert!(payload.contains("protected instructions"));assert!(payload.contains(encoded));assert!(!payload.contains("screen.png"));
            let parsed=parse(payload).unwrap();let messages=parsed.get(if protocol=="content"{"contents"}else{"messages"}).unwrap().array().unwrap();let user=messages.last().unwrap();
            match protocol{
                "local"=>assert_eq!(user.get("images").unwrap().array().unwrap()[0],s(encoded)),
                "chat"=>assert_eq!(field(user.get("content").unwrap().array().unwrap()[1].get("image_url").unwrap(),"url").unwrap(),format!("data:image/png;base64,{encoded}")),
                "messages"=>assert_eq!(field(user.get("content").unwrap().array().unwrap()[0].get("source").unwrap(),"data").unwrap(),encoded),
                _=>assert_eq!(field(user.get("parts").unwrap().array().unwrap()[1].get("inlineData").unwrap(),"data").unwrap(),encoded)
            }
        }
        let invalid=obj(vec![("op",s("request")),("protocol",s("chat")),("model",s("selected-model")),("messages",Arr(vec![obj(vec![("role",s("assistant")),("content",s("invalid role")),("attachments",Arr(vec![image]))])]))]);
        assert!(cabi(&invalid).get("error").is_ok());
    }
    #[test]fn attachment_provider_wire_shapes(){
        let attachment=fixture();let data=field(&attachment,"data").unwrap();
        let source=vec![obj(vec![("role",s("system")),("content",s("protected"))]),obj(vec![("role",s("user")),("content",s("describe")),("attachments",Arr(vec![attachment.clone()]))])];
        for protocol in ["chat","local","messages","content"]{
            let input=obj(vec![("protocol",s(protocol)),("model",s("chosen")),("messages",Arr(source.clone()))]);let body=request(&input).unwrap();
            let key=if protocol=="content"{"contents"}else{"messages"};let messages=body.get(key).unwrap().array().unwrap();let user=messages.last().unwrap();
            let got=match protocol{
                "local"=>field(&user,"content").map(|v|{assert_eq!(v,"describe");user.get("images").unwrap().array().unwrap()[0].text().unwrap().to_string()}).unwrap(),
                "chat"=>{let parts=user.get("content").unwrap().array().unwrap();assert_eq!(field(&parts[0],"text").unwrap(),"describe");field(parts[1].get("image_url").unwrap(),"url").unwrap().strip_prefix("data:image/png;base64,").unwrap().to_string()},
                "messages"=>{let parts=user.get("content").unwrap().array().unwrap();assert_eq!(field(&parts[1],"text").unwrap(),"describe");field(parts[0].get("source").unwrap(),"data").unwrap().to_string()},
                _=>{let parts=user.get("parts").unwrap().array().unwrap();assert_eq!(field(&parts[0],"text").unwrap(),"describe");field(parts[1].get("inlineData").unwrap(),"data").unwrap().to_string()}
            };assert_eq!(got,data);
            assert!(!body.string().contains("screen.png"));assert!(body.string().contains("protected"));
        }
    }
}
