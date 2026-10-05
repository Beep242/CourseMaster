import { useEffect, useState } from "react";
import invoke from "../api";
import type { Course, DayAccuracy, DeckMastery, StudyTotals, WeakCard } from "../types";
import { IconClock, IconInbox, IconRefresh } from "../icons";

interface Readiness {
  score: number;
  reason: string;
  coverage: number;
  retention: number;
  accuracy: number;
  has_enough_data: boolean;
}

interface ProgressData {
  totals: StudyTotals;
  history: DayAccuracy[];
  weak_cards: WeakCard[];
  decks: DeckMastery[];
  readiness: Readiness;
  next_exam: { title: string; due_date: string; days_away: number } | null;
}

function localToday(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function minutes(ms: number): string {
  const m = Math.round(ms / 60000);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

const pct = (n: number) => `${Math.round(n * 100)}%`;

/**
 * A sparkline drawn as inline SVG — no chart library, keeping the two-dependency
 * frontend intact. Daily accuracy over time; bar height is accuracy and bar
 * opacity carries volume, so a 100% day built on two cards does not look like a
 * 100% day built on forty.
 */
function AccuracySparkline({ history }: { history: DayAccuracy[] }) {
  if (history.length === 0) return <p className="hint">No reviews yet.</p>;
  const width = 100;
  const height = 28;
  const maxTotal = Math.max(...history.map((d) => d.total), 1);
  const barWidth = width / history.length;
  return (
    <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" style={{ width: "100%", height: 56 }} role="img"
      aria-label={`Accuracy over the last ${history.length} days`}>
      {history.map((d, i) => {
        const h = Math.max(d.accuracy * height, 1);
        return (
          <rect
            key={d.day}
            x={i * barWidth + barWidth * 0.15}
            y={height - h}
            width={barWidth * 0.7}
            height={h}
            fill="var(--accent)"
            opacity={0.35 + 0.65 * (d.total / maxTotal)}
          >
            <title>{`${d.day}: ${d.correct}/${d.total} correct`}</title>
          </rect>
        );
      })}
    </svg>
  );
}

export function Progress() {
  const [data, setData] = useState<ProgressData | null>(null);
  const [courses, setCourses] = useState<Course[]>([]);
  const [courseId, setCourseId] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load(course: string) {
    setLoading(true);
    setError(null);
    try {
      const [p, c] = await Promise.all([
        invoke<ProgressData>("progress", { courseId: course || null, localDate: localToday() }),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setData(p);
      setCourses(c);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load(courseId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [courseId]);

  if (loading && !data) return <p className="hint">Loading…</p>;

  return (
    <div>
      <p className="hint">How ready are you, and what is holding it back.</p>
      {error && <div className="error-banner">{error}</div>}

      <div className="row" style={{ justifyContent: "space-between" }}>
        <select value={courseId} onChange={(e) => setCourseId(e.target.value)} style={{ maxWidth: 260 }}>
          <option value="">All courses</option>
          {courses.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <button type="button" className="btn-ghost" onClick={() => load(courseId)}>
          <IconRefresh /> Refresh
        </button>
      </div>

      {data && (
        <>
          <div className="card">
            <div className="card-header">
              <div className="card-header-title">
                <IconClock />
                <h3>Exam ready</h3>
              </div>
              <span className="stat-value" style={{ fontSize: "1.8em" }}>
                {data.readiness.has_enough_data ? `${data.readiness.score}%` : "—"}
              </span>
            </div>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${data.readiness.score}%` }} />
            </div>
            {/* The reason is the point: a score nobody can interrogate is a
                vibe with a percent sign. */}
            <p className="hint" style={{ marginBottom: 0 }}>{data.readiness.reason}</p>
            <div className="stat-row">
              <div className="stat">
                <span className="stat-value">{pct(data.readiness.coverage)}</span>
                <span className="stat-label">Coverage</span>
              </div>
              <div className="stat">
                <span className="stat-value">{pct(data.readiness.retention)}</span>
                <span className="stat-label">Durable</span>
              </div>
              <div className="stat">
                <span className="stat-value">{pct(data.readiness.accuracy)}</span>
                <span className="stat-label">Accuracy</span>
              </div>
            </div>
            {data.next_exam && (
              <p className="hint" style={{ marginBottom: 0 }}>
                Next: <strong>{data.next_exam.title}</strong> in {data.next_exam.days_away}{" "}
                {data.next_exam.days_away === 1 ? "day" : "days"} ({data.next_exam.due_date})
              </p>
            )}
          </div>

          <div className="stat-row">
            <div className="stat">
              <span className="stat-value">{data.totals.reviews}</span>
              <span className="stat-label">Reviews</span>
            </div>
            <div className="stat">
              <span className="stat-value">{data.totals.days_studied}</span>
              <span className="stat-label">Days studied</span>
            </div>
            <div className="stat">
              <span className="stat-value">{minutes(data.totals.duration_ms)}</span>
              <span className="stat-label">Time on cards</span>
            </div>
          </div>

          <div className="card">
            <h3>Accuracy by day</h3>
            <AccuracySparkline history={data.history} />
            <p className="hint" style={{ marginBottom: 0 }}>
              Taller is more accurate; fainter bars are days with fewer cards.
            </p>
          </div>

          <div className="card">
            <h3>Decks</h3>
            {data.decks.length === 0 ? (
              <p className="hint">No decks yet.</p>
            ) : (
              data.decks.map((d) => (
                <div key={d.deck_id} style={{ margin: "0.7rem 0" }}>
                  <div className="row" style={{ justifyContent: "space-between", margin: 0 }}>
                    <span>
                      {d.deck_name}
                      {d.course_name ? <span className="hint"> · {d.course_name}</span> : null}
                    </span>
                    <span className="hint" style={{ margin: 0 }}>
                      {d.mature_cards}/{d.total_cards} durable
                    </span>
                  </div>
                  <div className="progress-track">
                    <div className="progress-fill" style={{ width: `${d.mastery * 100}%` }} />
                  </div>
                </div>
              ))
            )}
          </div>

          <div className="card">
            <h3>Cards you keep missing</h3>
            {data.weak_cards.length === 0 ? (
              <div className="empty-state">
                <IconInbox width={28} height={28} />
                <p>Nothing you're repeatedly getting wrong.</p>
              </div>
            ) : (
              data.weak_cards.map((w) => (
                <div key={w.card_id} className="extraction-card">
                  <div className="extraction-head">
                    <span className="extraction-title">{w.front}</span>
                    <span className="badge badge-warning">
                      {w.lapses} {w.lapses === 1 ? "miss" : "misses"}
                    </span>
                  </div>
                  <div className="extraction-meta">
                    <span>
                      {w.deck_name}
                      {w.course_name ? ` · ${w.course_name}` : ""}
                    </span>
                    <span>{pct(w.accuracy)} correct over {w.total_reviews}</span>
                  </div>
                </div>
              ))
            )}
          </div>
        </>
      )}
    </div>
  );
}
