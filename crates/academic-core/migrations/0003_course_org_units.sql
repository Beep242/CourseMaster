-- Links a Course to the LMS's own stable identifier for it (D2L's org unit
-- ID, extracted from its calendar feed) so a synced event can be matched to
-- the right course exactly, instead of only ever fuzzy-matching on title
-- text. UNIQUE (partial, so multiple manually-created courses with no LMS
-- link can all keep NULL) prevents two courses from accidentally claiming
-- the same LMS org unit.
ALTER TABLE courses ADD COLUMN external_org_unit_id TEXT;
CREATE UNIQUE INDEX idx_courses_org_unit ON courses(external_org_unit_id) WHERE external_org_unit_id IS NOT NULL;

-- Carried on the extraction (not just resolved at sync time) so that
-- creating/linking a course *after* a sync can retroactively backfill
-- course_id on extractions that were already sitting pending review.
ALTER TABLE syllabus_extractions ADD COLUMN external_org_unit_id TEXT;
CREATE INDEX idx_extractions_org_unit ON syllabus_extractions(external_org_unit_id);
