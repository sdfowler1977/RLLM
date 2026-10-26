//! Bounded request observations and native drawing geometry. No polling or chart runtime.
use super::*;

fn numeric(row:&Json,key:&str)->Option<f64>{
    match get(row,key){Num(v)=>v.parse::<f64>().ok().filter(|v|v.is_finite()&&*v>=0.0),_=>None}
}
fn num(v:f64)->Json{Num(format!("{v:.3}"))}
fn label(v:Option<f64>,unit:&str)->String{
    match v {Some(v)=>if unit=="ms"{if v>=1000.0{format!("{:.2} s",v/1000.0)}else{format!("{v:.0} ms")}}else{format!("{v:.1} {unit}")},None=>"Unavailable".into()}
}
fn tick(v:f64)->String{
    if v>=1_000_000.0{format!("{:.1}M",v/1_000_000.0)}
    else if v>=10_000.0{format!("{:.1}k",v/1000.0)}
    else if v.fract().abs()<0.001{format!("{v:.0}")}else{format!("{v:.1}")}
}
fn shape(kind:&str,color:&str,fields:Vec<(&str,Json)>)->Json{
    let mut p=obj(fields);put(&mut p,"kind",s(kind));put(&mut p,"color",s(color));p
}
fn line(x:f64,y:f64,x2:f64,y2:f64,color:&str,width:f64)->Json{
    shape("line",color,vec![("x",num(x)),("y",num(y)),("x2",num(x2)),("y2",num(y2)),("width",num(width))])
}
fn caption(x:f64,y:f64,value:&str,align:&str)->Json{
    shape("text","muted",vec![("x",num(x)),("y",num(y)),("value",s(value)),("size",num(30.0)),("align",s(align))])
}
fn circle(x:f64,y:f64,r:f64,color:&str)->Json{
    shape("circle",color,vec![("x",num(x)),("y",num(y)),("radius",num(r))])
}
fn graphic(height:f64,shapes:Vec<Json>)->Json{
    obj(vec![("width",num(1000.0)),("height",num(height)),("shapes",Arr(shapes))])
}
fn gauge(title:&str,value:Option<f64>,max:f64,unit:&str,color:&str,source:&str)->Json{
    let mut paths=vec![shape("arc","border",vec![("x",num(330.0)),("y",num(32.0)),("x2",num(670.0)),("y2",num(372.0)),("start",num(135.0)),("sweep",num(270.0)),("width",num(25.0))])];
    if let Some(v)=value {paths.push(shape("arc",color,vec![("x",num(330.0)),("y",num(32.0)),("x2",num(670.0)),("y2",num(372.0)),("start",num(135.0)),("sweep",num(270.0*(v/max.max(1.0)).clamp(0.0,1.0))),("width",num(25.0))]));}
    paths.push(caption(500.0,220.0,if value.is_some(){if unit=="tok/s"{"ESTIMATE"}else{"OBSERVED"}}else{"NO DATA"},"center"));
    obj(vec![("kind",s("gauge")),("title",s(title)),("value",s(&if unit=="tok/s"&&value.is_some(){format!("≈{}",label(value,unit))}else{label(value,unit)})),
        ("caption",s(&format!("{source}\nScale 0–{}",label(Some(max),unit)))),("visual",graphic(390.0,paths))])
}
fn series(row:&Json,key:&str)->Option<f64>{
    if key=="estimated" {throughput::value(get(row,"local_rate"))}
    else if key=="characters" {numeric(get(row,"telemetry"),"character_rate_tenths").map(|v|v/10.0)}
    else if key=="tokens" {numeric(get(row,"telemetry"),"rate_tenths").map(|v|v/10.0)}
    else {numeric(row,key)}
}
fn plot(rows:&[Json],title:&str,keys:&[(&str,&str,&str)],unit:&str)->Json{
    let max=keys.iter().flat_map(|(key,_,_)|rows.iter().filter_map(move |r|series(r,key))).fold(0.0,f64::max).max(1.0);
    let mut paths=vec![];
    for fraction in [0.0,0.5,1.0]{
        let y=270.0-230.0*fraction;
        paths.push(line(155.0,y,970.0,y,"border",2.0));
        paths.push(caption(135.0,y+10.0,&tick(max*fraction),"right"));
    }
    paths.push(caption(155.0,320.0,"Older","left"));
    paths.push(caption(970.0,320.0,"Latest request","right"));
    let mut any=false;
    for (key,_,color) in keys {
        let mut prior=None;
        for (i,row) in rows.iter().enumerate(){
            let x=if rows.len()<2{562.5}else{155.0+815.0*i as f64/(rows.len()-1) as f64};
            if let Some(v)=series(row,key){
                let y=270.0-230.0*v/max;
                if let Some((px,py))=prior {paths.push(line(px,py,x,y,color,5.0));}
                paths.push(circle(x,y,5.0,color));prior=Some((x,y));any=true;
            }else{prior=None;}
        }
    }
    if !any {paths.push(caption(565.0,155.0,"No observations yet","center"));}
    obj(vec![("kind",s("plot")),("title",s(title)),("caption",s(&format!("Vertical axis: {unit} · request order\n{} · gaps mean unavailable",keys.iter().map(|(_,name,_)|*name).collect::<Vec<_>>().join(" / ")))),
        ("legend",Arr(keys.iter().map(|(_,name,color)|obj(vec![("label",s(name)),("color",s(color))])).collect())),
        ("visual",graphic(340.0,paths))])
}
fn heat(rows:&[Json])->Json{
    let peak=rows.iter().filter_map(|r|numeric(r,"elapsed_ms")).fold(0.0,f64::max).max(1.0);
    let mut shapes=vec![];
    for (i,row) in rows.iter().enumerate(){
        let elapsed=numeric(row,"elapsed_ms");let outcome=text(row,"outcome");
        let color=if outcome=="error"{"error"}else if outcome=="cancelled"{"muted"}else{
            match elapsed {None=>"border",Some(v) if v/peak>0.75=>"warning",Some(v) if v/peak>0.4=>"success",Some(_)=>"accent"}
        };
        let x=12.0+(i%4) as f64*250.0;let y=12.0+(i/4) as f64*224.0;
        shapes.push(shape("rect",color,vec![("x",num(x)),("y",num(y)),("x2",num(x+226.0)),("y2",num(y+200.0)),
            ("event",s(&format!("monitor:request:{}",text(row,"id"))))]));
        let mut number=caption(x+113.0,y+118.0,&(i+1).to_string(),"center");put(&mut number,"size",num(48.0));put(&mut number,"color",s("surface"));shapes.push(number);
    }
    if rows.is_empty(){shapes.push(caption(500.0,70.0,"Request history will appear here","center"));}
    obj(vec![("kind",s("heat")),("title",s("REQUEST HEAT MAP")),
        ("caption",s(&format!("Oldest → newest · tap a cell for details\nCyan ≤40%, green ≤75%, amber >75% of peak {}; red = error; gray = cancelled; outline = unavailable.",label(Some(peak),"ms")))),
        ("visual",graphic(((rows.len()+3)/4).max(1) as f64*224.0+12.0,shapes))])
}
pub(super) fn dashboard(st:&Json)->Json{
    let requested=number(st,"monitor_window");let count=if [16,32,64,128].contains(&requested){requested}else{32};
    let all=array(st,"requests");
    let filter=get(st,"monitor_model");
    let history=all.into_iter().rev().take(128).filter(|r|filter==&Null||request_metrics::same_identity(r,filter)).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
    let rows=&history[history.len().saturating_sub(count)..];
    let last=rows.last().unwrap_or(&Null);
    let peak=rows.iter().filter_map(|r|numeric(r,"elapsed_ms")).fold(0.0,f64::max).max(1.0);
    let live=get(st,"request_record")!=&Null && boolean(get(st,"local_rate"),"live") && (filter==&Null || request_metrics::same_identity(get(st,"request_record"),filter));
    let rate=if live{get(st,"local_rate")}else{get(last,"local_rate")};
    let rate_peak=rows.iter().filter_map(|r|series(r,"estimated")).fold(throughput::value(rate).unwrap_or(0.0),f64::max).max(1.0);
    let mut rate_gauge=gauge("ESTIMATED TOKENS / S",throughput::value(rate),rate_peak,"tok/s","success",&format!("{} · {}\n{}",if live{"Live request average"}else{"Latest request average"},text(rate,"model"),"Visible characters ÷ 4 / elapsed request time"));
    put(&mut rate_gauge,"id",s("local_rate"));
    let finished=rows.iter().filter(|r|["success","error"].contains(&text(r,"outcome").as_str())).count();
    let errors=rows.iter().filter(|r|text(r,"outcome")=="error").count();
    let error_rate=if finished==0{None}else{Some(100.0*errors as f64/finished as f64)};
    let last_elapsed=numeric(last,"elapsed_ms");
    let gauges=vec![
        rate_gauge,
        gauge("RESPONSE TIME",last_elapsed,peak,"ms","accent","Latest request · phone clock"),
        gauge("FIRST TEXT",numeric(last,"first_text_ms"),peak,"ms","violet","Latest streamed reply · phone clock"),
        gauge("ERROR RATE",error_rate,100.0,"%","error","Selected requests · cancellations excluded"),
    ];
    let mut cards=vec![
        plot(rows,"APP-ESTIMATED TOKEN THROUGHPUT",&[("estimated","Visible characters ÷ 4 / phone time","success")],"estimated tok/s"),
        plot(rows,"RESPONSE LATENCY",&[("elapsed_ms","Total","accent"),("first_text_ms","First text","violet")],"ms"),
        plot(rows,"OBSERVED TEXT THROUGHPUT",&[("characters","Counted characters / phone time","success")],"chars/s"),
        plot(rows,"REPORTED TOKEN THROUGHPUT",&[("tokens","Server token count / phone time","violet")],"tokens/s"),
        heat(rows),
    ];
    cards.push(obj(vec![("kind",s("text")),("title",s("MEASUREMENT SOURCES")),
        ("caption",s(&format!("{} recorded requests in view. Timing includes network and queue time.\nLatest received: {}; sent: {} (HTTP application bytes).\nThe ≈ tokens/s estimate uses received visible Unicode characters ÷ 4, divided by elapsed request time. It is a running request average, including waiting and network time, not engine speed. Accuracy varies with language, code and tokenizer. Text-form tool requests are counted; hidden reasoning and structured tool-call fields are excluded. Provider-reported tokens remain in a separate chart. First-text gaps mean no streamed-text measurement. GPU, server RAM, power and engine timing need server telemetry.",rows.len(),numeric(last,"received_bytes").map(|v|format!("{v:.0} B")).unwrap_or("Unavailable".into()),numeric(last,"sent_bytes").map(|v|format!("{v:.0} B")).unwrap_or("Unavailable".into()))))]));
    let identity=if filter==&Null{"All models".into()}else{format!("{} · {}",model_label(filter),text(filter,"endpoint"))};
    obj(vec![("subtitle",s(&format!("{identity} · {} · {} recorded requests",if get(st,"request_record")!=&Null{"Request in progress"}else{"Request history"},rows.len()))),
        ("window",Num(count.to_string())),("gauges",Arr(gauges)),("cards",Arr(cards)),
        ("actions",Arr(vec![button("Server monitor","server_monitor"),button("Models and statistics","monitor:models"),button("All models","monitor:all"),button("Request details","monitor:details"),button("Measurement details","monitor:measurements")]))])
}
fn model_label(row:&Json)->String{let name=text(row,"model");if name.is_empty(){"Model unrecorded".into()}else{name}}
fn statistic(row:&Json,key:&str,unit:&str)->String{label(numeric(row,key),unit)}
fn models(st:&Json)->Json{
    let groups=request_metrics::groups(st);
    let mut fields=vec![field_ui("basis","Phone observations","Up to 16 recently used model/endpoint pairs from the last 128 requests. p50/p95 use nearest rank on successful timed requests. Missing timings are excluded. Character throughput divides summed observed reply characters by summed complete request time; it is not token throughput. Errors exclude cancellations and unknown outcomes.","readonly")];
    let mut buttons=vec![button("Show all models","monitor:all")];
    for (i,g) in groups.iter().enumerate(){
        fields.push(field_ui(&format!("model_{i}"),&format!("{} · {}",model_label(g),text(g,"endpoint")),
            &format!("{} requests · {} successful · {} errors · {} cancelled\nError rate: {}\nComplete request p50 / p95: {} / {} ({} samples)\nFirst text p50 / p95: {} / {} ({} samples)\nObserved text throughput: {} ({} complete replies)",number(g,"total"),number(g,"success"),number(g,"errors"),number(g,"cancelled"),statistic(g,"error_percent","%"),statistic(g,"elapsed_p50","ms"),statistic(g,"elapsed_p95","ms"),number(g,"elapsed_samples"),statistic(g,"first_text_p50","ms"),statistic(g,"first_text_p95","ms"),number(g,"first_text_samples"),statistic(g,"characters_per_second","chars/s"),number(g,"rate_samples")),"readonly"));
        buttons.push(button(&format!("Charts: {} · {}",model_label(g),text(g,"endpoint")),&format!("monitor:model:{}",text(g,"request_id"))));
    }
    modal("Models and observations",fields,buttons)
}
pub(super) fn event(st:&mut Json,a:&str)->Result<Json>{
    if a=="monitor:models"{put(st,"dialog",s("monitor_models"));return Ok(Null);}
    if a=="monitor:all"{put(st,"monitor_model",Null);}
    else if let Some(id)=a.strip_prefix("monitor:model:"){
        let row=array(st,"requests").into_iter().rev().take(128).find(|r|text(r,"id")==id).ok_or("Model request is no longer in the retained history")?;
        put(st,"monitor_model",request_metrics::identity(&row));
    }else if let Some(raw)=a.strip_prefix("monitor:window:"){
        let n=raw.parse::<usize>().map_err(|_|"Invalid monitor range")?;
        if ![16,32,64,128].contains(&n){return Err("Choose 16, 32, 64 or 128 requests".into());}
        put(st,"monitor_window",Num(n.to_string()));
    }else if let Some(id)=a.strip_prefix("monitor:request:"){
        if !array(st,"requests").iter().any(|r|text(r,"id")==id){return Err("Request is no longer in the retained history".into());}
        put(st,"monitor_request",s(id));put(st,"dialog",s("monitor_request"));return Ok(Null);
    }else if a=="monitor:details"{put(st,"dialog",s("monitor_details"));return Ok(Null);}
    else if a=="monitor:measurements"{put(st,"dialog",s("diagnostics"));return Ok(Null);}
    put(st,"page",s("diagnostics"));put(st,"dialog",Null);Ok(Null)
}
pub(super) fn detail(st:&Json)->Json{
    if text(st,"dialog")=="monitor_models"{return models(st);}
    let rows=array(st,"requests");
    if text(st,"dialog")=="monitor_details" {
        return modal("Request details",vec![],rows.iter().rev().take(128).map(|r|button(
            &format!("{} · {} · {}",if text(r,"model").is_empty(){"Model unrecorded".into()}else{text(r,"model")},text(r,"outcome"),label(numeric(r,"elapsed_ms"),"ms")),
            &format!("monitor:request:{}",text(r,"id")))).collect());
    }
    let r=rows.iter().find(|r|text(r,"id")==text(st,"monitor_request")).unwrap_or(&Null);
    modal("Request observation",vec![
        field_ui("model","Model",if text(r,"model").is_empty(){"Not recorded for this historical request"}else{get(r,"model").text().unwrap_or("")},"readonly"),
        field_ui("endpoint","Endpoint",&text(r,"endpoint"),"readonly"),
        field_ui("outcome","Outcome",&text(r,"outcome"),"readonly"),
        field_ui("timing","Phone measurements",&format!("Elapsed: {}\nFirst byte: {}\nFirst text: {}\nText rate: {}",label(numeric(r,"elapsed_ms"),"ms"),label(numeric(r,"first_byte_ms"),"ms"),label(numeric(r,"first_text_ms"),"ms"),label(series(r,"characters"),"chars/s")),"readonly"),
        field_ui("estimate","App-calculated estimated tokens/s",&format!("{}\n{}",throughput::label(get(r,"local_rate")),throughput::METHOD),"readonly"),
        field_ui("token","Provider token count / phone clock",&label(series(r,"tokens"),"tokens/s"),"readonly"),
    ],vec![])
}

