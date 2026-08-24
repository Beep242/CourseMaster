import { useEffect, useState } from "react";
import invoke from "../api";
import type { Course, PrioritizedItem } from "../types";
import { IconClock, IconInbox } from "../icons";

interface Props {
  onOpenCourse: (id: string) => void;
}

export function Dashboard({ onOpenCourse }: Props) {
  const [items, setItems] = useState<PrioritizedItem[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [p, c] = await Promise.all([
        invoke<PrioritizedItem[]>("prioritized_today"),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setItems(p);
      setCourses(c);
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
