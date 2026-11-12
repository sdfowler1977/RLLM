use std::ffi::c_void;
#[derive(Clone)]
struct Value {kind:u32,integer:i64,unsigned:u64,number:f64,text:String}
struct State {values:Vec<Value>,page:(u64,u64),model_values:Vec<(u64,String,String)>,machine_states:Vec<(u64,String)>,dispatched:Vec<(u64,String)>}
struct Control {id:(u64,u64),kind:u32,field:Option<usize>,action:Option<usize>,page:(u64,u64),visible:bool,enabled:bool,text:&'static str,x:i32,y:i32,w:i32,h:i32}
struct Action {operation:u32,field:Option<usize>,page:(u64,u64),operand:Value}
struct PropertyRecord {node:(u64,u64),name:&'static str,value:Value,unit:u32,origin:u32,channel:u32,token_ref:&'static str,constraint:&'static str,inherited_from:&'static str}
struct LocaleRecord {locale:&'static str,key:&'static str,value:&'static str}
struct VariantOption {owner:(u64,u64),name:&'static str,order:u32,selected:bool}
struct NamedSlot {owner:(u64,u64),name:&'static str,order:u32}
const TITLE:&str="RLLM";
const WIDTH:i32=1440;
const HEIGHT:i32=900;
const LOCALE:&str="en";
const HIGH_CONTRAST:bool=false;
fn properties()->Vec<PropertyRecord>{vec![
]}
fn locales()->Vec<LocaleRecord>{vec![
]}
fn variant_options()->Vec<VariantOption>{vec![
]}
fn named_slots()->Vec<NamedSlot>{vec![
]}
pub struct ModelRecord {pub id:u64,pub name:&'static str,pub schema:&'static str,pub state:u32}
pub struct ModelFieldRecord {pub model:u64,pub name:&'static str,pub value:&'static str}
pub struct DataBindingRecord {pub node:(u64,u64),pub model:u64,pub property:&'static str,pub field:&'static str}
pub struct AuthoredActionRecord {pub id:u64,pub name:&'static str,pub payload:&'static str,pub cancellable:bool,pub running:bool}
pub struct InteractionRecord {pub node:(u64,u64),pub action:u64,pub event:&'static str}
pub struct MachineRecord {pub id:u64,pub name:&'static str,pub current:&'static str}
pub struct TransitionRecord {pub machine:u64,pub from:&'static str,pub event:&'static str,pub to:&'static str}
pub struct TokenRecord {pub id:u64,pub name:&'static str,pub value:&'static str,pub kind:u32}
pub struct ThemeRecord {pub id:u64,pub name:&'static str,pub high_contrast:bool,pub reduced_motion:bool,pub companion_hc:u64,pub companion_rm:u64}
pub struct ThemeBindingRecord {pub theme:u64,pub token:&'static str,pub value:&'static str}
pub struct ComponentRecord {pub node:(u64,u64),pub master:(u64,u64),pub selected_variant:&'static str}
pub struct VariantAxisRecord {pub owner:(u64,u64),pub axis:&'static str,pub selected_value:&'static str}
pub struct SlotFillRecord {pub owner:(u64,u64),pub slot:(u64,u64),pub name:&'static str,pub fill:(u64,u64),pub order:u32}
pub const ACTIVE_THEME:u64=0;
pub fn authored_models()->Vec<ModelRecord>{vec![
]}
pub fn authored_model_fields()->Vec<ModelFieldRecord>{vec![
]}
pub fn authored_bindings()->Vec<DataBindingRecord>{vec![
]}
pub fn authored_actions()->Vec<AuthoredActionRecord>{vec![
]}
pub fn authored_interactions()->Vec<InteractionRecord>{vec![
]}
pub fn authored_machines()->Vec<MachineRecord>{vec![
]}
pub fn authored_transitions()->Vec<TransitionRecord>{vec![
]}
pub fn authored_tokens()->Vec<TokenRecord>{vec![
]}
pub fn authored_components()->Vec<ComponentRecord>{vec![
]}
pub fn authored_variant_axes()->Vec<VariantAxisRecord>{vec![
]}
pub fn authored_slot_fills()->Vec<SlotFillRecord>{vec![
]}
pub fn authored_themes()->Vec<ThemeRecord>{vec![
]}
pub fn authored_theme_bindings()->Vec<ThemeBindingRecord>{vec![
]}
fn fields()->State {State{values:vec![
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"Chat".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"unbound".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"navigation".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"navigation.rail-navigation".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"surface".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"catalog/widgets/navigation/rail-navigation.json".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"900".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"240".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"unbound".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"feedback".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"feedback.status-indicator".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"surface".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"catalog/widgets/feedback/status-indicator.json".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"unbound".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"collaboration".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"collaboration.chat-panel".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"surface".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"catalog/widgets/collaboration/chat-panel.json".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"unbound".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"collaboration".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"collaboration.message-composer".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"surface".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"catalog/widgets/collaboration/message-composer.json".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"unbound".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"data-display".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"data-display.metric-strip".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"surface".into()},
Value{kind:5,integer:0,unsigned:0,number:f64::from_bits(0),text:"catalog/widgets/data-display/metric-strip.json".into()},
],page:(15012424944407266815,780562255590203591),model_values:authored_model_fields().into_iter().map(|field|(field.model,field.name.into(),field.value.into())).collect(),machine_states:authored_machines().into_iter().map(|machine|(machine.id,machine.current.into())).collect(),dispatched:Vec::new()}}
fn actions()->Vec<Action>{vec![
]}
fn controls()->Vec<Control>{vec![
Control{id:(1339411327815599138,6429154242277442566),kind:4,field:None,action:None,page:(15012424944407266815,780562255590203591),visible:true,enabled:true,text:"Rail Navigation",x:0,y:0,w:240,h:72},
Control{id:(9268281852698179525,11967019034599551184),kind:4,field:None,action:None,page:(15012424944407266815,780562255590203591),visible:true,enabled:true,text:"Status Indicator",x:0,y:0,w:240,h:72},
Control{id:(6414517284304972751,15647244845585789731),kind:4,field:None,action:None,page:(15012424944407266815,780562255590203591),visible:true,enabled:true,text:"Chat Panel",x:0,y:0,w:240,h:72},
Control{id:(4003179147508781617,10826243101883889593),kind:4,field:None,action:None,page:(15012424944407266815,780562255590203591),visible:true,enabled:true,text:"Message Composer",x:0,y:0,w:240,h:72},
Control{id:(10488644372086191839,2808221018575192877),kind:4,field:None,action:None,page:(15012424944407266815,780562255590203591),visible:true,enabled:true,text:"Metric Strip",x:0,y:0,w:240,h:72},
]}
fn present(c:&Control,s:&State)->bool {c.visible&&c.page==s.page}
fn authored_caption(c:&Control)->String {
 let properties=properties();
 let authored=properties.iter().find(|p|p.node==c.id&&p.channel==0&&p.name=="text")
  .map(|p|value_text(&p.value)).unwrap_or_else(||c.text.into());
 if let Some(key)=properties.iter().find(|p|p.node==c.id&&p.channel==0&&p.name=="i18n"&&p.value.kind==5) {
  if let Some(value)=locales().iter().find(|entry|entry.locale==LOCALE&&entry.key==key.value.text) {
   return value.value.into();
  }
 }
 authored
}
fn caption(c:&Control,s:&State)->String {
 let binding=authored_bindings().into_iter().find(|binding|binding.node==c.id&&binding.property=="text");
 let label=binding.and_then(|binding|s.model_values.iter().find(|(model,name,_)|*model==binding.model&&name==binding.field).map(|(_,_,value)|value.clone()))
  .unwrap_or_else(||authored_caption(c));
 match c.field {Some(i)=>format!("{}: {}",label,value_text(&s.values[i])),None=>label}
}
fn value_text(v:&Value)->String {match v.kind {0=>String::new(),1=>(v.unsigned!=0).to_string(),2=>v.integer.to_string(),3=>v.unsigned.to_string(),4=>v.number.to_string(),5=>v.text.clone(),_=>unreachable!()}}
fn authored_event(node:(u64,u64),event:&str,s:&mut State)->bool {
 let actions=authored_actions();let mut dispatched=false;
 for interaction in authored_interactions().into_iter().filter(|item|item.node==node&&item.event==event) {
  if let Some(action)=actions.iter().find(|action|action.id==interaction.action) {
   s.dispatched.push((action.id,action.payload.into()));dispatched=true;
  }
 }
 if dispatched {
  for transition in authored_transitions() {
   if transition.event==event {
    if let Some((_,state))=s.machine_states.iter_mut().find(|(id,state)|*id==transition.machine&&state==transition.from) {
     *state=transition.to.into();
    }
   }
  }
 }
 dispatched
}
fn activate(c:&Control,s:&mut State)->bool {
 if !present(c,s)||!c.enabled{return false;}
 let authored=authored_event(c.id,"activate",s);
 if c.kind==1||c.kind==4{return authored;}
 let Some(i)=c.action else{return authored;};let actions=actions();let a=&actions[i];
 match a.operation {
  1=>{if let Some(i)=a.field{s.values[i]=a.operand.clone();return true;}},
  2=>{if let Some(i)=a.field {if s.values[i].kind==2&&a.operand.kind==2 {if let Some(v)=s.values[i].integer.checked_add(a.operand.integer){s.values[i].integer=v;return true;}}}},
  3=>{if let Some(i)=a.field {if s.values[i].kind==1{s.values[i].unsigned=1-s.values[i].unsigned;return true;}}},
  4=>{s.page=a.page;return true;},_=>{}
 }authored
}
pub struct HierarchyNode {pub id:(u64,u64),pub parent:(u64,u64),pub object_kind:u32,pub flags:u32,pub frame:(i64,i64,i64,i64),pub role:&'static str,pub accessible_name:&'static str,pub accessible_description:&'static str,pub reduced_motion_ms:f64}
const HIERARCHY:&[HierarchyNode]=&[
HierarchyNode{id:(967718070594607367,4864933808445765363),parent:(0,0),object_kind:1,flags:3,frame:(0,0,24,24),role:"application",accessible_name:"RLLM",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(15012424944407266815,780562255590203591),parent:(967718070594607367,4864933808445765363),object_kind:2,flags:3,frame:(0,0,24,24),role:"main",accessible_name:"",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(1339411327815599138,6429154242277442566),parent:(15012424944407266815,780562255590203591),object_kind:4,flags:3,frame:(0,0,240,72),role:"group",accessible_name:"Rail Navigation",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(9268281852698179525,11967019034599551184),parent:(15012424944407266815,780562255590203591),object_kind:4,flags:3,frame:(0,0,240,72),role:"group",accessible_name:"Status Indicator",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(6414517284304972751,15647244845585789731),parent:(15012424944407266815,780562255590203591),object_kind:4,flags:3,frame:(0,0,240,72),role:"group",accessible_name:"Chat Panel",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(4003179147508781617,10826243101883889593),parent:(15012424944407266815,780562255590203591),object_kind:4,flags:3,frame:(0,0,240,72),role:"group",accessible_name:"Message Composer",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(10488644372086191839,2808221018575192877),parent:(15012424944407266815,780562255590203591),object_kind:4,flags:3,frame:(0,0,240,72),role:"group",accessible_name:"Metric Strip",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
HierarchyNode{id:(10478821835543334710,10565367804876862253),parent:(967718070594607367,4864933808445765363),object_kind:2,flags:3,frame:(0,0,24,24),role:"main",accessible_name:"",accessible_description:"",reduced_motion_ms:f64::from_bits(0)},
];
pub fn hierarchy()->&'static [HierarchyNode]{HIERARCHY}
pub const REDUCED_MOTION:bool=false;
fn headless()->bool {
 let args:Vec<String>=std::env::args().skip(1).collect();if args.is_empty(){return false;}
 if args==["--accessibility"] {
  println!("scope=authored-semantics platform-provider=unqualified reduced-motion={} high-contrast={} locale={}",REDUCED_MOTION,HIGH_CONTRAST,LOCALE);
  for n in hierarchy(){println!("id={}:{} parent={}:{} kind={} role={:?} name={:?} description={:?} flags={} frame={:?} reduced-motion-ms={}",n.id.0,n.id.1,n.parent.0,n.parent.1,n.object_kind,n.role,n.accessible_name,n.accessible_description,n.flags,n.frame,n.reduced_motion_ms);}
  for p in properties(){println!("property node={}:{} name={:?} channel={} unit={} origin={} value={:?} token={:?} constraint={:?} inherited={:?}",p.node.0,p.node.1,p.name,p.channel,p.unit,p.origin,value_text(&p.value),p.token_ref,p.constraint,p.inherited_from);}
  for l in locales(){println!("locale={:?} key={:?} value={:?}",l.locale,l.key,l.value);}
  for v in variant_options(){println!("variant owner={}:{} order={} name={:?} selected={}",v.owner.0,v.owner.1,v.order,v.name,v.selected);}
  for slot in named_slots(){println!("slot owner={}:{} order={} name={:?}",slot.owner.0,slot.owner.1,slot.order,slot.name);}
  for model in authored_models(){println!("model id={} name={:?} schema={:?} state={}",model.id,model.name,model.schema,model.state);}
  for field in authored_model_fields(){println!("model-field model={} name={:?} value={:?}",field.model,field.name,field.value);}
  for binding in authored_bindings(){println!("data-binding node={}:{} model={} property={:?} field={:?}",binding.node.0,binding.node.1,binding.model,binding.property,binding.field);}
  for action in authored_actions(){println!("authored-action id={} name={:?} payload={:?} cancellable={} running={}",action.id,action.name,action.payload,action.cancellable,action.running);}
  for interaction in authored_interactions(){println!("interaction node={}:{} action={} event={:?}",interaction.node.0,interaction.node.1,interaction.action,interaction.event);}
  for machine in authored_machines(){println!("machine id={} name={:?} current={:?}",machine.id,machine.name,machine.current);}
  for transition in authored_transitions(){println!("transition machine={} from={:?} event={:?} to={:?}",transition.machine,transition.from,transition.event,transition.to);}
  for token in authored_tokens(){println!("token id={} name={:?} value={:?} kind={}",token.id,token.name,token.value,token.kind);}
  for theme in authored_themes(){println!("theme id={} name={:?} high-contrast={} reduced-motion={} companion-hc={} companion-rm={} active={}",theme.id,theme.name,theme.high_contrast,theme.reduced_motion,theme.companion_hc,theme.companion_rm,theme.id==ACTIVE_THEME);}
  for binding in authored_theme_bindings(){println!("theme-binding theme={} token={:?} value={:?}",binding.theme,binding.token,binding.value);}
  return true;
 }
 if args.len()%3!=0 {eprintln!("usage: --activate <id-high> <id-low> [--activate ...]");std::process::exit(2);}
 let cs=controls();let mut s=fields();
 for a in args.chunks_exact(3){if a[0]!="--activate"{eprintln!("unknown command");std::process::exit(2);}
 let id=match(a[1].parse::<u64>(),a[2].parse::<u64>()){(Ok(h),Ok(l))=>(h,l),_=>{eprintln!("invalid identity");std::process::exit(2);}};
 let Some(c)=cs.iter().find(|c|c.id==id)else{eprintln!("action unavailable");std::process::exit(2);};
 if !activate(c,&mut s){eprintln!("action unavailable or value overflow");std::process::exit(2);}println!("{}",caption(c,&s));
 }true
}
use std::os::raw::{c_char,c_int,c_long,c_ulong};
use std::ffi::CString;
#[repr(C)]
struct ButtonEvent {type_:c_int,serial:c_ulong,send:c_int,display:*mut c_void,window:c_ulong,root:c_ulong,subwindow:c_ulong,time:c_ulong,x:c_int,y:c_int,x_root:c_int,y_root:c_int,state:u32,button:u32,same:c_int}
#[repr(C)]
union Event {type_:c_int,button:std::mem::ManuallyDrop<ButtonEvent>,pad:[c_long;24]}
#[link(name="X11")]
extern "C" {
 fn XOpenDisplay(n:*const c_char)->*mut c_void;
 fn XDefaultRootWindow(d:*mut c_void)->c_ulong;
 fn XCreateSimpleWindow(d:*mut c_void,p:c_ulong,x:c_int,y:c_int,w:u32,h:u32,b:u32,bc:c_ulong,bg:c_ulong)->c_ulong;
 fn XStoreName(d:*mut c_void,w:c_ulong,n:*const c_char)->c_int;
 fn XSelectInput(d:*mut c_void,w:c_ulong,m:c_long)->c_int;
 fn XMapWindow(d:*mut c_void,w:c_ulong)->c_int;
 fn XCreateGC(d:*mut c_void,w:c_ulong,m:c_ulong,v:*mut c_void)->*mut c_void;
 fn XSetForeground(d:*mut c_void,g:*mut c_void,p:c_ulong)->c_int;
 fn XFillRectangle(d:*mut c_void,w:c_ulong,g:*mut c_void,x:c_int,y:c_int,wi:u32,he:u32)->c_int;
 fn XDrawString(d:*mut c_void,w:c_ulong,g:*mut c_void,x:c_int,y:c_int,s:*const c_char,n:c_int)->c_int;
 fn XNextEvent(d:*mut c_void,e:*mut Event)->c_int;
 fn XLookupKeysym(e:*mut ButtonEvent,index:c_int)->c_ulong;
 fn XFlush(d:*mut c_void)->c_int;
 fn XFreeGC(d:*mut c_void,g:*mut c_void)->c_int;
 fn XCloseDisplay(d:*mut c_void)->c_int;
}
fn main() {if headless(){return;} unsafe {
 let d=XOpenDisplay(std::ptr::null()); if d.is_null(){eprintln!("display unavailable");std::process::exit(1);}
 let w=XCreateSimpleWindow(d,XDefaultRootWindow(d),0,0,WIDTH as u32,HEIGHT as u32,0,0,if HIGH_CONTRAST{0x000000}else{0xf4f5f7});
 XStoreName(d,w,CString::new(TITLE).unwrap().as_ptr()); XSelectInput(d,w,(1<<15)|(1<<2)|(1<<0)); XMapWindow(d,w);
 let g=XCreateGC(d,w,0,std::ptr::null_mut()); let cs=controls(); let mut f=fields(); let mut focus=0usize;
 loop {
  XSetForeground(d,g,if HIGH_CONTRAST{0x000000}else{0xf4f5f7});XFillRectangle(d,w,g,0,0,WIDTH as u32,HEIGHT as u32);
  for (i,c) in cs.iter().enumerate(){if !present(c,&f){continue;}XSetForeground(d,g,if HIGH_CONTRAST{if i==focus{0xffff00}else{0xffffff}}else if i==focus {0xc7dcf3}else{0xe0e6ed});XFillRectangle(d,w,g,c.x,c.y,c.w as u32,c.h as u32);
   XSetForeground(d,g,if HIGH_CONTRAST{0xffffff}else{0x162536});let text=caption(c,&f);XDrawString(d,w,g,c.x+8,c.y+c.h/2+5,text.as_ptr().cast(),text.len() as i32);}
  XFlush(d);let mut e=Event{pad:[0;24]}; XNextEvent(d,&mut e);
  if e.type_==4 { let b=&e.button; if b.button==1 {for(i,c)in cs.iter().enumerate(){if present(c,&f)&&c.enabled&&b.x>=c.x&&b.x<c.x+c.w&&b.y>=c.y&&b.y<c.y+c.h {focus=i;activate(c,&mut f);let t=format!("{} — {}",TITLE,caption(c,&f));XStoreName(d,w,CString::new(t).unwrap().as_ptr());}}}}
  if e.type_==2 { // Key events share button-event coordinates and keycode slot.
   let code=XLookupKeysym(&mut *e.button,0);if code==0xff1b {break;}if code==0xff09&&!cs.is_empty(){focus=(focus+1)%cs.len();}if (code==0xff0d||code==0x20)&&!cs.is_empty(){activate(&cs[focus],&mut f);}
  }
 }
 XFreeGC(d,g);XCloseDisplay(d);
}}