#[cfg(test)]
mod checks{
    use super::*;
    #[test] fn missing_observations_are_gaps_not_zeroes(){
        let st=obj(vec![]);let out=dashboard(&st);
        for gauge in array(&out,"gauges"){assert_eq!(text(&gauge,"value"),"Unavailable");}
        assert!(!out.string().contains("NaN"));assert!(!out.string().contains("Infinity"));
        let rows=vec![obj(vec![("elapsed_ms",Num("0".into()))]),obj(vec![]),obj(vec![("elapsed_ms",Num("100".into()))])];
        let chart=plot(&rows,"Latency",&[("elapsed_ms","Total","accent")],"ms");
        // Three grid lines, no line spanning the missing middle request.
        assert_eq!(array(get(&chart,"visual"),"shapes").iter().filter(|p|text(p,"kind")=="line").count(),3);
    }
    #[test] fn request_window_is_bounded_and_cancellation_is_not_an_error(){
        let mut rows=vec![];
        for i in 0..128{rows.push(obj(vec![("id",s(&i.to_string())),("outcome",s(if i==127{"cancelled"}else{"success"})),("elapsed_ms",Num("100".into()))]));}
        let mut st=obj(vec![("requests",Arr(rows)),("monitor_window",Num("16".into()))]);
        let out=dashboard(&st);assert!(text(&out,"subtitle").contains("16 recorded"));
        assert_eq!(text(&array(&out,"gauges")[3],"value"),"0.0 %");
        assert!(event(&mut st,"monitor:window:999").is_err());
        assert!(event(&mut st,"monitor:request:127").is_ok());
        assert!(event(&mut st,"monitor:request:absent").is_err());
    }
}
