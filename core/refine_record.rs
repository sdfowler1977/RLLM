//! Bounded refine record.
//! At most eight rounds are recorded. The edit budget is a byte count.
//! The edit history holds at most 32 entries. Two rounds with the same score
//! set the stall flag. A held-out phrase in a candidate sets the leakage flag.
//! A candidate is kept only when its score gain is above the noise floor, its
//! cost fits the remaining byte budget, and leakage is absent.
//! STOP ends the record. The record does not execute instructions and does not
//! restart the product process.

#![forbid(unsafe_code)]

pub const ROUND_LIMIT: usize = 8;
pub const HISTORY_LIMIT: usize = 32;

pub const NOISE_REFUSAL: &str = "gain inside the noise floor was refused";
pub const LEAKAGE_REFUSAL: &str = "leakage was refused";
pub const BUDGET_REFUSAL: &str = "cost outside the byte budget was refused";
pub const KEPT_SENTENCE: &str = "candidate was kept";
pub const LATER_EDITS: &str = "later edits were not stored";
pub const LATER_ROUNDS: &str = "later rounds were not run";
pub const STOP_SENTENCE: &str = "STOP ends the record";
pub const INSTRUCTIONS_SENTENCE: &str = "the instructions were not executed";
pub const PROCESS_SENTENCE: &str = "the product process was not restarted";

struct Round {
    score: i64,
    gain: i64,
    cost: usize,
    kept: bool,
    leakage: bool,
    stall: bool,
}

struct Edit {
    round: usize,
    cost: usize,
    kept: bool,
}

pub struct RefineRecord {
    budget_bytes: usize,
    spent_bytes: usize,
    noise_floor: i64,
    held_out: String,
    rounds: Vec<Round>,
    history: Vec<Edit>,
    stall: bool,
    leakage: bool,
    refused_noise: bool,
    refused_leakage: bool,
    refused_budget: bool,
    kept_any: bool,
    history_capped: bool,
    later_rounds: bool,
    stopped: bool,
}

impl RefineRecord {
    /// Open a record. `budget_bytes` is the edit budget. `noise_floor` is the
    /// gain that must be exceeded. `held_out` is the phrase that marks leakage.
    pub fn new(budget_bytes: usize, noise_floor: i64, held_out: &str) -> Self {
        Self {
            budget_bytes,
            spent_bytes: 0,
            noise_floor,
            held_out: held_out.to_string(),
            rounds: Vec::new(),
            history: Vec::new(),
            stall: false,
            leakage: false,
            refused_noise: false,
            refused_leakage: false,
            refused_budget: false,
            kept_any: false,
            history_capped: false,
            later_rounds: false,
            stopped: false,
        }
    }

    /// Judge one candidate as the next round.
    /// A ninth call records that later rounds were not run.
    pub fn propose(&mut self, score: i64, prior_score: i64, cost_bytes: usize, candidate: &str) {
        if self.stopped {
            return;
        }
        if self.rounds.len() >= ROUND_LIMIT {
            self.later_rounds = true;
            return;
        }
        if self.history.len() >= HISTORY_LIMIT {
            self.history_capped = true;
            return;
        }
        self.judge(score, prior_score, cost_bytes, candidate, true);
    }

    /// Judge another edit on the latest round. The history stops at 32 entries.
    pub fn record_edit(&mut self, score: i64, prior_score: i64, cost_bytes: usize, candidate: &str) {
        if self.stopped || self.rounds.is_empty() {
            return;
        }
        if self.history.len() >= HISTORY_LIMIT {
            self.history_capped = true;
            return;
        }
        self.judge(score, prior_score, cost_bytes, candidate, false);
    }

