import { useEffect, useState } from "react";
import invoke from "../api";
import type { Assignment, AssignmentStatus, Course, Syllabus, SyllabusExtraction } from "../types";
import { IconCheck, IconDoc, IconInbox, IconPlus, IconSparkle, IconX } from "../icons";

interface Props {
  courseId: string;
}

const STATUS_LABEL: Record<AssignmentStatus, string> = {
  not_started: "Not started",
  in_progress: "In progress",
  waiting: "Waiting",
  completed: "Completed",
  submitted: "Submitted",
  overdue: "Overdue",
};

export function CourseDetail({ courseId }: Props) {
  const [course, setCourse] = useState<Course | null>(null);
  const [tab, setTab] = useState<"assignments" | "syllabus" | "ask">("assignments");
  const [assignments, setAssignments] = useState<Assignment[]>([]);
  const [syllabi, setSyllabi] = useState<Syllabus[]>([]);
  const [extractions, setExtractions] = useState<Record<string, SyllabusExtraction[]>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [syllabusText, setSyllabusText] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);

  const [showAssignmentForm, setShowAssignmentForm] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newDue, setNewDue] = useState("");

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [c, a, s] = await Promise.all([
        invoke<Course | null>("get_course", { id: courseId }),
        invoke<Assignment[]>("list_assignments", { courseId }),
        invoke<Syllabus[]>("list_syllabi", { courseId }),
      ]);
      setCourse(c);
      setAssignments(a);
      setSyllabi(s);
      const extractionEntries = await Promise.all(
        s.map(async (syl) => [syl.id, await invoke<SyllabusExtraction[]>("list_extractions", { syllabusId: syl.id })] as const),
      );
      setExtractions(Object.fromEntries(extractionEntries));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [courseId]);

  async function submitSyllabus() {
    if (!syllabusText.trim()) return;
    setSubmitting(true);
    setError(null);
    try {
      await invoke("submit_syllabus", { courseId, rawText: syllabusText });
      setSyllabusText("");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  }

  async function approve(id: string) {
    try {
      await invoke("approve_extraction", { extractionId: id, edits: null });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function reject(id: string) {
    try {
      await invoke("reject_extraction", { extractionId: id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function updateStatus(id: string, status: AssignmentStatus) {
    try {
      await invoke("update_assignment", { id, patch: { status } });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function updateCompletion(id: string, completion_percentage: number) {
    try {
      await invoke("update_assignment", { id, patch: { completion_percentage } });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function createAssignment() {
    if (!newTitle.trim()) return;
    try {
      await invoke("create_assignment", {
        input: {
          course_id: courseId,
          title: newTitle.trim(),
          description: null,
          kind: "assignment",
          due_date: newDue || null,
          due_time: null,
          difficulty: "medium",
          estimated_duration_minutes: null,
          priority: "medium",
        },
      });
      setNewTitle("");
      setNewDue("");
      setShowAssignmentForm(false);
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function askSyllabus() {
    if (!question.trim() || syllabi.length === 0) return;
    setAsking(true);
    setAnswer(null);
    try {
      const latest = syllabi[0];
      const result = await invoke<string>("ask_syllabus", { syllabusId: latest.id, question });
      setAnswer(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setAsking(false);
    }
  }

  const pendingCount = Object.values(extractions)
    .flat()
    .filter((e) => e.review_status === "pending").length;

  if (loading) return <p className="hint">Loading…</p>;
  if (!course) return <div className="error-banner">Course not found.</div>;

  return (
    <div>
      <p className="hint">
        {course.code ? `${course.code} · ` : ""}
        {course.professor_name ?? "No professor listed"}
      </p>
      {error && <div className="error-banner">{error}</div>}

      <div className="tabs">
        <button type="button" className={`tab-item ${tab === "assignments" ? "active" : ""}`} onClick={() => setTab("assignments")}>
          Assignments
        </button>
        <button type="button" className={`tab-item ${tab === "syllabus" ? "active" : ""}`} onClick={() => setTab("syllabus")}>
          Syllabus{pendingCount > 0 ? ` (${pendingCount} to review)` : ""}
        </button>
        <button type="button" className={`tab-item ${tab === "ask" ? "active" : ""}`} onClick={() => setTab("ask")}>
          Ask my syllabus
        </button>
      </div>

      {tab === "assignments" && (
        <div>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" onClick={() => setShowAssignmentForm((v) => !v)}>
              <IconPlus /> New assignment
            </button>
          </div>
          {showAssignmentForm && (
            <div className="card">
              <div className="field-grid">
                <div>
                  <label className="field-label">Title</label>
                  <input value={newTitle} onChange={(e) => setNewTitle(e.target.value)} autoFocus />
                </div>
                <div>
                  <label className="field-label">Due date</label>
                  <input type="date" value={newDue} onChange={(e) => setNewDue(e.target.value)} />
                </div>
              </div>
              <div className="row" style={{ justifyContent: "flex-end" }}>
                <button type="button" className="btn-ghost" onClick={() => setShowAssignmentForm(false)}>
                  Cancel
                </button>
                <button type="button" disabled={!newTitle.trim()} onClick={createAssignment}>
                  Add
                </button>
              </div>
            </div>
          )}
          {assignments.length === 0 ? (
            <div className="empty-state">
              <IconInbox width={32} height={32} />
              <p>No assignments yet.</p>
            </div>
          ) : (
            <div className="card">
              {assignments.map((a) => (
                <div key={a.id} className="extraction-card">
                  <div className="extraction-head">
                    <div>
                      <span className={`kind-pill kind-${a.kind}`}>{a.kind}</span> <span className="extraction-title">{a.title}</span>
                    </div>
                    <select
                      className="status-select"
                      value={a.status}
                      onChange={(e) => updateStatus(a.id, e.target.value as AssignmentStatus)}
                    >
                      {Object.entries(STATUS_LABEL).map(([value, label]) => (
                        <option key={value} value={value}>
                          {label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div className="extraction-meta">
                    {a.due_date ? <span>Due {a.due_date}</span> : <span>No due date</span>}
                    <span className={`priority-tag ${a.priority}`}>{a.priority}</span>
                  </div>
                  <div className="row" style={{ margin: "0.4rem 0" }}>
                    <div className="progress-track" style={{ flex: 1 }}>
                      <div className="progress-fill" style={{ width: `${a.completion_percentage}%` }} />
                    </div>
                    <input
                      type="range"
                      min={0}
                      max={100}
                      step={5}
                      value={a.completion_percentage}
                      style={{ width: 100 }}
                      onChange={(e) => updateCompletion(a.id, Number(e.target.value))}
                    />
                    <span style={{ fontSize: "0.78em", color: "var(--text-soft)", width: 32 }}>{a.completion_percentage}%</span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {tab === "syllabus" && (
        <div>
          <div className="card">
            <h3>Paste syllabus text</h3>
            <textarea
              rows={8}
              value={syllabusText}
              onChange={(e) => setSyllabusText(e.target.value)}
              placeholder="Paste the full syllabus text here…"
            />
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <button type="button" disabled={submitting || !syllabusText.trim()} onClick={submitSyllabus}>
                {submitting ? "Extracting with AI…" : "Extract assignments"}
              </button>
            </div>
          </div>

          {syllabi.map((syl) => (
            <div key={syl.id} className="card">
              <div className="card-header">
                <div className="card-header-title">
                  <IconDoc />
                  <h3>Imported {new Date(syl.imported_at).toLocaleString()}</h3>
                </div>
                <span
                  className={`badge ${syl.status === "failed" ? "badge-danger" : syl.status === "processing" ? "badge-warning" : "badge-success"}`}
                >
                  {syl.status.replace(/_/g, " ")}
                </span>
              </div>
              {syl.error_message && <div className="error-banner">{syl.error_message}</div>}
              {(extractions[syl.id] ?? []).length === 0 ? (
                <p className="hint">No deliverables extracted from this import.</p>
              ) : (
                (extractions[syl.id] ?? []).map((ex) => (
                  <div key={ex.id} className="extraction-card">
                    <div className="extraction-head">
                      <div>
                        <span className={`kind-pill kind-${ex.kind}`}>{ex.kind}</span> <span className="extraction-title">{ex.title}</span>
                      </div>
                      <span
                        className={`badge ${ex.review_status === "pending" ? "badge-warning" : ex.review_status === "rejected" ? "badge-danger" : "badge-success"}`}
                      >
                        {ex.review_status}
                      </span>
                    </div>
                    <div className="extraction-meta">
                      <span>{ex.due_date ?? "No date found"}</span>
                      <span>Confidence</span>
                      <div className="confidence-track">
                        <div className="confidence-fill" style={{ width: `${Math.round(ex.confidence * 100)}%` }} />
                      </div>
                      <span>{Math.round(ex.confidence * 100)}%</span>
                    </div>
                    <p className="extraction-excerpt">&ldquo;{ex.source_excerpt}&rdquo;</p>
                    {ex.review_status === "pending" && (
                      <div className="extraction-actions">
                        <button type="button" onClick={() => approve(ex.id)}>
                          <IconCheck /> Approve
                        </button>
                        <button type="button" className="btn-danger" onClick={() => reject(ex.id)}>
                          <IconX /> Reject
                        </button>
                      </div>
                    )}
                  </div>
                ))
              )}
            </div>
          ))}
        </div>
      )}

      {tab === "ask" && (
        <div className="card">
          <div className="card-header">
            <div className="card-header-title">
              <IconSparkle />
              <h3>Ask my syllabus</h3>
            </div>
          </div>
          {syllabi.length === 0 ? (
            <p className="hint">Paste a syllabus first — answers are grounded only in what you've imported.</p>
          ) : (
            <>
              <div className="row">
                <input
                  value={question}
                  onChange={(e) => setQuestion(e.target.value)}
                  placeholder="When is the final worth the most?"
                  onKeyDown={(e) => e.key === "Enter" && askSyllabus()}
                />
                <button type="button" disabled={asking || !question.trim()} onClick={askSyllabus}>
                  {asking ? "Asking…" : "Ask"}
                </button>
              </div>
              {answer && <p className="extraction-excerpt">{answer}</p>}
            </>
          )}
        </div>
      )}
    </div>
  );
}
