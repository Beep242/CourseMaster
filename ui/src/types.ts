export type Difficulty = "easy" | "medium" | "hard";
export type Priority = "low" | "medium" | "high" | "urgent";
export type AssignmentKind = "assignment" | "exam" | "quiz" | "project";
export type AssignmentStatus = "not_started" | "in_progress" | "waiting" | "completed" | "submitted" | "overdue";
export type ReviewStatus = "pending" | "approved" | "rejected" | "edited";
export type SyllabusStatus = "processing" | "ready_for_review" | "reviewed" | "failed";
export type SyllabusSource = "paste" | "calendar_feed";

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
