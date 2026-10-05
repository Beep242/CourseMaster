import { useCallback, useEffect, useRef, useState } from "react";
import invoke from "../api";
import type { CheckAnswerResult, Course, DueCard, Rating, ReviewOutcome } from "../types";
import { IconCheck, IconInbox, IconRefresh } from "../icons";

/** The browser's own calendar date. See the API's `resolve_today`: the server
 *  runs in UTC, so an 11pm review in Eastern time is already tomorrow to it,
 *  and "due today" is a local-day idea. */
function localToday(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** "1 day" / "6 days" / "2 mo" — a button label, not a precise duration. */
function humanInterval(days: number): string {
  if (days <= 0) return "today";
  if (days === 1) return "1 day";
  if (days < 30) return `${days} days`;
  if (days < 365) return `${Math.round(days / 30)} mo`;
  return `${(days / 365).toFixed(1)} yr`;
}

const GRADES: { rating: Rating; label: string; key: string; className: string }[] = [
  { rating: "again", label: "Again", key: "1", className: "btn-danger" },
  { rating: "hard", label: "Hard", key: "2", className: "btn-secondary" },
  { rating: "good", label: "Good", key: "3", className: "" },
  { rating: "easy", label: "Easy", key: "4", className: "btn-secondary" },
];

interface Props {
  /** Restrict to one course; omitted on the top-level Review page. */
  courseId?: string;
  /** Shown instead of the queue when there is nothing due. */
  emptyHint?: string;
}

/**
 * The due-today queue, spanning every deck and course.
 *
 * This is the screen spaced repetition actually happens on: the flip session
 * (increment 8) is a read-through of one deck, this is "what should I study
 * now" across all of them.
 */
export function Review({ courseId, emptyHint }: Props) {
  const [queue, setQueue] = useState<DueCard[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [index, setIndex] = useState(0);
  const [flipped, setFlipped] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [done, setDone] = useState(0);
  const [lapsed, setLapsed] = useState(0);
  const [leech, setLeech] = useState<DueCard | null>(null);
  const shownAt = useRef<number>(Date.now());

  /** Flip through, or type the answer and have it checked. Typing is graded by
   *  the deterministic matcher, so it costs nothing and answers instantly. */
  const [mode, setMode] = useState<"flip" | "type">("flip");
  const [typed, setTyped] = useState("");
  const [checked, setChecked] = useState<CheckAnswerResult | null>(null);
  const [checking, setChecking] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [q, c] = await Promise.all([
        invoke<DueCard[]>("review_queue", { courseId: courseId ?? null, localDate: localToday() }),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setQueue(q);
      setCourses(c);
      setIndex(0);
      setFlipped(false);
      setTyped("");
      setChecked(null);
      shownAt.current = Date.now();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [courseId]);

  useEffect(() => {
    load();
  }, [load]);

  const card = queue[index];

  const grade = useCallback(
    async (rating: Rating) => {
      if (!card || submitting) return;
      setSubmitting(true);
      setError(null);
      try {
        const outcome = await invoke<ReviewOutcome>("submit_review", {
          id: card.id,
          input: {
            rating,
            local_date: localToday(),
            // How long the card was on screen. Honest about what it measures —
            // the server clamps it, since a card left open over lunch is not
            // two hours of study.
            duration_ms: Date.now() - shownAt.current,
          },
        });
        setDone((d) => d + 1);
        if (rating === "again") setLapsed((l) => l + 1);
        if (outcome.is_leech) setLeech(card);
        setFlipped(false);
        setTyped("");
        setChecked(null);
        setIndex((i) => i + 1);
        shownAt.current = Date.now();
      } catch (e) {
        setError(String(e));
      } finally {
        setSubmitting(false);
      }
    },
    [card, submitting],
  );

  async function checkTyped() {
    if (!card || !typed.trim() || checking) return;
    setChecking(true);
    setError(null);
    try {
      const result = await invoke<CheckAnswerResult>("check_answer", { id: card.id, answer: typed });
      setChecked(result);
      // Reveal regardless of verdict: seeing the real answer is the point of
      // being wrong, and confirms it when right.
      setFlipped(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  }

  async function suspendLeech() {
    if (!leech) return;
    try {
      await invoke("suspend_card", { id: leech.id, suspended: true, localDate: localToday() });
      setLeech(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!card) return;
      // While typing an answer, the keyboard belongs to the input — grabbing
      // Space would make the field unusable.
      if (mode === "type" && !checked) return;
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setFlipped((f) => !f);
        return;
      }
      // Grades only once the answer is visible — grading a card you have not
      // seen the back of is always a misclick.
      if (!flipped) return;
      const match = GRADES.find((g) => g.key === e.key);
      if (match) {
        e.preventDefault();
        grade(match.rating);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [card, flipped, grade, mode, checked]);

  if (loading) return <p className="hint">Loading your queue…</p>;

  if (error && queue.length === 0) {
    return (
      <div>
        <div className="error-banner">{error}</div>
        <button type="button" className="btn-secondary" onClick={load}>
          <IconRefresh /> Try again
        </button>
      </div>
    );
  }

  if (queue.length === 0) {
    return (
      <div className="empty-state">
        <IconCheck width={32} height={32} />
        <p>Nothing due right now.</p>
        <p className="hint">{emptyHint ?? "Add cards to a deck, or come back tomorrow."}</p>
      </div>
    );
  }

  if (!card) {
    return (
      <div className="empty-state">
        <IconCheck width={32} height={32} />
        <p>Done — {done} {done === 1 ? "card" : "cards"} reviewed.</p>
        {lapsed > 0 && <p className="hint">{lapsed} to see again soon.</p>}
        <button type="button" onClick={load}>
          <IconRefresh /> Check for more
        </button>
      </div>
    );
  }

  const courseName = card.course_name ?? courses.find((c) => c.id === courseId)?.name;

  return (
    <div className="study-stage">
      {error && <div className="error-banner">{error}</div>}

      {leech && (
        <div className="card" style={{ width: "100%", maxWidth: 640 }}>
          <p style={{ margin: 0 }}>
            You keep forgetting “{leech.front.slice(0, 60)}”. Usually that means the card is doing too much, not that you
            need to see it more often.
          </p>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn-ghost" onClick={() => setLeech(null)}>
              Keep it
            </button>
            <button type="button" className="btn-secondary" onClick={suspendLeech}>
              Suspend it
            </button>
          </div>
        </div>
      )}

      <div className="study-progress">
        <div className="progress-track">
          <div className="progress-fill" style={{ width: `${(index / queue.length) * 100}%` }} />
        </div>
        <span>
          {index} / {queue.length} · {done} done
        </span>
      </div>

      <div className="tabs" style={{ width: "100%", maxWidth: 640 }}>
        <button
          type="button"
          className={`tab-item ${mode === "flip" ? "active" : ""}`}
          onClick={() => {
            setMode("flip");
            setChecked(null);
            setTyped("");
          }}
        >
          Flip
        </button>
        <button
          type="button"
          className={`tab-item ${mode === "type" ? "active" : ""}`}
          onClick={() => {
            setMode("type");
            setFlipped(false);
            setChecked(null);
          }}
        >
          Type the answer
        </button>
      </div>

      <div className="row" style={{ justifyContent: "space-between", width: "100%", maxWidth: 640, fontSize: "0.82em" }}>
        <span className="hint" style={{ margin: 0 }}>
          {card.deck_name}
          {courseName ? ` · ${courseName}` : ""}
        </span>
        {card.is_new ? (
          <span className="badge badge-success">new</span>
        ) : (
          <span className="badge badge-neutral">seen {card.schedule?.repetitions ?? 0}×</span>
        )}
      </div>

      <button
        type="button"
        className={`flashcard ${flipped ? "flipped" : ""}`}
        onClick={() => setFlipped((f) => !f)}
        aria-label={flipped ? "Showing the answer" : "Showing the question. Activate to reveal the answer."}
      >
        <div className="flashcard-inner">
          <div className="flashcard-face">
            <div className="flashcard-text">{card.front}</div>
            <span className="flashcard-hint">Tap, or press Space, to reveal</span>
          </div>
          <div className="flashcard-face back">
            <div className="flashcard-text">{card.back}</div>
            {card.explanation && <p className="hint" style={{ margin: 0, textAlign: "center" }}>{card.explanation}</p>}
          </div>
        </div>
      </button>

      {checked && (
        <div className="study-progress" style={{ justifyContent: "center" }}>
          {checked.verdict === "correct" && <span className="badge badge-success">Correct</span>}
          {checked.verdict === "incorrect" && <span className="badge badge-danger">Not quite</span>}
          {checked.verdict === "undecided" && (
            <span className="badge badge-warning">Close — you be the judge</span>
          )}
          <span className="hint" style={{ margin: 0 }}>
            you wrote &ldquo;{typed}&rdquo;
          </span>
        </div>
      )}

      {flipped ? (
        <div className="study-controls">
          {GRADES.map((g) => (
            <button
              key={g.rating}
              type="button"
              className={checked?.suggested_rating === g.rating ? "" : g.className}
              disabled={submitting}
              style={checked?.suggested_rating === g.rating ? { outline: "2px solid var(--accent)" } : undefined}
              onClick={() => grade(g.rating)}
              title={`Press ${g.key}`}
            >
              {g.label}
              <span style={{ opacity: 0.7, marginLeft: 6, fontSize: "0.85em" }}>
                {humanInterval(card.projections[g.rating])}
              </span>
            </button>
          ))}
        </div>
      ) : mode === "type" ? (
        <div style={{ width: "100%", maxWidth: 640 }}>
          <input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                checkTyped();
              }
            }}
            placeholder="Type your answer, then press Enter"
            aria-label="Your answer"
            autoFocus
          />
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" disabled={checking || !typed.trim()} onClick={checkTyped}>
              {checking ? "Checking…" : "Check"}
            </button>
          </div>
        </div>
      ) : (
        <div className="study-controls">
          <button type="button" onClick={() => setFlipped(true)}>
            Show answer
          </button>
        </div>
      )}

      <p className="hint" style={{ textAlign: "center" }}>
        {flipped ? "1 Again · 2 Hard · 3 Good · 4 Easy" : "Space reveals the answer"}
      </p>
    </div>
  );
}
