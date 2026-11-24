//! In-memory candidate pool and lesson library.
//! The pool keeps at most 16 candidates in store order.
//! A composition reads at most 4 of those candidates and returns one combined text.
//! The library keeps at most 32 lessons. A later read returns the stored lesson for its key.
//! A candidate past the pool cap is named and left out. A lesson past the library cap is named and left out.
//! This module keeps its records in memory and does not start a child process.

#![forbid(unsafe_code)]

/// Cost units = reported output tokens + model request wall milliseconds
/// + 100 per tool call with an observed result. The edit byte budget is separate.
pub(crate) fn execution_cost(output_tokens: u64, wall_ms: u64, tool_calls: u64) -> Option<u64> {
    output_tokens.checked_add(wall_ms)?.checked_add(tool_calls.checked_mul(100)?)
}

pub const CANDIDATE_CAP: usize = 16;
pub const COMPOSITION_CAP: usize = 4;
pub const LESSON_CAP: usize = 32;

pub const LATER_CANDIDATES: &str = "later candidates were not stored";
pub const LATER_LESSONS: &str = "later lessons were not stored";
pub const NO_CHILD_PROCESS: &str = "no child process was started";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidatePool {
    stored: Vec<Candidate>,
    omitted: Vec<String>,
}

impl Default for CandidatePool {
    fn default() -> Self {
        Self::new()
    }
}

impl CandidatePool {
    pub fn new() -> Self {
        Self {
            stored: Vec::new(),
            omitted: Vec::new(),
        }
    }

    /// Keep the candidate when the pool is under the cap.
    /// A later name is recorded as omitted and is not composed.
    pub fn store(&mut self, name: &str, text: &str) {
        if self.stored.iter().any(|candidate| candidate.name == name)
            || self.omitted.iter().any(|omitted| omitted == name)
        {
            return;
        }
        if self.stored.len() >= CANDIDATE_CAP {
            self.omitted.push(name.to_string());
            return;
        }
        self.stored.push(Candidate {
            name: name.to_string(),
            text: text.to_string(),
        });
    }

    /// Read the first stored candidates, at most four, and join their text.
    pub fn compose(&self) -> String {
        let mut lines = Vec::new();
        for candidate in self.stored.iter().take(COMPOSITION_CAP) {
            lines.push(candidate.text.clone());
        }
        lines.push(NO_CHILD_PROCESS.to_string());
        lines.join("\n")
    }

    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        for (index, candidate) in self.stored.iter().enumerate() {
            lines.push(format!(
                "candidate {} {} {}",
                index + 1,
                candidate.name,
                candidate.text
            ));
        }
        for name in &self.omitted {
            lines.push(format!("candidate {name} was not stored"));
        }
        if !self.omitted.is_empty() {
            lines.push(LATER_CANDIDATES.to_string());
        }
        lines.push(NO_CHILD_PROCESS.to_string());
        lines.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub key: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonLibrary {
    lessons: Vec<Lesson>,
    omitted: Vec<String>,
}

impl Default for LessonLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl LessonLibrary {
    pub fn new() -> Self {
        Self {
            lessons: Vec::new(),
            omitted: Vec::new(),
        }
    }

    /// Keep the first lesson stored for a key. A key past the cap is named and left out.
    pub fn store(&mut self, key: &str, text: &str) {
        if self.lessons.iter().any(|lesson| lesson.key == key) {
            return;
        }
        if self.omitted.iter().any(|omitted| omitted == key) {
            return;
        }
        if self.lessons.len() >= LESSON_CAP {
            self.omitted.push(key.to_string());
            return;
        }
        self.lessons.push(Lesson {
            key: key.to_string(),
            text: text.to_string(),
        });
    }

    /// Return the lesson stored for `key`. A missing key is named as not stored.
    pub fn read(&self, key: &str) -> String {
        if let Some(lesson) = self.lessons.iter().find(|lesson| lesson.key == key) {
            return lesson.text.clone();
        }
        format!("lesson {key} was not stored")
    }

    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        for lesson in &self.lessons {
            lines.push(format!("lesson {} {}", lesson.key, lesson.text));
        }
        for key in &self.omitted {
            lines.push(format!("lesson {key} was not stored"));
        }
        if !self.omitted.is_empty() {
            lines.push(LATER_LESSONS.to_string());
        }
        lines.push(NO_CHILD_PROCESS.to_string());
        lines.join("\n")
    }
}

/// Store each text as a candidate and a lesson. The first composition is stored
/// as lesson `level2` and read back. No child process is started.
pub fn retain_texts(parts: &[String]) -> String {
    let mut pool = CandidatePool::new();
    let mut lessons = LessonLibrary::new();
    for (index, text) in parts.iter().enumerate() {
        let name = format!("c{}", index + 1);
        pool.store(&name, text);
        lessons.store(&name, text);
    }
    let first = pool.compose();
    lessons.store("level2", &first);
    let second = lessons.read("level2");
    format!(
        "{first}\nsecond level {second}\n{}\n{}",
        pool.report(),
        lessons.report()
    )
}
