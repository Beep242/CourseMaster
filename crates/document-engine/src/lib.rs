pub mod error;
pub mod syllabus_extraction;

pub use error::DocumentError;
pub use syllabus_extraction::{ask_syllabus, process_syllabus};
