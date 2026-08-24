CREATE TABLE user_profile (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL,
    university TEXT,
    major TEXT,
    weekly_availability_json TEXT NOT NULL DEFAULT '{}',
    preferred_study_times_json TEXT NOT NULL DEFAULT '[]',
    sleep_schedule_json TEXT NOT NULL DEFAULT '{}',
    goals TEXT,
    onboarding_complete INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE semesters (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    start_date TEXT,
    end_date TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE courses (
    id TEXT PRIMARY KEY,
    semester_id TEXT NOT NULL REFERENCES semesters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    code TEXT,
    professor_name TEXT,
    professor_email TEXT,
    credit_hours REAL,
    color TEXT NOT NULL DEFAULT '#8b5cf6',
    current_grade TEXT,
    office_hours TEXT,
    late_policy TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_courses_semester ON courses(semester_id);

CREATE TABLE syllabi (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    raw_text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'processing',
    error_message TEXT,
    imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_syllabi_course ON syllabi(course_id);

-- One row per AI-extracted candidate fact (an assignment, exam, quiz, or
-- project deadline) pending human review. Nothing here is ever treated as
-- real schedule data until promoted to an `assignments` row via approval —
-- this table is the audit trail the spec's "review before scheduling"
-- requirement depends on: source_excerpt and confidence travel with every
-- extracted field so the review screen can show provenance, not just a
-- bare claim.
CREATE TABLE syllabus_extractions (
    id TEXT PRIMARY KEY,
    syllabus_id TEXT NOT NULL REFERENCES syllabi(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    due_date TEXT,
    due_time TEXT,
    source_excerpt TEXT NOT NULL,
    confidence REAL NOT NULL,
    review_status TEXT NOT NULL DEFAULT 'pending',
    resulting_assignment_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_extractions_syllabus ON syllabus_extractions(syllabus_id);
CREATE INDEX idx_extractions_status ON syllabus_extractions(review_status);

CREATE TABLE assignments (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    kind TEXT NOT NULL DEFAULT 'assignment',
    due_date TEXT,
    due_time TEXT,
    difficulty TEXT NOT NULL DEFAULT 'medium',
    estimated_duration_minutes INTEGER,
    priority TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'not_started',
    completion_percentage INTEGER NOT NULL DEFAULT 0,
    source_extraction_id TEXT REFERENCES syllabus_extractions(id) ON DELETE SET NULL,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_assignments_course ON assignments(course_id);
CREATE INDEX idx_assignments_due ON assignments(due_date);
CREATE INDEX idx_assignments_status ON assignments(status);

CREATE TABLE subtasks (
    id TEXT PRIMARY KEY,
    assignment_id TEXT NOT NULL REFERENCES assignments(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'not_started',
    estimated_minutes INTEGER,
    order_index INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_subtasks_assignment ON subtasks(assignment_id);
