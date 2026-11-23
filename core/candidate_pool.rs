//! In-memory candidate pool and lesson library.
//! The pool keeps at most 16 candidates in store order.
//! A composition reads at most 4 of those candidates and returns one combined text.
//! The library keeps at most 32 lessons. A later read returns the stored lesson for its key.
//! A candidate past the pool cap is named and left out. A lesson past the library cap is named and left out.
//! This module keeps its records in memory and does not start a child process.

#![forbid(unsafe_code)]

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_read_one_lesson() {
        let mut library = LessonLibrary::new();
        library.store("probe-contrast", "compose from observed contrasts");
        assert_eq!(
            library.read("probe-contrast"),
            "compose from observed contrasts"
        );
        assert_eq!(
            library.read("probe-contrast"),
            "compose from observed contrasts"
        );
        assert_eq!(
            library.report(),
            "lesson probe-contrast compose from observed contrasts\nno child process was started"
        );
    }

    #[test]
    fn seventeenth_candidate_is_named_as_not_stored() {
        assert_eq!(CANDIDATE_CAP, 16);
        let mut pool = CandidatePool::new();
        for index in 1..=16 {
            pool.store(&format!("c{index}"), &format!("text {index}"));
        }
        pool.store("c17", "kept out");
        let report = pool.report();
        assert!(report.contains("candidate 16 c16 text 16\n"), "{report}");
        assert!(report.contains("candidate c17 was not stored\n"), "{report}");
        assert!(report.contains("later candidates were not stored\n"), "{report}");
        assert!(report.ends_with("no child process was started"), "{report}");
        assert!(!report.contains("kept out"), "{report}");
        assert!(!report.contains("candidate 17 "), "{report}");
        let stored = report
            .lines()
            .filter(|line| line.starts_with("candidate ") && !line.contains("was not stored"))
            .count();
        assert_eq!(stored, 16);
    }

    #[test]
    fn composition_of_four_stays_within_the_cap() {
        assert_eq!(COMPOSITION_CAP, 4);
        let mut pool = CandidatePool::new();
        for name in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
            pool.store(name, name);
        }
        assert_eq!(
            pool.compose(),
            "alpha\nbeta\ngamma\ndelta\nno child process was started"
        );
        let report = pool.report();
        assert!(report.contains("candidate 5 epsilon epsilon"), "{report}");
        assert!(report.contains("candidate 6 zeta zeta"), "{report}");
        assert!(!report.contains("later candidates were not stored"), "{report}");
    }

    #[test]
    fn thirty_third_lesson_is_named_as_not_stored() {
        assert_eq!(LESSON_CAP, 32);
        let mut library = LessonLibrary::new();
        for index in 1..=32 {
            library.store(&format!("k{index}"), &format!("kept {index}"));
        }
        library.store("extra", "dropped lesson");
        assert_eq!(library.read("k1"), "kept 1");
        assert_eq!(library.read("k32"), "kept 32");
        assert_eq!(library.read("extra"), "lesson extra was not stored");
        let report = library.report();
        assert!(report.contains("lesson extra was not stored\n"), "{report}");
        assert!(report.contains("later lessons were not stored\n"), "{report}");
        assert!(report.ends_with("no child process was started"), "{report}");
        assert!(!report.contains("dropped lesson"), "{report}");
    }

    #[test]
    fn second_level_reads_the_stored_composition() {
        let report = retain_texts(&["alpha".to_string(), "beta".to_string()]);
        assert!(report.contains("alpha\nbeta\nno child process was started\n"), "{report}");
        assert!(
            report.contains("second level alpha\nbeta\nno child process was started\n"),
            "{report}"
        );
        assert!(report.contains("lesson level2 alpha\nbeta\nno child process was started\n"), "{report}");
        assert!(report.ends_with("no child process was started"), "{report}");
    }

    #[test]
    fn later_read_returns_the_first_stored_lesson() {
        let mut library = LessonLibrary::new();
        library.store("kept-first", "compose from observed contrasts");
        library.store("kept-first", "a later text is not stored");
        assert_eq!(
            library.read("kept-first"),
            "compose from observed contrasts"
        );
        let report = library.report();
        assert!(!report.contains("a later text is not stored"), "{report}");
        assert!(report.ends_with("no child process was started"), "{report}");
    }

    #[test]
    fn later_candidate_is_not_composed() {
        let mut pool = CandidatePool::new();
        for index in 1..=16 {
            pool.store(&format!("c{index}"), &format!("text {index}"));
        }
        pool.store("c17", "kept out");
        assert_eq!(
            pool.compose(),
            "text 1\ntext 2\ntext 3\ntext 4\nno child process was started"
        );
        let report = pool.report();
        assert!(report.contains("candidate c17 was not stored\n"), "{report}");
        assert!(report.ends_with("no child process was started"), "{report}");
        assert!(!report.contains("kept out"), "{report}");
    }
}
