use serde::{Deserialize, Serialize};

pub type Id = String;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }, default = $default:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $s),+
                }
            }

            pub fn parse(s: &str) -> Self {
                match s {
                    $($s => Self::$variant,)+
                    _ => Self::$default,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum!(Difficulty { Easy => "easy", Medium => "medium", Hard => "hard" }, default = Medium);
string_enum!(Priority { Low => "low", Medium => "medium", High => "high", Urgent => "urgent" }, default = Medium);
string_enum!(AssignmentKind { Assignment => "assignment", Exam => "exam", Quiz => "quiz", Project => "project" }, default = Assignment);
string_enum!(AssignmentStatus {
    NotStarted => "not_started",
    InProgress => "in_progress",
    Waiting => "waiting",
    Completed => "completed",
    Submitted => "submitted",
    Overdue => "overdue",
}, default = NotStarted);
string_enum!(ReviewStatus { Pending => "pending", Approved => "approved", Rejected => "rejected", Edited => "edited" }, default = Pending);
string_enum!(SyllabusStatus { Processing => "processing", ReadyForReview => "ready_for_review", Reviewed => "reviewed", Failed => "failed" }, default = Processing);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub name: String,
    pub university: Option<String>,
    pub major: Option<String>,
    pub weekly_availability: serde_json::Value,
    pub preferred_study_times: serde_json::Value,
    pub sleep_schedule: serde_json::Value,
    pub goals: Option<String>,
    pub onboarding_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Semester {
    pub id: Id,
    pub name: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSemester {
    pub name: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Course {
    pub id: Id,
    pub semester_id: Id,
    pub name: String,
    pub code: Option<String>,
    pub professor_name: Option<String>,
    pub professor_email: Option<String>,
    pub credit_hours: Option<f64>,
    pub color: String,
    pub current_grade: Option<String>,
    pub office_hours: Option<String>,
    pub late_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCourse {
    pub semester_id: Id,
    pub name: String,
    pub code: Option<String>,
    pub professor_name: Option<String>,
    pub professor_email: Option<String>,
    pub credit_hours: Option<f64>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Syllabus {
    pub id: Id,
    pub course_id: Id,
    pub raw_text: String,
    pub status: SyllabusStatus,
    pub error_message: Option<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyllabusExtraction {
    pub id: Id,
    pub syllabus_id: Id,
    pub kind: AssignmentKind,
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub source_excerpt: String,
    pub confidence: f64,
    pub review_status: ReviewStatus,
    pub resulting_assignment_id: Option<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionEdits {
    pub title: Option<String>,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub kind: Option<AssignmentKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Id,
    pub course_id: Id,
    pub title: String,
    pub description: Option<String>,
    pub kind: AssignmentKind,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub difficulty: Difficulty,
    pub estimated_duration_minutes: Option<i64>,
    pub priority: Priority,
    pub status: AssignmentStatus,
    pub completion_percentage: i64,
    pub source_extraction_id: Option<Id>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAssignment {
    pub course_id: Id,
    pub title: String,
    pub description: Option<String>,
    pub kind: AssignmentKind,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub difficulty: Difficulty,
    pub estimated_duration_minutes: Option<i64>,
    pub priority: Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub difficulty: Option<Difficulty>,
    pub estimated_duration_minutes: Option<i64>,
    pub priority: Option<Priority>,
    pub status: Option<AssignmentStatus>,
    pub completion_percentage: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: Id,
    pub assignment_id: Id,
    pub title: String,
    pub status: AssignmentStatus,
    pub estimated_minutes: Option<i64>,
    pub order_index: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSubtask {
    pub assignment_id: Id,
    pub title: String,
    pub estimated_minutes: Option<i64>,
    pub order_index: i64,
}
