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
    pub course_id: Option<Id>,
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

/// Distinguishes "the client did not mention this field" from "the client
/// explicitly sent null to clear it".
///
/// The house patch convention everywhere else is read-modify-write with `None`
/// meaning *keep* (see `AssignmentUpdate` and `ExtractionEdits`), which has no
/// way to set a nullable column back to NULL. That is fine for an assignment,
/// and not fine for a card: a student editing an AI-generated card will
/// absolutely want to delete a wrong explanation. With this, an absent field is
/// `None` (keep), an explicit `null` is `Some(None)` (clear), and a value is
/// `Some(Some(v))` (set).
pub type Patch<T> = Option<Option<T>>;

/// Needed because `#[serde(default)]` alone cannot tell an absent field from a
/// present `null` — both would deserialize to `None`.
pub fn deserialize_patch<'de, T, D>(deserializer: D) -> Result<Patch<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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

string_enum!(CardKind {
    Basic => "basic",
    MultipleChoice => "multiple_choice",
    TrueFalse => "true_false",
    Typed => "typed",
}, default = Basic);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    pub id: Id,
    /// None means the deck is not filed under a course yet — an import can
    /// land before that decision is made. See migration 0005.
    pub course_id: Option<Id>,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub card_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDeck {
    #[serde(default)]
    pub course_id: Option<Id>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeckUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// Clearable: moving a deck out of a course is a real action.
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub course_id: Patch<Id>,
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub description: Patch<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: Id,
    pub deck_id: Id,
    pub order_index: i64,
    pub kind: CardKind,
    pub front: String,
    pub back: String,
    /// Multiple-choice distractors, stored at save time so a review needs no
    /// AI call. Only set for `multiple_choice`.
    pub options: Option<Vec<String>>,
    pub explanation: Option<String>,
    pub tags: Vec<String>,
    /// The verbatim passage a generated card came from; None for a card typed
    /// by hand.
    pub source_excerpt: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCard {
    pub front: String,
    pub back: String,
    #[serde(default)]
    pub kind: Option<CardKind>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub source_excerpt: Option<String>,
    #[serde(default)]
    pub order_index: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CardUpdate {
    #[serde(default)]
    pub front: Option<String>,
    #[serde(default)]
    pub back: Option<String>,
    #[serde(default)]
    pub kind: Option<CardKind>,
    #[serde(default)]
    pub order_index: Option<i64>,
    /// Moving a card to another deck.
    #[serde(default)]
    pub deck_id: Option<Id>,
    /// All clearable — a student editing a generated card needs to be able to
    /// delete a bad explanation or a wrong set of distractors, not just
    /// overwrite them.
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub options: Patch<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub explanation: Patch<String>,
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub tags: Patch<Vec<String>>,
}

/// A card plus the deck and course it belongs to, for cross-library search
/// where a bare card gives the student no idea where it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSearchHit {
    #[serde(flatten)]
    pub card: Card,
    pub deck_name: String,
    pub course_name: Option<String>,
}

string_enum!(ImportSourceKind {
    Paste => "paste",
    QuizletPaste => "quizlet_paste",
    AnkiCsv => "anki_csv",
    Pdf => "pdf",
    Docx => "docx",
    Pptx => "pptx",
}, default = Paste);

string_enum!(ImportStatus {
    Generating => "generating",
    ReadyForReview => "ready_for_review",
    Failed => "failed",
    Completed => "completed",
}, default = Generating);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardImport {
    pub id: Id,
    pub deck_id: Id,
    pub source_kind: ImportSourceKind,
    pub source_label: Option<String>,
    pub source_text: String,
    pub status: ImportStatus,
    pub error_message: Option<String>,
    pub total_cost_usd: Option<f64>,
    /// Counts below are computed per read, like `Deck::card_count`.
    pub pending_count: i64,
    pub approved_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// One generated card awaiting review. Mirrors `SyllabusExtraction`: nothing
/// here is real until `approve_candidate` promotes it into a `cards` row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardCandidate {
    pub id: Id,
    pub import_id: Id,
    pub order_index: i64,
    pub kind: CardKind,
    pub front: String,
    pub back: String,
    pub options: Option<Vec<String>>,
    pub explanation: Option<String>,
    pub tags: Vec<String>,
    pub source_excerpt: Option<String>,
    pub review_status: ReviewStatus,
    pub resulting_card_id: Option<Id>,
    pub created_at: String,
}

/// Edits a reviewer applied before approving. Plain `Option` (absent = keep)
/// rather than `Patch`, because a candidate is a throwaway staging row: the
/// place to clear a field is the real card, after approval.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateEdits {
    #[serde(default)]
    pub front: Option<String>,
    #[serde(default)]
    pub back: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// A card's spaced-repetition state as stored. Mirrors `srs::CardState` field
/// for field — the migration's columns were transcribed from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSchedule {
    pub card_id: Id,
    pub repetitions: i64,
    pub interval_days: i64,
    pub ease_factor: f64,
    pub due_date: String,
    pub lapses: i64,
    pub suspended: bool,
    pub updated_at: String,
}

/// A card plus its schedule and where it lives — what a review queue needs to
/// show one card without the client fetching three things.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DueCard {
    #[serde(flatten)]
    pub card: Card,
    pub deck_name: String,
    pub course_name: Option<String>,
    /// None when the card has never been reviewed.
    pub schedule: Option<CardSchedule>,
    pub is_new: bool,
    /// What each grade button would do, in days, computed server-side from the
    /// same `srs` code a review applies. Sent with the card so the UI can label
    /// the buttons without reimplementing SM-2 in TypeScript — a second
    /// implementation would be free to drift, and nothing would notice.
    pub projections: IntervalProjections,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IntervalProjections {
    pub again: i64,
    pub hard: i64,
    pub good: i64,
    pub easy: i64,
}

/// One graded answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardReview {
    pub id: Id,
    pub card_id: Id,
    pub rating: String,
    pub local_date: String,
    pub reviewed_at: String,
    pub duration_ms: Option<i64>,
    pub interval_before: i64,
    pub interval_after: i64,
    pub ease_after: f64,
    pub was_lapse: bool,
}

/// What `record_review` gives back: the new schedule plus whether this answer
/// tipped the card into leech territory, so the UI can offer to suspend it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewOutcome {
    pub schedule: CardSchedule,
    pub review_id: Id,
    pub is_leech: bool,
}

/// A cached explanation of one specific wrong answer. See migration 0008: the
/// key is the *normalised* mistake, so the same misunderstanding is explained
/// (and paid for) once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardExplanation {
    pub id: Id,
    pub card_id: Id,
    pub mistake_key: String,
    pub submitted: String,
    pub explanation: String,
    pub total_cost_usd: Option<f64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeakCard {
    pub card_id: Id,
    pub front: String,
    pub deck_name: String,
    pub course_name: Option<String>,
    pub total_reviews: i64,
    pub lapses: i64,
    pub accuracy: f64,
    pub interval_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayAccuracy {
    /// The student's local calendar date, not the server's.
    pub day: String,
    pub total: i64,
    pub correct: i64,
    pub accuracy: f64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudyTotals {
    pub reviews: i64,
    pub correct: i64,
    pub accuracy: f64,
    pub days_studied: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckMastery {
    pub deck_id: Id,
    pub deck_name: String,
    pub course_name: Option<String>,
    pub total_cards: i64,
    pub seen_cards: i64,
    pub mature_cards: i64,
    /// Mature cards over *total* cards — coverage, not accuracy.
    pub mastery: f64,
}
