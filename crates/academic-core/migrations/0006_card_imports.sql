-- Staging for generated cards, so nothing reaches a deck without being seen.
--
-- Same shape as `syllabus_extractions` (0001/0002), which is this codebase's
-- established review-before-commit pattern: candidates land here as `pending`,
-- and only an explicit approval promotes one into a real `cards` row. The AI
-- never writes to `cards` directly.
--
-- It also exists so a generation that took two minutes and real money survives
-- a closed tab: the candidates are durable the moment they are parsed, not held
-- in a browser's memory until the student finishes reviewing them.

CREATE TABLE card_imports (
    id TEXT PRIMARY KEY,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    -- paste today; pdf/docx/pptx/quizlet/anki_csv arrive in later increments
    -- without needing another migration.
    source_kind TEXT NOT NULL DEFAULT 'paste',
    -- What the student called it, or a filename — so a deck with several
    -- imports shows which is which.
    source_label TEXT,
    -- The material the cards came from, kept whole rather than only as
    -- per-card excerpts. This is what later grounds an "explain why I got this
    -- wrong" answer in the student's own notes instead of the model's general
    -- knowledge, and what makes a re-generation possible without re-pasting.
    source_text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'generating',
    error_message TEXT,
    -- What this generation actually cost, carried out of the AI seam by
    -- ExtractionResponse. Null when the provider did not report one.
    total_cost_usd REAL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_card_imports_deck ON card_imports(deck_id);
CREATE INDEX idx_card_imports_status ON card_imports(status);

CREATE TABLE card_candidates (
    id TEXT PRIMARY KEY,
    import_id TEXT NOT NULL REFERENCES card_imports(id) ON DELETE CASCADE,
    order_index INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL DEFAULT 'basic',
    front TEXT NOT NULL,
    back TEXT NOT NULL,
    options_json TEXT,
    explanation TEXT,
    tags_json TEXT,
    -- The passage of `source_text` this card came from, so the review screen
    -- shows provenance rather than asking the student to trust a bare claim —
    -- the same reason syllabus_extractions carries one.
    source_excerpt TEXT,
    review_status TEXT NOT NULL DEFAULT 'pending',
    -- ON DELETE SET NULL, not CASCADE: deleting a card the student approved
    -- earlier must not erase the record that it was reviewed and approved.
    resulting_card_id TEXT REFERENCES cards(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_card_candidates_import ON card_candidates(import_id);
CREATE INDEX idx_card_candidates_status ON card_candidates(review_status);
