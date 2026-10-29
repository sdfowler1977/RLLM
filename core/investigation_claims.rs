//! Immutable investigation claims and deterministic evidence comparisons.
//! The caller supplies authenticated observations and current access checks.
//! This module neither executes a proposal nor grants permission to execute it.
use std::collections::BTreeSet;

pub const MAX_REVISIONS: usize = 64;
pub const MAX_CITATIONS: usize = 16;
pub const MAX_TEXT: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Citation {
    pub record: String,
    pub snapshot: Option<u32>,
    pub node: Option<u32>,
}
impl Citation {
    pub fn validate(&self) -> Result<(), String> {
        if self.record.is_empty() || self.record.len() > 128 ||
            self.record.chars().any(char::is_control) ||
            (self.node.is_some() && self.snapshot.is_none()) {
            return Err("A citation requires an exact record and a valid snapshot/node scope".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    User,
    Model { agent: String, connection: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { File, Directory, Symlink, Other }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Criterion {
    Kind { source: Citation, expected: Kind },
    Size { source: Citation, expected: u64 },
    CompleteCaptureDigestEqual { left: Citation, right: Citation },
}
impl Criterion {
    pub fn sources(&self) -> Vec<&Citation> {
        match self {
            Self::Kind { source, .. } | Self::Size { source, .. } => vec![source],
            Self::CompleteCaptureDigestEqual { left, right } => vec![left, right],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub id: u64,
    pub previous: Option<u64>,
    pub question: String,
    pub hypothesis: String,
    pub origin: Origin,
    pub citations: Vec<Citation>,
    pub criterion: Option<Criterion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub previous: Option<u64>,
    pub question: String,
    pub hypothesis: String,
    pub citations: Vec<Citation>,
    pub criterion: Option<Criterion>,
}

/// State reconstructed from the caller's versioned encrypted storage. IDs are
/// allocated here, never accepted from model-authored draft content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Claims {
    revisions: Vec<Revision>,
}
impl Claims {
    pub fn revisions(&self) -> &[Revision] { &self.revisions }

    pub fn restore(revisions: Vec<Revision>) -> Result<Self, String> {
        if revisions.len() > MAX_REVISIONS { return Err("Too many retained claim revisions".into()); }
        let mut seen = BTreeSet::new();
        let mut last = 0;
        for revision in &revisions {
            if revision.id == 0 || revision.id <= last || !seen.insert(revision.id) {
                return Err("Invalid claim revision identity/order".into());
            }
            if revision.previous.is_some_and(|id| id >= revision.id || !seen.contains(&id)) {
                return Err("Claim revision references an unavailable predecessor".into());
            }
            validate_draft(&Draft { previous: revision.previous,
                question: revision.question.clone(), hypothesis: revision.hypothesis.clone(),
                citations: revision.citations.clone(), criterion: revision.criterion.clone() })?;
            validate_origin(&revision.origin)?;
            last = revision.id;
        }
        Ok(Self { revisions })
    }

    /// `accessible` must resolve each citation under current workspace/rules.
    /// It must not manufacture observations, rescan, or execute the source.
    pub fn append(&mut self, draft: Draft, origin: Origin,
        accessible: impl FnMut(&Citation) -> bool) -> Result<&Revision, String> {
        let id = self.revisions.last().map_or(Some(1), |r| r.id.checked_add(1))
            .ok_or("Claim identity space exhausted; existing revisions are retained")?;
        self.append_at(draft, origin, id, accessible)
    }

    /// Persistent callers keep a high-water mark even after explicit deletion.
    pub fn append_at(&mut self, draft: Draft, origin: Origin, id: u64,
        mut accessible: impl FnMut(&Citation) -> bool) -> Result<&Revision, String> {
        if id == 0 || self.revisions.last().is_some_and(|r| id <= r.id) {
            return Err("Claim identity would reuse an existing revision".into());
        }
        validate_draft(&draft)?;
        validate_origin(&origin)?;
        if self.revisions.len() >= MAX_REVISIONS {
            return Err("Claim storage is full; existing revisions are retained".into());
        }
        if draft.previous.is_some_and(|id| !self.revisions.iter().any(|r| r.id == id)) {
            return Err("Previous claim revision is unavailable".into());
        }
        for citation in &draft.citations {
            if !accessible(citation) { return Err("Cited observation is unavailable under current access".into()); }
        }
        self.revisions.push(Revision { id, previous: draft.previous, question: draft.question,
            hypothesis: draft.hypothesis, origin, citations: draft.citations, criterion: draft.criterion });
        Ok(self.revisions.last().expect("just appended"))
    }
}

fn validate_origin(origin: &Origin) -> Result<(), String> {
    if let Origin::Model { agent, connection } = origin {
        if agent.is_empty() || connection.is_empty() || agent.len() > 128 || connection.len() > 128 {
            return Err("Model claim origin requires bounded agent and connection identities".into());
        }
    }
    Ok(())
}

fn validate_draft(draft: &Draft) -> Result<(), String> {
    if draft.question.trim().is_empty() || draft.hypothesis.trim().is_empty() ||
        draft.question.len() > MAX_TEXT || draft.hypothesis.len() > MAX_TEXT {
        return Err("A claim needs a question and hypothesis within the text allowance".into());
    }
    if draft.citations.len() > MAX_CITATIONS { return Err("A claim may cite at most sixteen observations".into()); }
    let mut seen = BTreeSet::new();
    for citation in &draft.citations {
        citation.validate()?;
        if !seen.insert(citation) { return Err("Duplicate claim citation".into()); }
    }
    if let Some(criterion) = &draft.criterion {
        for source in criterion.sources() {
            source.validate()?;
            if !seen.contains(source) { return Err("Every criterion source must be explicitly cited".into()); }
            if source.node.is_none() { return Err("This criterion requires an exact observed node".into()); }
        }
    }
    Ok(())
}

/// A projection of an actual saved occurrence, constructed by the trusted
/// graph adapter. Absence is unknown, not zero or a guessed file kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub kind: Option<Kind>,
    pub size: Option<u64>,
    pub changed_during_observation: bool,
    pub sample: Option<Sample>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub offset: u64,
    pub length: u64,
    pub complete_file: bool,
    pub sha256: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict { Supported, Contradicted, Unresolved }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    pub explanation: &'static str,
    pub citations: Vec<Citation>,
}

/// Evaluates retained observations only. A Supported result is limited to the
/// explicit criterion and historical evidence; it does not prove a prose claim.
pub fn evaluate(criterion: &Criterion, mut lookup: impl FnMut(&Citation) -> Option<Fact>) -> Outcome {
    let citations = criterion.sources().into_iter().cloned().collect();
    let result = match criterion {
        Criterion::Kind { source, expected } => match lookup(source) {
            Some(f) if !f.changed_during_observation => match f.kind {
                Some(kind) => (if kind == *expected { Verdict::Supported } else { Verdict::Contradicted }, "Compared the retained observed kind"),
                None => (Verdict::Unresolved, "The retained kind is unknown"),
            },
            _ => (Verdict::Unresolved, "Observation is unavailable or changed during acquisition"),
        },
        Criterion::Size { source, expected } => match lookup(source) {
            Some(f) if !f.changed_during_observation => match f.size {
                Some(size) => (if size == *expected { Verdict::Supported } else { Verdict::Contradicted }, "Compared the retained metadata byte length"),
                None => (Verdict::Unresolved, "The retained byte length is unknown"),
            },
            _ => (Verdict::Unresolved, "Observation is unavailable or changed during acquisition"),
        },
        Criterion::CompleteCaptureDigestEqual { left, right } => match (lookup(left), lookup(right)) {
            (Some(a), Some(b)) => match (complete_sample(&a), complete_sample(&b)) {
                (Some(x), Some(y)) => (if x.length == y.length && x.sha256 == y.sha256 { Verdict::Supported } else { Verdict::Contradicted }, "Compared SHA-256 and length of two complete captured files; digest equality is not a byte-by-byte proof"),
                _ => (Verdict::Unresolved, "Whole-file comparison requires two complete unchanged file captures; prefixes are insufficient"),
            },
            _ => (Verdict::Unresolved, "A cited observation is unavailable under current access"),
        },
    };
    Outcome { verdict: result.0, explanation: result.1, citations }
}

fn complete_sample(fact: &Fact) -> Option<&Sample> {
    let sample = fact.sample.as_ref()?;
    if fact.changed_during_observation || fact.kind != Some(Kind::File) ||
        !sample.complete_file || sample.offset != 0 || fact.size != Some(sample.length) { return None; }
    Some(sample)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn citation(id: &str) -> Citation { Citation { record: id.into(), snapshot: Some(0), node: Some(0) } }
    fn draft() -> Draft { Draft { previous: None, question: "What format is the selected item?".into(), hypothesis: "It is a regular file.".into(), citations: vec![citation("one")], criterion: Some(Criterion::Kind { source: citation("one"), expected: Kind::File }) } }
    fn fact() -> Fact { Fact { kind: Some(Kind::File), size: Some(4), changed_during_observation: false, sample: Some(Sample { offset: 0, length: 4, complete_file: true, sha256: [7; 32] }) } }
    #[test]
    fn revisions_keep_origin_and_prior_text() {
        let mut claims = Claims::default();
        claims.append(draft(), Origin::Model { agent: "Analyst".into(), connection: "local".into() }, |_| true).unwrap();
        let original = claims.revisions()[0].clone();
        let mut next = draft(); next.previous = Some(1); next.hypothesis = "It may be a directory.".into();
        claims.append(next, Origin::User, |_| true).unwrap();
        assert_eq!(claims.revisions()[0], original);
        assert_eq!(claims.revisions()[1].previous, Some(1));
        assert_eq!(Claims::restore(claims.revisions().to_vec()).unwrap(), claims);
    }
    #[test]
    fn refusal_is_transactional_and_no_authority_is_inferred() {
        let mut claims = Claims::default();
        assert!(claims.append(draft(), Origin::User, |_| false).is_err());
        assert!(claims.revisions().is_empty());
        let mut d = draft(); d.citations.clear();
        assert!(claims.append(d, Origin::User, |_| true).is_err());
        let mut d = draft(); d.previous = Some(9);
        assert!(claims.append(d, Origin::User, |_| true).is_err());
        assert!(claims.revisions().is_empty());
    }
    #[test]
    fn unknown_and_changed_are_unresolved_not_contradictions() {
        let c = Criterion::Size { source: citation("one"), expected: u64::MAX };
        let mut f = fact(); f.size = Some(u64::MAX);
        assert_eq!(evaluate(&c, |_| Some(f.clone())).verdict, Verdict::Supported);
        f.size = None;
        assert_eq!(evaluate(&c, |_| Some(f.clone())).verdict, Verdict::Unresolved);
        f.size = Some(4); f.changed_during_observation = true;
        assert_eq!(evaluate(&c, |_| Some(f.clone())).verdict, Verdict::Unresolved);
        assert_eq!(evaluate(&c, |_| None).verdict, Verdict::Unresolved);
    }
    #[test]
    fn prefixes_never_establish_whole_file_equality() {
        let c = Criterion::CompleteCaptureDigestEqual { left: citation("one"), right: citation("two") };
        assert_eq!(evaluate(&c, |_| Some(fact())).verdict, Verdict::Supported);
        let mut f = fact(); f.sample.as_mut().unwrap().complete_file = false;
        assert_eq!(evaluate(&c, |_| Some(f.clone())).verdict, Verdict::Unresolved);
        f = fact(); f.size = Some(5);
        assert_eq!(evaluate(&c, |_| Some(f.clone())).verdict, Verdict::Unresolved);
        assert_eq!(evaluate(&c, |r| { let mut f = fact(); if r.record == "two" { f.sample.as_mut().unwrap().sha256 = [8;32]; } Some(f) }).verdict, Verdict::Contradicted);
    }
    #[test]
    fn malformed_saved_revision_order_and_scope_refuse() {
        let mut claims = Claims::default(); claims.append(draft(), Origin::User, |_| true).unwrap();
        let r = claims.revisions()[0].clone();
        assert!(Claims::restore(vec![r.clone(), r.clone()]).is_err());
        let mut r = r; r.previous = Some(r.id);
        assert!(Claims::restore(vec![r]).is_err());
        assert!(Citation { record: "one".into(), snapshot: None, node: Some(0) }.validate().is_err());
    }
}
