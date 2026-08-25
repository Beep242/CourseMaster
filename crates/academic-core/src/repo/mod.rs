pub mod assignments;
pub mod calendar_feeds;
pub mod courses;
pub mod profile;
pub mod semesters;
pub mod subtasks;
pub mod syllabus;

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
