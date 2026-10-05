-- Spaced repetition state. Columns transcribed from `srs::CardState`, which
-- was written and fully tested first precisely so these would not have to be
-- guessed — SQLite cannot retype a column afterwards.

CREATE TABLE card_schedule (
    -- One row per card, so the card id *is* the key. A card with no row here
    -- has simply never been reviewed; the row is created on first review
    -- rather than for every card up front, so generating 40 cards does not
    -- write 40 schedule rows nobody has looked at.
    card_id TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    repetitions INTEGER NOT NULL DEFAULT 0,
    interval_days INTEGER NOT NULL DEFAULT 0,
    ease_factor REAL NOT NULL DEFAULT 2.5,
    due_date TEXT NOT NULL,
    -- Lifetime, never reset by a recovery: this is what leech detection reads.
    lapses INTEGER NOT NULL DEFAULT 0,
    -- A card taken out of rotation by hand, or flagged as a leech. Kept rather
    -- than deleted, so un-suspending restores its history instead of starting
    -- it over.
    suspended INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_card_schedule_due ON card_schedule(due_date, suspended);

-- One row per graded answer. Deliberately normalized rather than a JSON blob
-- like `practice_attempts`: every Area 8 metric — accuracy over time, the
-- weak-card list, study time, streaks — is a query over this table, and none
-- of them is expressible against a blob with no card_id in it.
CREATE TABLE card_reviews (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    rating TEXT NOT NULL,
    -- The CLIENT's local calendar date. A streak and "due today" are
    -- local-day concepts, and the server has no idea what day it is for the
    -- student: it runs in UTC, and a 11pm review in Eastern time is already
    -- tomorrow by UTC. Supplying it from the browser is the only way to get
    -- day boundaries right without storing a timezone and trusting it.
    local_date TEXT NOT NULL,
    reviewed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- Client-reported and server-clamped. Honest about what it is: the time
    -- the card was on screen, which is not the same as time spent thinking.
    duration_ms INTEGER,
    -- The interval before and after this answer, so accuracy history and
    -- "is this card getting easier" are answerable without replaying SM-2.
    interval_before INTEGER NOT NULL,
    interval_after INTEGER NOT NULL,
    ease_after REAL NOT NULL,
    was_lapse INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_card_reviews_card ON card_reviews(card_id);
CREATE INDEX idx_card_reviews_date ON card_reviews(local_date);
