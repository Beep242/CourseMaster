pub mod assignments;
pub mod calendar_feeds;
pub mod card_imports;
pub mod cards;
pub mod courses;
pub mod decks;
pub mod explanations;
pub mod practice_tests;
pub mod profile;
pub mod reviews;
pub mod semesters;
pub mod study_guides;
pub mod subtasks;
pub mod syllabus;

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
