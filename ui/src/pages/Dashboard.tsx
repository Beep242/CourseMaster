import { useEffect, useState } from "react";
import invoke from "../api";
import type { Course, PrioritizedItem, TodaySummary } from "../types";
import { IconBolt, IconClock, IconInbox } from "../icons";

interface Props {
  onOpenCourse: (id: string) => void;
  onStartReview: () => void;
}

export function Dashboard({ onOpenCourse, onStartReview }: Props) {
  const [items, setItems] = useState<PrioritizedItem[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [today, setToday] = useState<TodaySummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const now = new Date();
      const pad = (n: number) => String(n).padStart(2, "0");
      const localDate = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
      const [p, c, t] = await Promise.all([
        invoke<PrioritizedItem[]>("prioritized_today"),
        invoke<Course[]>("list_courses", { semesterId: null }),
        invoke<TodaySummary>("today", { localDate }),
      ]);
      setItems(p);
      setCourses(c);
      setToday(t);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  const overdue = items.filter((i) => i.is_overdue).length;
  const courseName = (id: string) => courses.find((c) => c.id === id)?.name ?? "Unknown course";

  return (
    <div>
      <p className="hint">What should I do right now?</p>
      {error && <div className="error-banner">{error}</div>}

      {today?.nudge && (
        <div className="card">
          <div className="row" style={{ justifyContent: "space-between", margin: 0 }}>
            <span>{today.nudge}</span>
            {today.due_count + today.new_count > 0 && (
              <button type="button" className="btn-secondary" onClick={onStartReview}>
                <IconBolt /> Review {today.due_count + today.new_count}
              </button>
            )}
          </div>
        </div>
      )}

      {today && today.exams.length > 0 && (
        <div className="card">
          <h3>Coming up</h3>
          {today.exams.slice(0, 5).map((e) => (
            <div key={e.assignment_id} className="row" style={{ justifyContent: "space-between", margin: "0.35rem 0" }}>
              <span>
                <span className={`kind-pill kind-${e.kind}`}>{e.kind}</span> {e.title}
                {e.course_name ? <span className="hint"> · {e.course_name}</span> : null}
              </span>
              <span className={e.days_away <= 3 ? "badge badge-danger" : "badge badge-neutral"}>
                {e.days_away === 0 ? "today" : e.days_away === 1 ? "tomorrow" : `${e.days_away} days`}
              </span>
            </div>
          ))}
        </div>
      )}

      <div className="stat-row">
        <div className="stat">
          <span className="stat-value">{items.length}</span>
          <span className="stat-label">Active items</span>
        </div>
        <div className="stat">
          <span className="stat-value">{overdue}</span>
          <span className="stat-label">Overdue</span>
        </div>
        <div className="stat">
          <span className="stat-value">{courses.length}</span>
          <span className="stat-label">Courses</span>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <IconClock />
            <h3>Up next</h3>
          </div>
          <button type="button" className="btn-ghost" onClick={load}>
            Refresh
          </button>
        </div>
        {loading ? (
          <p className="hint">Loading…</p>
        ) : items.length === 0 ? (
          <div className="empty-state">
            <IconInbox width={32} height={32} />
            <p>Nothing on the radar yet. Add a course and paste a syllabus to get started.</p>
          </div>
        ) : (
          <div className="priority-list">
            {items.slice(0, 15).map((item, i) => (
              <div
                key={item.id}
                className={`priority-item ${item.is_overdue ? "overdue" : ""}`}
                onClick={() => onOpenCourse(item.course_id)}
                role="button"
              >
                <span className="priority-rank">{i + 1}</span>
                <div className="priority-body">
                  <div className="priority-title">{item.title}</div>
                  <div className="priority-reason">
                    {courseName(item.course_id)} · {item.reason}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
