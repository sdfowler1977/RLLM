//! Bounded phone observations grouped by the identity saved with each request.
use super::*;

const HISTORY: usize = 128;
const GROUPS: usize = 16;

fn measured(row: &Json, key: &str, maximum: u64) -> Option<u64> {
    match get(row, key) {
        Num(value) => value.parse::<u64>().ok().filter(|&v| v <= maximum),
        _ => None,
    }
}
fn percentile(values: &mut [u64], percent: usize) -> Json {
    if values.is_empty() { return Null; }
    values.sort_unstable();
    Num(values[(values.len() * percent + 99) / 100 - 1].to_string())
}
pub(super) fn identity(row: &Json) -> Json {
    obj(vec![("model", s(&text(row, "model"))), ("endpoint", s(&text(row, "endpoint")))])
}
pub(super) fn same_identity(row: &Json, key: &Json) -> bool {
    text(row, "model") == text(key, "model") && text(row, "endpoint") == text(key, "endpoint")
}
pub(super) fn groups(st: &Json) -> Vec<Json> {
    let all = array(st, "requests");
    let rows = &all[all.len().saturating_sub(HISTORY)..];
    let mut keys = Vec::new();
    for row in rows.iter().rev() {
        let key = identity(row);
        if !keys.contains(&key) { keys.push(key); }
        if keys.len() == GROUPS { break; }
    }
    keys.into_iter().map(|mut group| {
        let mut total = 0; let mut success = 0; let mut errors = 0; let mut cancelled = 0;
        let mut elapsed = Vec::new(); let mut first = Vec::new();
        let mut chars = 0u64; let mut time = 0u64; let mut measured_count = 0;
        let mut request_id = String::new();
        for row in rows.iter().filter(|r| same_identity(r, &group)) {
            total += 1; request_id = text(row, "id");
            match text(row, "outcome").as_str() {
                "success" => {
                    success += 1;
                    if let Some(ms) = measured(row, "elapsed_ms", 3_600_000) {
                        elapsed.push(ms);
                        if let Some(value) = measured(row, "first_text_ms", ms) { first.push(value); }
                        if ms > 0 {
                            if let Some(count) = measured(get(row, "telemetry"), "characters", 1_048_576) {
                                chars += count; time += ms; measured_count += 1;
                            }
                        }
                    }
                },
                "error" => errors += 1,
                "cancelled" => cancelled += 1,
                _ => {},
            }
        }
        for (key, count) in [("total",total),("success",success),("errors",errors),("cancelled",cancelled),("rate_samples",measured_count)] {
            put(&mut group, key, Num(count.to_string()));
        }
        put(&mut group, "request_id", s(&request_id));
        put(&mut group, "elapsed_samples", Num(elapsed.len().to_string()));
        put(&mut group, "first_text_samples", Num(first.len().to_string()));
        put(&mut group, "elapsed_p50", percentile(&mut elapsed, 50));
        put(&mut group, "elapsed_p95", percentile(&mut elapsed, 95));
        put(&mut group, "first_text_p50", percentile(&mut first, 50));
        put(&mut group, "first_text_p95", percentile(&mut first, 95));
        put(&mut group, "error_percent", if success + errors == 0 {Null} else {Num(format!("{:.3}", errors as f64 * 100.0 / (success + errors) as f64))});
        put(&mut group, "characters_per_second", if time == 0 {Null} else {Num(format!("{:.3}", chars as f64 * 1000.0 / time as f64))});
        group
    }).collect()
}

#[cfg(test)] mod checks {
    use super::*;
    fn row(id: usize, model: &str, outcome: &str, ms: u64, chars: u64) -> Json {
        obj(vec![("id",s(&id.to_string())),("model",s(model)),("endpoint",s("server/v1")),("outcome",s(outcome)),("elapsed_ms",Num(ms.to_string())),("telemetry",obj(vec![("characters",Num(chars.to_string()))]))])
    }
    #[test] fn rates_use_summed_count_and_time_excluding_errors_and_cancellations() {
        let st=obj(vec![("requests",Arr(vec![row(1,"m","success",100,10),row(2,"m","success",900,90),row(3,"m","error",1,999),row(4,"m","cancelled",1,999),row(5,"m","unknown",1,999)]))]);
        let g=groups(&st).remove(0);
        assert_eq!(get(&g,"characters_per_second"),&Num("100.000".into()));
        assert_eq!(get(&g,"error_percent"),&Num("33.333".into()));
        assert_eq!(number(&g,"rate_samples"),2);
    }
    #[test] fn nearest_rank_missing_values_and_first_text_bounds() {
        let mut rows=vec![row(0,"m","success",0,0)];
        for i in 1..=20 {let mut r=row(i,"m","success",i as u64*100,10);put(&mut r,"first_text_ms",Num((i*10).to_string()));rows.push(r);}
        let mut missing=row(21,"m","success",10,10);put(&mut missing,"elapsed_ms",Null);rows.push(missing);
        let mut invalid=row(22,"m","error",1,10);put(&mut invalid,"first_text_ms",Num("999".into()));rows.push(invalid);
        let g=groups(&obj(vec![("requests",Arr(rows))])).remove(0);
        assert_eq!(number(&g,"elapsed_p50"),1000);assert_eq!(number(&g,"elapsed_p95"),1900);assert_eq!(number(&g,"first_text_p95"),190);assert_eq!(number(&g,"first_text_samples"),20);
        assert_eq!(percentile(&mut [],95),Null);
    }
    #[test] fn grouping_uses_historical_model_and_endpoint_with_fixed_limits() {
        let mut rows=vec![row(0,"old","error",1,0)];for i in 1..=128 {rows.push(row(i,&format!("model{}",i%20),"success",100,5));}
        let st=obj(vec![("requests",Arr(rows)),("request_model",s("current-is-not-history"))]);
        let g=groups(&st);assert_eq!(g.len(),16);assert_eq!(text(&g[0],"model"),"model8");assert!(g.iter().all(|r|text(r,"model")!="old"));
        let one=row(1,"same","success",100,5);let mut two=one.clone();put(&mut two,"endpoint",s("other/v1"));
        assert_eq!(groups(&obj(vec![("requests",Arr(vec![one,two]))])).len(),2);
    }
}
