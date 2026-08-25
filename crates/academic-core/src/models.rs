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
string_enum!(SyllabusSource { Paste => "paste", CalendarFeed => "calendar_feed" }, default = Paste);
string_enum!(StudyGuideKind {
    QuickReview => "quick_review",
    Complete => "complete",
    CramSheet => "cram_sheet",
    FormulaSheet => "formula_sheet",
}, default = QuickReview);
string_enum!(PracticeDifficulty { Easy => "easy", Medium => "medium", Hard => "hard", ExamSimulation => "exam_simulation" }, default = Medium);
string_enum!(QuestionKind { MultipleChoice => "multiple_choice", TrueFalse => "true_false", ShortAnswer => "short_answer" }, default = MultipleChoice);

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
    /// The LMS's own stable id for this course (e.g. a D2L org unit id),
    /// present when this course was created by linking a calendar-feed's
    /// auto-detected course group rather than typed in by hand.
    pub external_org_unit_id: Option<String>,
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
    #[serde(default)]
    pub external_org_unit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Syllabus {
    pub id: Id,
    /// None for a calendar-feed-sourced batch, which spans every course in
    /// one import — see `SyllabusExtraction::course_id` for where the
    /// per-item course guess/override actually lives in that case.
    pub course_id: Option<Id>,
    pub calendar_feed_id: Option<Id>,
    pub source: SyllabusSource,
    pub raw_text: String,
    pub status: SyllabusStatus,
    pub error_message: Option<String>,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyllabusExtraction {
    pub id: Id,
    pub syllabus_id: Id,
    /// None means "inherit the parent syllabus's course_id" (the paste
    /// flow, where every extraction in the batch belongs to one course).
    /// Set is either a calendar-feed course-name guess or a reviewer
    /// override — resolved by `repo::syllabus::approve_extraction`.
    pub course_id: Option<Id>,
    pub kind: AssignmentKind,
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub source_excerpt: String,
    pub confidence: f64,
    pub review_status: ReviewStatus,
    pub resulting_assignment_id: Option<Id>,
    pub external_uid: Option<String>,
    pub external_org_unit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionEdits {
    pub title: Option<String>,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub kind: Option<AssignmentKind>,
    pub course_id: Option<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarFeed {
    pub id: Id,
    pub name: String,
    pub ics_url: String,
    pub last_synced_at: Option<String>,
    pub last_sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCalendarFeed {
    pub name: String,
    pub ics_url: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudyGuide {
    pub id: Id,
    pub course_id: Id,
    pub kind: StudyGuideKind,
    pub title: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticeTest {
    pub id: Id,
    pub course_id: Id,
    pub title: String,
    pub difficulty: PracticeDifficulty,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticeQuestion {
    pub id: Id,
    pub practice_test_id: Id,
    pub order_index: i64,
    pub kind: QuestionKind,
    pub topic: Option<String>,
    pub question_text: String,
    /// Only set for `multiple_choice`.
    pub options: Option<Vec<String>>,
    pub correct_answer: String,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticeAttempt {
    pub id: Id,
    pub practice_test_id: Id,
    pub score_percentage: f64,
    pub answers: serde_json::Value,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmittedAnswer {
    pub question_id: Id,
    pub response: String,
}
