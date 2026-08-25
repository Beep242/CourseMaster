pub mod calendar_feed;
pub mod error;
pub mod syllabus_extraction;

pub use calendar_feed::{detect_course_groups, link_course_from_group, sync_feed, DetectedCourseGroup};
pub use error::DocumentError;
pub use syllabus_extraction::{ask_syllabus, process_syllabus};
