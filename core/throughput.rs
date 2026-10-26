//! App-observed visible text and monotonic request time. No provider usage input.
use super::*;

pub(super) const METHOD:&str="Approximate tokens = received visible Unicode characters ÷ 4. Rate = that estimate ÷ elapsed request seconds, including waiting and network time. This is a running request average, not engine generation speed. Language, code and tokenizer differences affect accuracy. Text-form tool requests are counted; hidden reasoning and structured tool-call fields are excluded. Server-reported usage is separate.";

pub(super) fn start(st:&mut Json) {
    put(st,"local_rate",obj(vec![("chat",s(&text(st,"request_chat"))),
        ("ticket",s(&text(st,"model_expected"))),("model",s(&text(st,"request_model"))),
        ("connection",Num(number(st,"request_index").to_string())),
        ("characters",Num("0".into())),("elapsed_ms",Num("0".into())),("live",Bool(true))]));
}

pub(super) fn observe(st:&mut Json,inputs:&Json,received:Option<&str>) {
    let mut rate=get(st,"local_rate").clone();
    if !boolean(&rate,"live") {return;}
    let elapsed=number(inputs,"elapsed_ms");
    // Duplicate/out-of-order UI callbacks must never turn the clock backwards.
    // Text may still have arrived before a queued clock tick, so retain its count.
    if let Some(value)=received {
        if value.len()<=2*1024*1024 {
            let count=value.chars().count().max(number(&rate,"characters"));
            put(&mut rate,"characters",Num(count.to_string()));
        }
    }
    if elapsed>0 && elapsed<=3_600_000 && elapsed>=number(&rate,"elapsed_ms") {
        put(&mut rate,"elapsed_ms",Num(elapsed.to_string()));
    }
    put(st,"local_rate",rate);
}

pub(super) fn finish(st:&mut Json,inputs:&Json,outcome:&str)->Json {
    observe(st,inputs,None);
    let mut rate=get(st,"local_rate").clone();
    if rate==Null{return Null;}
    // Completion is timestamped by the transport before its UI callback queues.
    // A delayed UI tick can have advanced past that real completion instant.
    let elapsed=number(inputs,"elapsed_ms");
    if elapsed>0 && elapsed<=3_600_000 {put(&mut rate,"elapsed_ms",Num(elapsed.to_string()));}
    put(&mut rate,"live",Bool(false));put(&mut rate,"outcome",s(outcome));
    put(st,"local_rate",rate.clone());rate
}

pub(super) fn value(rate:&Json)->Option<f64> {
    let elapsed=number(rate,"elapsed_ms");let count=number(rate,"characters");
    if elapsed==0||elapsed>3_600_000||count==0||count>2*1024*1024 {None}
    else {Some(count as f64*250.0/elapsed as f64)}
}
pub(super) fn label(rate:&Json)->String {
    value(rate).map(|v|format!("≈{v:.1} tok/s")).unwrap_or("— tok/s".into())
}
pub(super) fn for_chat(st:&Json)->Json {
    let live=get(st,"local_rate");
    if boolean(live,"live") && text(live,"chat")==text(st,"active") {return live.clone();}
    array(st,"requests").iter().rev().map(|r|get(r,"local_rate"))
        .find(|r|text(r,"chat")==text(st,"active")).cloned().unwrap_or(Null)
}
pub(super) fn ui(st:&Json)->Json {
    let rate=for_chat(st);let label=match value(&rate){
        Some(v) if v>=1_000_000.0=>format!("≈{:.1}M tok/s",v/1_000_000.0),
        Some(v) if v>=10_000.0=>format!("≈{:.1}k tok/s",v/1000.0),
        _=>label(&rate),
    };
    let status=if boolean(&rate,"live"){if value(&rate).is_some(){"Live request average"}else{"Waiting for text"}}
        else if rate==Null{"No measurement yet"}else if text(&rate,"outcome")=="success"{"Last completed request"}else{"Last interrupted request"};
    obj(vec![("label",s(&label)),("status",s(status)),("model",s(&text(&rate,"model"))),
        ("description",s(&format!("Analytics. {status}. {label}. Approximate tokens per second calculated on this device. {}",text(&rate,"model")))),
        ("measurement",rate)])
}

#[cfg(test)] mod checks {
    use super::*;
    #[test] fn local_estimate_uses_characters_and_clock_only() {
        let mut st=obj(vec![("request_chat",s("a")),("active",s("a"))]);start(&mut st);
        assert_eq!(value(get(&st,"local_rate")),None);
        observe(&mut st,&obj(vec![("elapsed_ms",Num("1000".into())),("completion_tokens",Num("99999".into()))]),Some("aé中🦀"));
        assert_eq!(value(get(&st,"local_rate")),Some(1.0));
        observe(&mut st,&obj(vec![("elapsed_ms",Num("2000".into()))]),None);
        assert_eq!(value(get(&st,"local_rate")),Some(0.5));
        observe(&mut st,&obj(vec![("elapsed_ms",Num("500".into()))]),Some("aé中🦀aé中🦀"));
        assert_eq!(value(get(&st,"local_rate")),Some(1.0));
        let row=finish(&mut st,&obj(vec![]),"cancelled");
        observe(&mut st,&obj(vec![("elapsed_ms",Num("3000".into()))]),Some("late text ignored"));
        assert_eq!(get(&st,"local_rate"),&row);
        start(&mut st);assert_eq!(value(get(&st,"local_rate")),None);
        put(&mut st,"active",s("b"));assert_eq!(for_chat(&st),Null);
    }
}
