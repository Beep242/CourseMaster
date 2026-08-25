CREATE TABLE study_guides (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_study_guides_course ON study_guides(course_id);

CREATE TABLE practice_tests (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    difficulty TEXT NOT NULL DEFAULT 'medium',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_practice_tests_course ON practice_tests(course_id);

-- `options_json` is only set for multiple_choice; `correct_answer` holds the
-- literal correct option text for multiple_choice/true_false, or a model
-- answer to grade short_answer responses against. `topic` is a short
-- AI-assigned label (e.g. "Kirchhoff's laws") used to summarize weak areas
-- across an attempt without needing a separate topic-tracking system yet.
CREATE TABLE practice_questions (
    id TEXT PRIMARY KEY,
    practice_test_id TEXT NOT NULL REFERENCES practice_tests(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL,
    topic TEXT,
    question_text TEXT NOT NULL,
    options_json TEXT,
    correct_answer TEXT NOT NULL,
    explanation TEXT
);
CREATE INDEX idx_practice_questions_test ON practice_questions(practice_test_id);

-- `answers_json` is the full graded transcript (per-question submitted
-- response, correctness, feedback) — kept as one JSON blob rather than a
-- normalized answers table since it's always read/written as a whole unit
-- (one attempt's results page), never queried per-answer.
CREATE TABLE practice_attempts (
    id TEXT PRIMARY KEY,
    practice_test_id TEXT NOT NULL REFERENCES practice_tests(id) ON DELETE CASCADE,
    score_percentage REAL NOT NULL,
    answers_json TEXT NOT NULL,
    completed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_practice_attempts_test ON practice_attempts(practice_test_id);
