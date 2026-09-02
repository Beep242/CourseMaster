import { useEffect, useState } from "react";
import invoke from "../api";
import type { Course, Semester } from "../types";
import { IconChevronRight, IconInbox, IconPlus, IconX } from "../icons";

interface Props {
  onOpenCourse: (id: string) => void;
}

const SWATCHES = ["#8b5cf6", "#d946ef", "#34d399", "#fbbf24", "#38bdf8", "#fb7185"];

export function Courses({ onOpenCourse }: Props) {
  const [semesters, setSemesters] = useState<Semester[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [saving, setSaving] = useState(false);

  const [name, setName] = useState("");
  const [code, setCode] = useState("");
  const [professor, setProfessor] = useState("");
  const [credits, setCredits] = useState("3");
  const [color, setColor] = useState(SWATCHES[0]);
  const [semesterId, setSemesterId] = useState("");

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [s, c] = await Promise.all([invoke<Semester[]>("list_semesters"), invoke<Course[]>("list_courses", { semesterId: null })]);
      setSemesters(s);
      setCourses(c);
      if (!semesterId && s.length > 0) setSemesterId(s[0].id);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function createCourse() {
    if (!name.trim() || !semesterId) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("create_course", {
        input: {
          semester_id: semesterId,
          name: name.trim(),
          code: code.trim() || null,
          professor_name: professor.trim() || null,
          professor_email: null,
          credit_hours: credits ? Number(credits) : null,
          color,
        },
      });
      setName("");
      setCode("");
      setProfessor("");
      setShowForm(false);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function removeCourse(id: string, name: string) {
    if (!window.confirm(`Delete "${name}"? This also deletes its assignments, syllabi, and study materials — this can't be undone.`)) return;
    setError(null);
    try {
      await invoke("delete_course", { id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeSemester(id: string, name: string) {
    if (!window.confirm(`Delete the semester "${name}"? This deletes every course in it, and everything attached to those courses — this can't be undone.`)) return;
    setError(null);
    try {
      await invoke("delete_semester", { id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div>
      <p className="hint">Every course you're taking this semester, in one place.</p>
      {error && <div className="error-banner">{error}</div>}

      {semesters.length > 0 && (
        <div className="card">
          <h3>Semesters</h3>
          {semesters.map((s) => (
            <div key={s.id} className="row" style={{ justifyContent: "space-between", margin: "0.4rem 0" }}>
              <span>{s.name}</span>
              <button type="button" className="btn-ghost" onClick={() => removeSemester(s.id, s.name)} title="Delete semester">
                <IconX />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="row" style={{ justifyContent: "flex-end" }}>
        <button type="button" onClick={() => setShowForm((v) => !v)}>
          <IconPlus /> New course
        </button>
      </div>

      {showForm && (
        <div className="card">
          <h3>Add a course</h3>
          <div className="field-grid">
            <div>
              <label className="field-label">Course name</label>
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Algorithms" autoFocus />
            </div>
            <div>
              <label className="field-label">Code</label>
              <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="CS 301" />
            </div>
            <div>
              <label className="field-label">Professor</label>
              <input value={professor} onChange={(e) => setProfessor(e.target.value)} placeholder="Dr. Alvarez" />
            </div>
            <div>
              <label className="field-label">Credit hours</label>
              <input type="number" step="0.5" value={credits} onChange={(e) => setCredits(e.target.value)} />
            </div>
            <div>
              <label className="field-label">Semester</label>
              <select value={semesterId} onChange={(e) => setSemesterId(e.target.value)}>
                {semesters.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="field-label">Color</label>
              <div className="row" style={{ margin: "0.3rem 0" }}>
                {SWATCHES.map((swatch) => (
                  <span
                    key={swatch}
                    onClick={() => setColor(swatch)}
                    style={{
                      width: 22,
                      height: 22,
                      borderRadius: "50%",
                      background: swatch,
                      cursor: "pointer",
                      display: "inline-block",
                      border: color === swatch ? "2px solid var(--text-main)" : "2px solid transparent",
                    }}
                  />
                ))}
              </div>
            </div>
          </div>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn-ghost" onClick={() => setShowForm(false)}>
              Cancel
            </button>
            <button type="button" disabled={saving || !name.trim()} onClick={createCourse}>
              {saving ? "Adding…" : "Add course"}
            </button>
          </div>
        </div>
      )}

      {loading ? (
        <p className="hint">Loading…</p>
      ) : courses.length === 0 ? (
        <div className="empty-state">
          <IconInbox width={32} height={32} />
          <p>No courses yet. Add your first one above.</p>
        </div>
      ) : (
        <div className="course-grid">
          {courses.map((course) => (
            <div key={course.id} className="card course-card" onClick={() => onOpenCourse(course.id)}>
              <div className="course-card-title">
                <span className="course-color-dot" style={{ background: course.color }} />
                {course.name}
                <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 4 }}>
                  <button
                    type="button"
                    className="btn-ghost"
                    style={{ padding: "0.2em" }}
                    title="Delete course"
                    onClick={(e) => {
                      e.stopPropagation();
                      removeCourse(course.id, course.name);
                    }}
                  >
                    <IconX width={15} height={15} />
                  </button>
                  <IconChevronRight />
                </span>
              </div>
              <div className="course-card-meta">
                {course.code ?? "No code"}
                {course.professor_name ? ` · ${course.professor_name}` : ""}
                {course.current_grade ? ` · ${course.current_grade}` : ""}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
