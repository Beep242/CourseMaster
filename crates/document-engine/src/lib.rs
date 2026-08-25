pub mod calendar_feed;
pub mod error;
pub mod practice_test;
pub mod study_guide;
pub mod syllabus_extraction;

pub use calendar_feed::{detect_course_groups, link_course_from_group, sync_feed, DetectedCourseGroup};
pub use error::DocumentError;
pub use practice_test::{generate_practice_test, grade_attempt};
pub use study_guide::generate_study_guide;
pub use syllabus_extraction::{ask_syllabus, process_syllabus};
