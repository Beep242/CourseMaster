pub mod calendar_feed;
pub mod error;
pub mod syllabus_extraction;

pub use calendar_feed::sync_feed;
pub use error::DocumentError;
pub use syllabus_extraction::{ask_syllabus, process_syllabus};
