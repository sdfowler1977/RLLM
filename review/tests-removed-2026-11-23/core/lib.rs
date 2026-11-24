#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn json_roundtrip() {
        for v in [
            "quote \" newline\n tab\t backslash\\",
            "中文 café \u{1D11E}",
            "\0\u{001f}",
        ] {
            assert_eq!(parse(&quote(v)).unwrap(), s(v));
        }
        assert_eq!(parse("\"\\ud834\\udd1e\"").unwrap(), s("\u{1D11E}"));
    }
    #[test]
    fn bad_json() {
        for v in [
            "{\"x\":1,\"x\":2}",
            "[1,]",
            "01",
            "1e",
            "\"\\ud800\"",
            "true x",
            "\"\n\"",
        ] {
            assert!(parse(v).is_err(), "{v}");
        }
    }
    #[test]
    fn formats() {
        for protocol in ["chat", "local", "messages", "content"] {
            let j = obj(vec![
                ("protocol", s(protocol)),
                ("model", s("custom/model")),
                (
                    "messages",
                    Arr(vec![
                        obj(vec![("role", s("system")), ("content", s("Instructions"))]),
                        obj(vec![("role", s("user")), ("content", s("one"))]),
                        obj(vec![("role", s("assistant")), ("content", s("two"))]),
                        obj(vec![("role", s("user")), ("content", s("three"))]),
                    ]),
                ),
            ]);
            let r = request(&j).unwrap();
            assert!(r.string().contains("three"));
            assert!(r.string().contains("Instructions"));
        }
    }
    #[test]
    fn outputs() {
        for (protocol, body) in [
            (
                "chat",
                r#"{"choices":[{"message":{"content":"a\"b\n中文"}}]}"#,
            ),
            ("local", r#"{"message":{"content":"a\"b\n中文"}}"#),
            (
                "messages",
                r#"{"content":[{"type":"text","text":"a\"b\n中文"}]}"#,
            ),
            (
                "content",
                r#"{"candidates":[{"content":{"parts":[{"text":"a\"b\n中文"}]}}]}"#,
            ),
        ] {
            assert_eq!(
                response(&obj(vec![("protocol", s(protocol)), ("body", s(body))])).unwrap(),
                s("a\"b\n中文")
            );
        }
    }
    #[test]
    fn scoped_failure() {
        assert!(envelope(r#"{"op":"request"}"#).contains("error"));
        assert_eq!(
            envelope(r#"{"op":"validate","value":{"ready":true}}"#),
            r#"{"value":{"ready":true}}"#
        );
    }
    #[test]
    fn the_turn_window_is_clamped_and_the_elapsed_time_is_formatted() {
        // The window is reachable only through the turn operation, so the
        // clamp and the elapsed text cannot be reached by a second path.
        let low = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":1}"#).unwrap();
        assert_eq!(low.get("window_ms").unwrap(), &Num("30000".into()));
        assert_eq!(low.get("lease_ms").unwrap(), &Num("30000".into()));
        let high = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":9000000}"#).unwrap();
        assert_eq!(high.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(high.get("lease_ms").unwrap(), &Num("120000".into()));
        let mid = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":120000}"#).unwrap();
        assert_eq!(mid.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(mid.get("lease_ms").unwrap(), &Num("120000".into()));
        let edge = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":30000}"#).unwrap();
        assert_eq!(edge.get("window_ms").unwrap(), &Num("30000".into()));
        let cap = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":600000}"#).unwrap();
        assert_eq!(cap.get("window_ms").unwrap(), &Num("120000".into()));
        let over = operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":600001}"#).unwrap();
        assert_eq!(over.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":0,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":60000,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "1m 0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":125000,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "2m 5s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"tick","elapsed_ms":-1500,"lease_ms":120000}"#).unwrap().get("elapsed_text").unwrap().text().unwrap(),
            "0s"
        );
        assert_eq!(
            operation(r#"{"op":"turn","action":"begin","ticket":"t","window_ms":"no"}"#).unwrap_err(),
            "Invalid window_ms"
        );
    }
    #[test]
    fn depth_limit() {
        assert!(parse(&("[".repeat(66) + "0" + &"]".repeat(66))).is_err());
    }
    fn turn_request(fields: Vec<(&str, Json)>) -> Json {
        let mut all = vec![("op", s("turn"))];
        all.extend(fields);
        obj(all)
    }
    #[test]
    fn a_turn_keeps_a_durable_identity_and_the_core_lease() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-1")),
                ("window_ms", Num("120000".into())),
                ("started_ms", Num("7".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(begun.get("decision").unwrap().text().unwrap(), "queued");
        assert_eq!(begun.get("lease_ms").unwrap(), &Num("120000".into()));
        assert_eq!(begun.get("window_ms").unwrap(), &Num("120000".into()));
        assert_eq!(begun.get("cadence_ms").unwrap(), &Num("1000".into()));
        assert_eq!(begun.get("label").unwrap().text().unwrap(), "Replying");
        assert_eq!(begun.get("title").unwrap().text().unwrap(), "RLLM is working");
        assert_eq!(begun.get("channel").unwrap().text().unwrap(), "turn");
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        assert!(!holder.is_empty());
        assert_eq!(
            begun
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-1"
        );
        assert_eq!(
            begun
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("holder")
                .unwrap()
                .text()
                .unwrap(),
            holder
        );
        let clamped = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-2")),
                ("window_ms", Num("9000000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(clamped.get("lease_ms").unwrap(), &Num("120000".into()));
        assert_eq!(clamped.get("window_ms").unwrap(), &Num("120000".into()));
    }
    #[test]
    fn the_promotion_of_a_turn_is_the_time_its_request_has_left() {
        // A request that has already spent part of its window gets a promotion
        // that ends with it, never one that outlives it.
        let late = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-late")),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("30000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(late.get("lease_ms").unwrap(), &Num("90000".into()));
        assert_eq!(late.get("window_ms").unwrap(), &Num("120000".into()));
        // A request whose window is spent gets no promotion at all.
        let spent = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-spent")),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(spent.get("lease_ms").unwrap(), &Num("0".into()));
        // The extension of a running turn is decided the same way.
        let admitted = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-run")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = admitted.get("holder").unwrap().text().unwrap().to_string();
        let state = admitted.get("state").unwrap().clone();
        let extended = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state),
                ("ticket", s("ticket-run")),
                ("holder", s(&holder)),
                ("window_ms", Num("120000".into())),
                ("elapsed_ms", Num("45000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(extended.get("extended").unwrap(), &Bool(true));
        assert_eq!(extended.get("lease_ms").unwrap(), &Num("75000".into()));
        assert_eq!(extended.get("window_ms").unwrap(), &Num("120000".into()));
    }
    #[test]
    fn a_turn_reads_two_clocks_and_never_rewinds_its_own_progress() {
        // The request has run for a minute. The promotion was decided a quarter
        // of a minute ago and its lease is thirty seconds.
        let tick = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_elapsed_ms", Num("15000".into())),
                ("lease_ms", Num("30000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(tick.get("expired").unwrap(), &Bool(false));
        assert_eq!(
            tick.get("progress_text").unwrap().text().unwrap(),
            "Replying · 1m 0s"
        );
        assert_eq!(tick.get("elapsed_text").unwrap().text().unwrap(), "1m 0s");
        // The promotion's own clock expires while the request is still running,
        // and the request's progress is still the request's own.
        let expired = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_elapsed_ms", Num("30000".into())),
                ("lease_ms", Num("30000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(expired.get("expired").unwrap(), &Bool(true));
        assert_eq!(
            expired.get("progress_text").unwrap().text().unwrap(),
            "Replying · 1m 0s"
        );
        assert_eq!(expired.get("elapsed_text").unwrap().text().unwrap(), "1m 0s");
    }
    #[test]
    fn a_turn_without_an_identity_is_refused_and_a_live_turn_is_held() {
        let refused = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(refused.get("decision").unwrap().text().unwrap(), "refused");
        assert_eq!(refused.get("state").unwrap().get("turn_expected").unwrap(), &Null);
        let held = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-3")),
                ("live_ticket", s("ticket-1")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(held.get("decision").unwrap().text().unwrap(), "held");
    }
    #[test]
    fn a_turn_reports_its_progress_and_its_expiry_from_the_core() {
        let tick = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("59000".into())),
                ("lease_ms", Num("60000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(tick.get("expired").unwrap(), &Bool(false));
        assert_eq!(tick.get("progress_text").unwrap().text().unwrap(), "Replying · 59s");
        assert_eq!(tick.get("elapsed_text").unwrap().text().unwrap(), "59s");
        let late = operation(
            &turn_request(vec![
                ("action", s("tick")),
                ("elapsed_ms", Num("60000".into())),
                ("lease_ms", Num("60000".into())),
                ("label", s("Replying")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(late.get("expired").unwrap(), &Bool(true));
    }
    #[test]
    fn the_expiry_notice_is_the_refused_start_notice() {
        let expire = operation(&turn_request(vec![("action", s("expire"))]).string()).unwrap();
        assert_eq!(
            expire.get("notice").unwrap().text().unwrap(),
            "Background protection was lost. The turn still runs."
        );
    }
    #[test]
    fn the_reply_text_limit_and_the_titles_are_owned_by_the_core() {
        let body = "x".repeat(900);
        let published = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s(&body)),
                ("chat", s("chat-1")),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(published.get("decision").unwrap().text().unwrap(), "publish");
        assert_eq!(published.get("title").unwrap().text().unwrap(), "RLLM reply");
        assert_eq!(published.get("channel").unwrap().text().unwrap(), "replies");
        let text = published.get("text").unwrap().text().unwrap();
        assert_eq!(text.chars().count(), 601);
        assert!(text.ends_with('…'));
        let failed = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("boom")),
                ("failed", Bool(true)),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(failed.get("title").unwrap().text().unwrap(), "RLLM turn failed");
        let foreground = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("boom")),
                ("backgrounded", Bool(false)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreground.get("decision").unwrap().text().unwrap(), "suppress");
    }
    #[test]
    fn a_held_reply_is_announced_after_a_resume() {
        let held = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", obj(vec![])),
                ("body", s("the answer")),
                ("chat", s("chat-9")),
                ("backgrounded", Bool(true)),
                ("notifications_allowed", Bool(false)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(held.get("decision").unwrap().text().unwrap(), "hold");
        let state = held.get("state").unwrap().clone();
        assert_eq!(
            state.get("turn_pending").unwrap().get("text").unwrap().text().unwrap(),
            "the answer"
        );
        let drained = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", state),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(false)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(drained.get("decision").unwrap().text().unwrap(), "surface");
        assert_eq!(drained.get("text").unwrap().text().unwrap(), "the answer");
        assert_eq!(drained.get("chat").unwrap().text().unwrap(), "chat-9");
        // A reply that could not be shown is still held, so a later grant
        // announces it instead of dropping it.
        assert_eq!(
            drained
                .get("state")
                .unwrap()
                .get("turn_pending")
                .unwrap()
                .get("text")
                .unwrap()
                .text()
                .unwrap(),
            "the answer"
        );
        let granted = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", drained.get("state").unwrap().clone()),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(granted.get("decision").unwrap().text().unwrap(), "publish");
        assert_eq!(granted.get("text").unwrap().text().unwrap(), "the answer");
        assert_eq!(granted.get("state").unwrap().get("turn_pending").unwrap(), &Null);
        let again = operation(
            &turn_request(vec![
                ("action", s("publish")),
                ("state", granted.get("state").unwrap().clone()),
                ("drain", Bool(true)),
                ("notifications_allowed", Bool(true)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(again.get("decision").unwrap().text().unwrap(), "suppress");
    }
    #[test]
    fn a_recreated_activity_rebinds_to_the_live_turn_by_its_identity() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-7")),
                ("window_ms", Num("120000".into())),
                ("started_ms", Num("3".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let rebound = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("ticket-7")),
                ("live_holder", s(&holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(rebound.get("decision").unwrap().text().unwrap(), "rebound");
        assert_eq!(rebound.get("ticket").unwrap().text().unwrap(), "ticket-7");
        assert_eq!(
            rebound.get("state").unwrap().get("model_expected").unwrap().text().unwrap(),
            "ticket-7"
        );
        // A live turn that this process does not hold the lease of is not
        // adopted, even when the request identity is the same.
        let foreign = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("ticket-7")),
                ("live_holder", s("h-foreign")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreign.get("decision").unwrap().text().unwrap(), "uncertain");
    }
    #[test]
    fn an_uncertain_turn_is_never_replayed() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-8")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let uncertain = operation(
            &turn_request(vec![
                ("action", s("rebind")),
                ("state", begun.get("state").unwrap().clone()),
                ("live_ticket", s("")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(uncertain.get("decision").unwrap().text().unwrap(), "uncertain");
        assert_eq!(
            uncertain.get("state").unwrap().get("notice").unwrap().text().unwrap(),
            "The previous turn did not finish. Nothing was replayed."
        );
        assert_eq!(uncertain.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn only_the_turn_that_owns_the_identity_retires_it() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-9")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let state = begun.get("state").unwrap().clone();
        let stale = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state.clone()),
                ("ticket", s("ticket-8")),
                ("holder", s("h-other")),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale.get("retired").unwrap(), &Bool(false));
        assert_eq!(
            stale
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-9"
        );
        let own = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state),
                ("ticket", s("ticket-9")),
                ("holder", s(&holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("retired").unwrap(), &Bool(true));
        assert_eq!(own.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn only_the_holder_may_extend_the_lease() {
        let begun = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-10")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let holder = begun.get("holder").unwrap().text().unwrap().to_string();
        let state = begun.get("state").unwrap().clone();
        let own = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-10")),
                ("holder", s(&holder)),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("extended").unwrap(), &Bool(true));
        assert_eq!(own.get("lease_ms").unwrap(), &Num("120000".into()));
        // The lease decision writes nothing: begin is the only writer of the
        // identity, so an extension can never take it over.
        assert_eq!(
            own.get("state").unwrap().get("turn_expected").unwrap(),
            state.get("turn_expected").unwrap()
        );
        let foreign = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-10")),
                ("holder", s("h-other")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(foreign.get("extended").unwrap(), &Bool(false));
        assert_eq!(
            foreign.get("state").unwrap().get("turn_expected").unwrap(),
            state.get("turn_expected").unwrap()
        );
    }
    #[test]
    fn a_stale_teardown_leaves_the_newer_live_turn_alone() {
        let first = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", obj(vec![])),
                ("ticket", s("ticket-11")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        let first_holder = first.get("holder").unwrap().text().unwrap().to_string();
        let second = operation(
            &turn_request(vec![
                ("action", s("begin")),
                ("state", first.get("state").unwrap().clone()),
                ("ticket", s("ticket-12")),
                ("live_ticket", s("ticket-11")),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(second.get("decision").unwrap().text().unwrap(), "held");
        let second_holder = second.get("holder").unwrap().text().unwrap().to_string();
        assert_ne!(first_holder, second_holder);
        let state = second.get("state").unwrap().clone();
        // The turn that is already over may not retire the identity of the
        // newer live turn, and it may not extend its lease either.
        let stale = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state.clone()),
                ("ticket", s("ticket-11")),
                ("holder", s(&first_holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale.get("retired").unwrap(), &Bool(false));
        assert_eq!(
            stale
                .get("state")
                .unwrap()
                .get("turn_expected")
                .unwrap()
                .get("ticket")
                .unwrap()
                .text()
                .unwrap(),
            "ticket-12"
        );
        let stale_lease = operation(
            &turn_request(vec![
                ("action", s("extend")),
                ("state", state.clone()),
                ("ticket", s("ticket-12")),
                ("holder", s(&first_holder)),
                ("window_ms", Num("120000".into())),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(stale_lease.get("extended").unwrap(), &Bool(false));
        let own = operation(
            &turn_request(vec![
                ("action", s("finish")),
                ("state", state),
                ("ticket", s("ticket-12")),
                ("holder", s(&second_holder)),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(own.get("retired").unwrap(), &Bool(true));
        assert_eq!(own.get("state").unwrap().get("turn_expected").unwrap(), &Null);
    }
    #[test]
    fn binary_network_zip_reaches_import_preview() {
        let root = std::env::current_dir()
            .unwrap()
            .join("toolchain")
            .join(format!(
                "client-network-zip-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("binary.dat"), [0, 255, 1]).unwrap();
        let built = operation(
            &obj(vec![
                ("op", s("project_archive")),
                ("root", s(root.to_str().unwrap())),
            ])
            .string(),
        )
        .unwrap();
        let zip = network::unbase64(built.get("archive").unwrap().text().unwrap()).unwrap();
        let mut response =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", zip.len()).into_bytes();
        response.extend_from_slice(&zip);
        let extracted = operation(
            &obj(vec![
                ("op", s("network_binary")),
                ("data", s(&network::b64(&response))),
            ])
            .string(),
        )
        .unwrap();
        let preview = operation(
            &obj(vec![
                ("op", s("archive_preview")),
                ("archive", extracted.get("archive").unwrap().clone()),
            ])
            .string(),
        )
        .unwrap();
        assert_eq!(preview.get("files").unwrap(), &Num("1".into()));
        assert!(preview.string().contains("binary.dat"));
        assert_eq!(
            network::unbase64(extracted.get("archive").unwrap().text().unwrap()).unwrap(),
            zip
        );
        // Preserve this owned verification fixture for review.
    }
}
