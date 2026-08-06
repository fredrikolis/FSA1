// Concern: captures the corpus's graded facts | Non-concern: argv and exit codes, grading one fixture | IO: (the corpus) -> Facts

pub mod corpus;
pub mod grade;
pub mod literal;
pub mod resolver;
pub mod snapshot;

pub use snapshot::{Coverage, Facts, Verdict, VerdictKind};

pub fn capture() -> Result<Facts, String> {
    let fixtures = corpus::load_all()?;
    let verdicts = grade::grade_all(&fixtures);
    Ok(Facts::capture(&fixtures, verdicts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_corpus_loads_and_grades_without_panic() {
        let facts = capture().expect("the corpus must load and grade cleanly");
        assert!(
            !facts.verdicts.is_empty(),
            "the seed corpus must not be empty"
        );
    }
}
