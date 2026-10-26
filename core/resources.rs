use super::*;
fn integer(j: &Json, k: &str) -> u64 {
    match j.get(k) {
        Ok(Num(n)) | Ok(Str(n)) => n.parse().unwrap_or(0),
        _ => 0,
    }
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
        ("workers", Num(cores.clamp(1, 4).to_string())),
        (
            "measured",
            Bool(measured_heap > 0 && measured_system > 0 && cores > 0),
        ),
    ])
}
#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn default_budget() {
        let device = obj(vec![
            ("heap_bytes", Num((256 * 1024 * 1024u64).to_string())),
            ("memory_bytes", Num((4 * 1024 * 1024 * 1024u64).to_string())),
            ("cores", Num("8".into())),
        ]);
        let p = policy(&device, &Null, 0);
        assert_eq!(integer(&p, "budget_bytes"), 128 * 1024 * 1024);
        assert_eq!(integer(&p, "workers"), 4);
        assert_eq!(integer(&p, "response_bytes"), 4 * 1024 * 1024);
    }
    #[test]
    fn no_measurement_is_not_a_gate() {
        let p = policy(&Null, &Null, 0);
        assert!(integer(&p, "response_bytes") > 0);
        assert_eq!(p.get("measured"), Ok(&Bool(false)));
    }
    #[test]
    fn envelope_limit() {
        let p = policy(&Null, &Null, 7 * 1024 * 1024);
        assert!(integer(&p, "response_bytes") < 1024 * 1024);
    }
}
