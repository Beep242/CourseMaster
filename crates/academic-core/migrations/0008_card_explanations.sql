-- Cached AI explanations for a wrong answer.
--
-- Keyed on the card plus the *normalised* mistake, so getting a card wrong the
-- same way twice is free. Normalising the key is what makes that true in
-- practice: "Mitochondria" and "mitochondria." are the same misunderstanding
-- and should not be two paid calls.
CREATE TABLE card_explanations (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    -- grading::normalize of what the student wrote; empty string for "no
    -- answer given", which is a distinct and common case worth caching once.
    mistake_key TEXT NOT NULL,
    -- What they actually typed, kept verbatim for display — the key is lossy
    -- by design.
    submitted TEXT NOT NULL,
    explanation TEXT NOT NULL,
    total_cost_usd REAL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
-- The cache lookup, and the thing that makes a repeat mistake free.
CREATE UNIQUE INDEX idx_card_explanations_key ON card_explanations(card_id, mistake_key);
