export type Difficulty = "easy" | "medium" | "hard";
export type Priority = "low" | "medium" | "high" | "urgent";
export type AssignmentKind = "assignment" | "exam" | "quiz" | "project";
export type AssignmentStatus = "not_started" | "in_progress" | "waiting" | "completed" | "submitted" | "overdue";
export type ReviewStatus = "pending" | "approved" | "rejected" | "edited";
export type SyllabusStatus = "processing" | "ready_for_review" | "reviewed" | "failed";
export type SyllabusSource = "paste" | "calendar_feed";
export type StudyGuideKind = "quick_review" | "complete" | "cram_sheet" | "formula_sheet";
export type PracticeDifficulty = "easy" | "medium" | "hard" | "exam_simulation";
export type QuestionKind = "multiple_choice" | "true_false" | "short_answer";

export interface UserProfile {
  name: string;
  university: string | null;
  major: string | null;
  weekly_availability: unknown;
  preferred_study_times: unknown;
  sleep_schedule: unknown;
  goals: string | null;
  onboarding_complete: boolean;
}

export interface Semester {
  id: string;
  name: string;
  start_date: string | null;
  end_date: string | null;
  is_active: boolean;
}

export interface Course {
  id: string;
  semester_id: string;
  name: string;
  code: string | null;
  professor_name: string | null;
  professor_email: string | null;
  credit_hours: number | null;
  color: string;
  current_grade: string | null;
  office_hours: string | null;
  late_policy: string | null;
  external_org_unit_id: string | null;
}

export interface DetectedCourseGroup {
  org_unit_id: string | null;
  location: string;
  suggested_code: string | null;
  suggested_name: string;
  event_count: number;
}

export interface Syllabus {
  id: string;
  course_id: string | null;
  calendar_feed_id: string | null;
  source: SyllabusSource;
  raw_text: string;
  status: SyllabusStatus;
  error_message: string | null;
  imported_at: string;
}

export interface SyllabusExtraction {
  id: string;
  syllabus_id: string;
  course_id: string | null;
  kind: AssignmentKind;
  title: string;
  description: string | null;
  due_date: string | null;
  due_time: string | null;
  source_excerpt: string;
  confidence: number;
  review_status: ReviewStatus;
  resulting_assignment_id: string | null;
  external_uid: string | null;
}

export interface CalendarFeed {
  id: string;
  name: string;
  ics_url: string;
  last_synced_at: string | null;
  last_sync_error: string | null;
}

export interface Assignment {
  id: string;
  course_id: string;
  title: string;
  description: string | null;
  kind: AssignmentKind;
  due_date: string | null;
  due_time: string | null;
  difficulty: Difficulty;
  estimated_duration_minutes: number | null;
  priority: Priority;
  status: AssignmentStatus;
  completion_percentage: number;
  source_extraction_id: string | null;
  notes: string | null;
}

export interface Subtask {
  id: string;
  assignment_id: string;
  title: string;
  status: AssignmentStatus;
  estimated_minutes: number | null;
  order_index: number;
}

export interface PrioritizedItem {
  id: string;
  title: string;
  course_id: string;
  score: number;
  days_until_due: number | null;
  is_overdue: boolean;
  reason: string;
}

export interface StudyGuide {
  id: string;
  course_id: string;
  kind: StudyGuideKind;
  title: string;
  content: string;
  created_at: string;
}

export interface PracticeQuestion {
  id: string;
  practice_test_id: string;
  order_index: number;
  kind: QuestionKind;
  topic: string | null;
  question_text: string;
  options: string[] | null;
  correct_answer: string;
  explanation: string | null;
}

export interface PracticeTestSummary {
  id: string;
  course_id: string;
  title: string;
  difficulty: PracticeDifficulty;
  created_at: string;
}

export interface PracticeTest extends PracticeTestSummary {
  questions: PracticeQuestion[];
}

export interface GradedAnswer {
  question_id: string;
  question_text: string;
  kind: QuestionKind;
  topic: string | null;
  submitted: string;
  correct_answer: string;
  is_correct: boolean;
  feedback: string | null;
}

export interface PracticeAttempt {
  id: string;
  practice_test_id: string;
  score_percentage: number;
  answers: GradedAnswer[];
  completed_at: string;
}

export type CardKind = "basic" | "multiple_choice" | "true_false" | "typed";

export interface Deck {
  id: string;
  /** null means the deck is not filed under a course yet. */
  course_id: string | null;
  name: string;
  description: string | null;
  color: string;
  /** Computed server-side from the cards table, not a cached column. */
  card_count: number;
  created_at: string;
  updated_at: string;
}

export interface NewDeck {
  course_id?: string | null;
  name: string;
  description?: string | null;
  color?: string | null;
}

/**
 * Omit a field to leave it alone; send an explicit `null` to clear it. That
 * distinction is why the Rust side uses `Patch<T>` rather than the plain
 * `Option` convention the older update types use — see `models::Patch`.
 */
export interface DeckUpdate {
  name?: string;
  color?: string;
  course_id?: string | null;
  description?: string | null;
}

export interface Card {
  id: string;
  deck_id: string;
  order_index: number;
  kind: CardKind;
  front: string;
  back: string;
  /** Only set for multiple_choice; generated once at save time, not per review. */
  options: string[] | null;
  explanation: string | null;
  tags: string[];
  /** The passage a generated card came from; null for a hand-written card. */
  source_excerpt: string | null;
  created_at: string;
  updated_at: string;
}

export interface NewCard {
  front: string;
  back: string;
  kind?: CardKind;
  options?: string[] | null;
  explanation?: string | null;
  tags?: string[] | null;
  source_excerpt?: string | null;
  order_index?: number;
}

/** Same omit-vs-null semantics as DeckUpdate. */
export interface CardUpdate {
  front?: string;
  back?: string;
  kind?: CardKind;
  order_index?: number;
  deck_id?: string;
  options?: string[] | null;
  explanation?: string | null;
  tags?: string[] | null;
}

/** A card plus where it lives, so a search hit is identifiable. */
export interface CardSearchHit extends Card {
  deck_name: string;
  course_name: string | null;
}