    /// End the record. Later proposals and edits are not run.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        for (index, round) in self.rounds.iter().enumerate() {
            lines.push(format!(
                "round {} score {} gain {} cost {} bytes kept {} leakage {} stall {}",
                index + 1,
                round.score,
                round.gain,
                round.cost,
                round.kept,
                round.leakage,
                round.stall
            ));
        }
        for (index, edit) in self.history.iter().enumerate() {
            lines.push(format!(
                "edit {} round {} cost {} bytes kept {}",
                index + 1,
                edit.round,
                edit.cost,
                edit.kept
            ));
        }
        let remaining = self.budget_bytes.saturating_sub(self.spent_bytes);
        lines.push(format!("rounds {} of {ROUND_LIMIT}", self.rounds.len()));
        lines.push(format!(
            "edit history {} of {HISTORY_LIMIT}",
            self.history.len()
        ));
        lines.push(format!(
            "edit budget {} bytes remaining {remaining} bytes",
            self.budget_bytes
        ));
        lines.push(format!("noise floor {}", self.noise_floor));
        lines.push(format!(
            "stall {}",
            if self.stall { "set" } else { "clear" }
        ));
        lines.push(format!(
            "leakage {}",
            if self.leakage { "set" } else { "clear" }
        ));
        if self.refused_noise {
            lines.push(NOISE_REFUSAL.to_string());
        }
        if self.refused_leakage {
            lines.push(LEAKAGE_REFUSAL.to_string());
        }
        if self.refused_budget {
            lines.push(BUDGET_REFUSAL.to_string());
        }
        if self.kept_any {
            lines.push(KEPT_SENTENCE.to_string());
        }
        if self.history_capped {
            lines.push(LATER_EDITS.to_string());
        }
        if self.later_rounds {
            lines.push(LATER_ROUNDS.to_string());
        }
        if self.stopped {
            lines.push(STOP_SENTENCE.to_string());
        }
        lines.push(INSTRUCTIONS_SENTENCE.to_string());
        lines.push(PROCESS_SENTENCE.to_string());
        lines.join("\n")
    }

    /// Judge one live round and return the record.
    pub fn judge_round(score: i64, prior_score: i64, cost_bytes: usize, candidate: &str, edit: &str, held_out: &str) -> String {
        let mut record = RefineRecord::new(32768, 0, held_out);
        if candidate == "STOP" {
            record.stop();
        } else {
            record.propose(score, prior_score, cost_bytes, candidate);
            if !edit.is_empty() && edit != candidate {
                record.record_edit(score, prior_score, edit.len(), edit);
            }
        }
        record.report()
    }

    /// A leakage or budget refusal blocks acceptance. A noise-floor line does not,
    /// because a preserved score is still a valid round. Instructions are not run.
    pub fn blocking_refusal(report: &str) -> Option<&'static str> {
        if report.contains(LEAKAGE_REFUSAL) {
            Some(LEAKAGE_REFUSAL)
        } else if report.contains(BUDGET_REFUSAL) {
            Some(BUDGET_REFUSAL)
        } else {
            None
        }
    }

    fn judge(
        &mut self,
        score: i64,
        prior_score: i64,
        cost_bytes: usize,
        candidate: &str,
        new_round: bool,
    ) {
        let leakage = phrase_present(&self.held_out, candidate);
        let gain = score.saturating_sub(prior_score);
        let remaining = self.budget_bytes.saturating_sub(self.spent_bytes);
        let outside_budget = cost_bytes > remaining;
        let inside_floor = gain <= self.noise_floor;
        let kept = !leakage && !outside_budget && !inside_floor;
        if kept {
            self.spent_bytes = self.spent_bytes.saturating_add(cost_bytes);
            self.kept_any = true;
        }
        if leakage {
            self.leakage = true;
            self.refused_leakage = true;
        }
        if outside_budget {
            self.refused_budget = true;
        }
        if inside_floor {
            self.refused_noise = true;
        }
        let stall = new_round && self.rounds.iter().any(|round| round.score == score);
        if stall {
            self.stall = true;
        }
        if new_round {
            self.rounds.push(Round {
                score,
                gain,
                cost: cost_bytes,
                kept,
                leakage,
                stall,
            });
        }
        let round = self.rounds.len();
        self.history.push(Edit {
            round,
            cost: cost_bytes,
            kept,
        });
    }
}

fn phrase_present(held_out: &str, candidate: &str) -> bool {
    !held_out.is_empty() && candidate.contains(held_out)
}

#[cfg(test)]
fn joined(lines: &[&str]) -> String {
    lines.join("\n")
}

