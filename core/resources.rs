use super::*;
fn integer(j: &Json, k: &str) -> u64 {
    match j.get(k) {
        Ok(Num(n)) | Ok(Str(n)) => n.parse().unwrap_or(0),
        _ => 0,
    }
}
/// Current platform measurement takes precedence; absent metadata discovers the
/// usable host topology rather than silently selecting a single CPU.
pub(crate) fn workers(device:&Json)->usize{
    let measured=integer(device,"cores");
    let cores=if measured>0{measured}else{std::thread::available_parallelism().map(|n|n.get()as u64).unwrap_or(1)};
    cores.clamp(1,4)as usize
}
pub fn policy(device: &Json, config: &Json, state_bytes: usize) -> Json {
    let measured_heap = integer(device, "heap_bytes");
    let measured_system = integer(device, "memory_bytes");
    let cores = integer(device, "cores");
    let performance = config.get("performance") == Ok(&Bool(true));
    let percent = if performance {
        integer(config, "percent").clamp(50, 95)
    } else {
        50
    };
    // An absent measurement chooses a bounded conservative policy, never an offline state.
    let heap = if measured_heap > 0 {
        measured_heap
    } else {
        128 * 1024 * 1024
    };
    let system = if measured_system > 0 {
        measured_system
    } else {
        heap
    };
    let budget = heap.saturating_mul(percent) / 100;
    let budget = budget.min(system.saturating_mul(percent) / 100);
    // Reserve space for JSON/base64 copies, persisted state, controls and platform allocations.
    let native_room = (8 * 1024 * 1024usize)
        .saturating_sub(state_bytes + 262144)
        .saturating_mul(3)
        / 4;
    let response = (budget / 32).min(4 * 1024 * 1024).min(native_room as u64);
    obj(vec![
        (
            "mode",
            s(if performance {
                "Performance"
            } else {
                "Standard"
            }),
        ),
        ("percent", Num(percent.to_string())),
        ("budget_bytes", Num(budget.to_string())),
        ("response_bytes", Num(response.to_string())),
        ("workers", Num(workers(device).to_string())),
        (
            "measured",
            Bool(measured_heap > 0 && measured_system > 0 && cores > 0),
        ),
    ])
}
