-- Flashcards: the container the study features in FEATURES_PLAN.md hang off.
--
-- Deliberately NOT reusing `practice_questions` (0004). That table is owned by
-- one `practice_tests` row via ON DELETE CASCADE so a question cannot be moved
-- or shared, and it has no created_at/updated_at, no tags, no media and no
-- provenance columns. `practice_attempts` is a single answers_json blob with no
-- question_id anywhere, so it cannot express per-card history either. Practice
-- tests stay the graded mock-exam feature they already are; the two coexist,
-- and a deck can later generate a practice test from its cards.

CREATE TABLE decks (
    id TEXT PRIMARY KEY,
    -- Nullable on purpose, mirroring the `syllabi.course_id` precedent from
    -- 0002: an import can land before the student has decided which course it
    -- belongs to, and a deck filed under no course is better than an import
    -- that cannot be saved. It also makes a flat course-less library the
    -- degenerate case rather than a separate mode.
    course_id TEXT REFERENCES courses(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    color TEXT NOT NULL DEFAULT '#8b5cf6',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_decks_course ON decks(course_id);

-- A card hangs off exactly ONE deck and reaches its course transitively
-- through decks.course_id. Deliberately not denormalised with its own
-- course_id: this schema has no triggers and no cross-table CHECK, so two
-- parents would be an unenforceable consistency bug, and every cross-course
-- query needed here is a two-table join over a few thousand rows.
--
-- Also deliberately not Anki's note-vs-card split, and no cards<->decks join
-- table: a card in two decks buys one student nothing and is the single
-- largest source of complexity in Anki imports.
CREATE TABLE cards (
    id TEXT PRIMARY KEY,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL DEFAULT 'basic',
    front TEXT NOT NULL,
    back TEXT NOT NULL,
    -- Multiple-choice distractors, generated once when the card is saved and
    -- stored, so a review costs no AI call and has no latency.
    options_json TEXT,
    explanation TEXT,
    tags_json TEXT,
    -- The verbatim passage a generated card came from. NULLABLE because a
    -- hand-written card has no source — and SQLite cannot relax a NOT NULL
    -- afterwards, so getting this wrong would mean rebuilding the table.
    -- This is what later grounds an "explain why I got this wrong" answer in
    -- the student's own material instead of in the model's general knowledge.
    source_excerpt TEXT,
    -- Reserved. Images on cards are deferred (see FEATURES_PLAN.md: no object
    -- storage, and /app/data is a volume but is not served), but the column
    -- costs nothing now and means enabling them later needs no migration
    -- against this table.
    image_data_uri TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_cards_deck ON cards(deck_id);
-- Cheap, and it is what keeps "next card in this deck" ordering from degrading
-- into a sort of the whole table once a deck holds a few hundred cards.
CREATE INDEX idx_cards_deck_order ON cards(deck_id, order_index);