#[cfg(test)]
#[test]
fn gain_inside_the_noise_floor_is_refused() {
    let mut record = RefineRecord::new(64, 2, "sealed-token");
    record.propose(5, 4, 4, "plain candidate");
    assert_eq!(
        record.report(),
        joined(&[
            "round 1 score 5 gain 1 cost 4 bytes kept false leakage false stall false",
            "edit 1 round 1 cost 4 bytes kept false",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 64 bytes remaining 64 bytes",
            "noise floor 2",
            "stall clear",
            "leakage clear",
            "gain inside the noise floor was refused",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );

    let mut same = RefineRecord::new(64, 2, "sealed-token");
    same.propose(2, 0, 1, "plain candidate");
    let report = same.report();
    assert!(
        report.contains("round 1 score 2 gain 2 cost 1 bytes kept false leakage false stall false\n"),
        "{report}"
    );
    assert!(report.contains("gain inside the noise floor was refused\n"), "{report}");
    assert!(!report.contains("candidate was kept"), "{report}");
}

#[cfg(test)]
#[test]
fn leakage_is_refused() {
    let mut leaked = RefineRecord::new(64, 0, "sealed-token");
    leaked.propose(10, 0, 4, "see sealed-token now");
    assert_eq!(
        leaked.report(),
        joined(&[
            "round 1 score 10 gain 10 cost 4 bytes kept false leakage true stall false",
            "edit 1 round 1 cost 4 bytes kept false",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 64 bytes remaining 64 bytes",
            "noise floor 0",
            "stall clear",
            "leakage set",
            "leakage was refused",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );

    let mut clean = RefineRecord::new(64, 0, "sealed-token");
    clean.propose(10, 0, 4, "plain candidate");
    let report = clean.report();
    assert!(
        report.contains("round 1 score 10 gain 10 cost 4 bytes kept true leakage false stall false\n"),
        "{report}"
    );
    assert!(report.contains("leakage clear\n"), "{report}");
    assert!(!report.contains("leakage was refused"), "{report}");

    let mut empty = RefineRecord::new(64, 0, "");
    empty.propose(3, 0, 1, "sealed-token");
    let report = empty.report();
    assert!(report.contains("leakage clear\n"), "{report}");
    assert!(report.contains("kept true"), "{report}");
}

#[cfg(test)]
#[test]
fn the_ninth_round_says_later_rounds_were_not_run() {
    let mut record = RefineRecord::new(8, 0, "sealed-token");
    for score in 1..=8 {
        record.propose(score, score - 1, 1, "candidate");
    }
    record.propose(9, 8, 1, "ninth-body");
    let report = record.report();
    let rounds = report
        .lines()
        .filter(|line| line.starts_with("round "))
        .count();
    assert_eq!(rounds, 8, "{report}");
    assert!(report.contains("round 8 score 8 gain 1 cost 1 bytes kept true "), "{report}");
    assert!(
        report.contains("later rounds were not run\nthe instructions were not executed\nthe product process was not restarted"),
        "{report}"
    );
    assert!(!report.contains("round 9"), "{report}");
    assert!(!report.contains("ninth-body"), "{report}");
    assert!(!report.contains("STOP ends the record"), "{report}");
    assert!(report.contains("edit budget 8 bytes remaining 0 bytes\n"), "{report}");
}

#[cfg(test)]
#[test]
fn stop_ends_the_record() {
    let mut record = RefineRecord::new(64, 0, "sealed-token");
    record.propose(3, 0, 1, "candidate");
    record.stop();
    record.propose(9, 0, 1, "after-stop-body");
    assert_eq!(
        record.report(),
        joined(&[
            "round 1 score 3 gain 3 cost 1 bytes kept true leakage false stall false",
            "edit 1 round 1 cost 1 bytes kept true",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 64 bytes remaining 63 bytes",
            "noise floor 0",
            "stall clear",
            "leakage clear",
            "candidate was kept",
            "STOP ends the record",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );
    assert!(!record.report().contains("after-stop-body"));
    assert!(!record.report().contains("round 2 "));
}

#[cfg(test)]
#[test]
fn stall_flag_when_two_rounds_share_a_score() {
    let mut record = RefineRecord::new(32, 0, "sealed-token");
    record.propose(4, 0, 1, "first");
    record.propose(4, 0, 1, "second");
    let report = record.report();
    assert!(
        report.contains("round 1 score 4 gain 4 cost 1 bytes kept true leakage false stall false\n"),
        "{report}"
    );
    assert!(
        report.contains("round 2 score 4 gain 4 cost 1 bytes kept true leakage false stall true\n"),
        "{report}"
    );
    assert!(report.contains("stall set\n"), "{report}");
    assert!(!report.contains("stall clear"), "{report}");
}

#[cfg(test)]
#[test]
fn cost_outside_the_byte_budget_is_refused() {
    let mut record = RefineRecord::new(8, 0, "sealed-token");
    record.propose(9, 0, 9, "plain candidate");
    assert_eq!(
        record.report(),
        joined(&[
            "round 1 score 9 gain 9 cost 9 bytes kept false leakage false stall false",
            "edit 1 round 1 cost 9 bytes kept false",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 8 bytes remaining 8 bytes",
            "noise floor 0",
            "stall clear",
            "leakage clear",
            "cost outside the byte budget was refused",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );
}

#[cfg(test)]
#[test]
fn candidate_is_kept_when_gain_budget_and_leakage_allow() {
    let mut record = RefineRecord::new(8, 2, "sealed-token");
    record.propose(5, 2, 8, "plain candidate");
    assert_eq!(
        record.report(),
        joined(&[
            "round 1 score 5 gain 3 cost 8 bytes kept true leakage false stall false",
            "edit 1 round 1 cost 8 bytes kept true",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 8 bytes remaining 0 bytes",
            "noise floor 2",
            "stall clear",
            "leakage clear",
            "candidate was kept",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );
}

#[cfg(test)]
#[test]
fn edit_history_stops_at_32() {
    let mut record = RefineRecord::new(100, 0, "sealed-token");
    record.propose(1, 0, 0, "first");
    for _ in 0..31 {
        record.record_edit(1, 0, 0, "more");
    }
    record.record_edit(1, 0, 50, "sealed-token");
    record.propose(5, 0, 1, "round-two-body");
    let report = record.report();
    assert!(report.contains("edit 32 round 1 cost 0 bytes kept true\n"), "{report}");
    assert!(!report.contains("edit 33 "), "{report}");
    assert!(report.contains("rounds 1 of 8\n"), "{report}");
    assert!(report.contains("edit history 32 of 32\n"), "{report}");
    assert!(report.contains("later edits were not stored\n"), "{report}");
    assert!(report.contains("leakage clear\n"), "{report}");
    assert!(report.contains("edit budget 100 bytes remaining 100 bytes\n"), "{report}");
    assert!(!report.contains("round 2 "), "{report}");
    assert!(!report.contains("round-two-body"), "{report}");
    assert!(!report.contains("later rounds were not run"), "{report}");
    assert!(report.ends_with(
        "the instructions were not executed\nthe product process was not restarted"
    ));
}

#[cfg(test)]
#[test]
fn stop_candidate_ends_the_live_round() {
    let report = RefineRecord::judge_round(3, 1, 4, "STOP", "", "sealed-token");
    assert!(report.contains("STOP ends the record\n"), "{report}");
    assert!(!report.contains("round 1 "), "{report}");
    assert!(report.ends_with(
        "the instructions were not executed\nthe product process was not restarted"
    ));
}

#[cfg(test)]
#[test]
fn a_different_edit_is_stored_on_the_live_round() {
    let report = RefineRecord::judge_round(4, 1, 4, "selected", "other instruction", "sealed-token");
    assert!(report.contains("round 1 score 4 gain 3 cost 4 bytes kept true"), "{report}");
    assert!(report.contains("edit 2 round 1 cost 17 bytes kept true"), "{report}");
    assert!(report.ends_with(
        "the instructions were not executed\nthe product process was not restarted"
    ));
}

#[cfg(test)]
#[test]
fn leakage_and_budget_block_acceptance() {
    let leaked = RefineRecord::judge_round(4, 1, 4, "carries sealed-token", "", "sealed-token");
    assert_eq!(
        RefineRecord::blocking_refusal(&leaked),
        Some("leakage was refused")
    );
    let over = RefineRecord::judge_round(4, 1, 40_000, "plain", "", "");
    assert_eq!(
        RefineRecord::blocking_refusal(&over),
        Some("cost outside the byte budget was refused")
    );
    let kept = RefineRecord::judge_round(4, 1, 4, "plain", "", "");
    assert_eq!(RefineRecord::blocking_refusal(&kept), None);
}

#[cfg(test)]
#[test]
fn a_noise_floor_refusal_does_not_block_acceptance() {
    let noisy = RefineRecord::judge_round(4, 4, 4, "plain", "", "");
    assert!(
        noisy.contains("gain inside the noise floor was refused\n"),
        "{noisy}"
    );
    assert_eq!(RefineRecord::blocking_refusal(&noisy), None);
}

#[cfg(test)]
#[test]
fn later_edits_are_not_run_after_stop() {
    let mut record = RefineRecord::new(64, 0, "sealed-token");
    record.propose(3, 0, 1, "candidate");
    record.stop();
    record.record_edit(9, 0, 40_000, "sealed-token after-stop-edit");
    let report = record.report();
    assert_eq!(
        report,
        joined(&[
            "round 1 score 3 gain 3 cost 1 bytes kept true leakage false stall false",
            "edit 1 round 1 cost 1 bytes kept true",
            "rounds 1 of 8",
            "edit history 1 of 32",
            "edit budget 64 bytes remaining 63 bytes",
            "noise floor 0",
            "stall clear",
            "leakage clear",
            "candidate was kept",
            "STOP ends the record",
            "the instructions were not executed",
            "the product process was not restarted",
        ])
    );
    assert!(!report.contains("after-stop-edit"), "{report}");
    assert!(!report.contains("edit 2 "), "{report}");
    assert!(!report.contains("leakage was refused"), "{report}");
    assert!(!report.contains("cost outside the byte budget was refused"), "{report}");
    assert_eq!(RefineRecord::blocking_refusal(&report), None);
}
