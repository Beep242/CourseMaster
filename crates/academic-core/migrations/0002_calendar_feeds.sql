-- D2L/Brightspace (and any future ICS-based LMS) integration: a student's
-- personal calendar subscription feed spans every enrolled course in one
-- URL, so unlike a pasted syllabus there's no single course to attach the
-- import to up front — course assignment happens per-extraction during
-- review instead. That requires `syllabi.course_id` to become optional and
-- `syllabus_extractions` to carry its own course guess/override.
CREATE TABLE calendar_feeds (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    ics_url TEXT NOT NULL,
    last_synced_at TEXT,
    last_sync_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE syllabi_new (
    id TEXT PRIMARY KEY,
    course_id TEXT REFERENCES courses(id) ON DELETE CASCADE,
    calendar_feed_id TEXT REFERENCES calendar_feeds(id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'paste',
    raw_text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'processing',
    error_message TEXT,
    imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO syllabi_new (id, course_id, source, raw_text, status, error_message, imported_at)
SELECT id, course_id, 'paste', raw_text, status, error_message, imported_at FROM syllabi;
DROP TABLE syllabi;
ALTER TABLE syllabi_new RENAME TO syllabi;
CREATE INDEX idx_syllabi_course ON syllabi(course_id);
CREATE INDEX idx_syllabi_feed ON syllabi(calendar_feed_id);

-- NULL means "inherit from the parent syllabus's course_id" (the paste
-- flow); set means an explicit guess (calendar-feed flow) or a reviewer
-- override, resolved in that order by repo::syllabus::approve_extraction.
ALTER TABLE syllabus_extractions ADD COLUMN course_id TEXT REFERENCES courses(id) ON DELETE SET NULL;

-- ICS VEVENT UID, carried through so re-syncing a calendar feed can skip
-- events already imported (in any review state) instead of spamming
-- duplicate pending extractions on every sync.
ALTER TABLE syllabus_extractions ADD COLUMN external_uid TEXT;
CREATE INDEX idx_extractions_external_uid ON syllabus_extractions(external_uid);
