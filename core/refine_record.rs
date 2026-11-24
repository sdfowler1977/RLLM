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
