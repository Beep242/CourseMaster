import { useEffect, useState } from "react";
import invoke from "../api";
import type { CalendarFeed, Course, Syllabus, SyllabusExtraction } from "../types";
import { IconCalendar, IconCheck, IconInbox, IconPlus, IconRefresh, IconX } from "../icons";

interface BatchWithExtractions {
  batch: Syllabus;
  extractions: SyllabusExtraction[];
}

export function Imports() {
  const [feeds, setFeeds] = useState<CalendarFeed[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [batchesByFeed, setBatchesByFeed] = useState<Record<string, BatchWithExtractions[]>>({});
  const [loading, setLoading] = useState(true);
  const [syncing, setSyncing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [showForm, setShowForm] = useState(false);
  const [name, setName] = useState("");
  const [icsUrl, setIcsUrl] = useState("");
  const [saving, setSaving] = useState(false);

  async function loadBatchesFor(feedId: string) {
    const batches = await invoke<Syllabus[]>("list_feed_batches", { feedId });
    const withExtractions = await Promise.all(
      batches
        .filter((b) => b.status === "ready_for_review")
        .map(async (batch) => ({
          batch,
          extractions: (await invoke<SyllabusExtraction[]>("list_extractions", { syllabusId: batch.id })).filter(
            (e) => e.review_status === "pending",
          ),
        })),
    );
    setBatchesByFeed((prev) => ({ ...prev, [feedId]: withExtractions.filter((b) => b.extractions.length > 0) }));
  }

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [feedList, courseList] = await Promise.all([
        invoke<CalendarFeed[]>("list_calendar_feeds"),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setFeeds(feedList);
      setCourses(courseList);
      await Promise.all(feedList.map((f) => loadBatchesFor(f.id)));
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

  async function addFeed() {
    if (!icsUrl.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("create_calendar_feed", { input: { name: name.trim() || "My D2L Calendar", ics_url: icsUrl.trim() } });
      setName("");
      setIcsUrl("");
      setShowForm(false);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function doSync(feedId: string) {
    setSyncing(feedId);
    setError(null);
    try {
      await invoke("sync_calendar_feed", { feedId });
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(null);
    }
  }

  async function setExtractionCourse(feedId: string, extractionId: string, courseId: string) {
    setBatchesByFeed((prev) => ({
      ...prev,
      [feedId]: prev[feedId]?.map((b) => ({
        ...b,
        extractions: b.extractions.map((e) => (e.id === extractionId ? { ...e, course_id: courseId || null } : e)),
      })),
    }));
  }

  async function approve(feedId: string, extraction: SyllabusExtraction) {
    if (!extraction.course_id) {
      setError("Pick a course before approving this item.");
      return;
    }
    try {
      await invoke("approve_extraction", { extractionId: extraction.id, edits: { course_id: extraction.course_id } });
      await loadBatchesFor(feedId);
    } catch (e) {
      setError(String(e));
    }
  }

  async function reject(feedId: string, extractionId: string) {
    try {
      await invoke("reject_extraction", { extractionId });
      await loadBatchesFor(feedId);
    } catch (e) {
      setError(String(e));
    }
  }

  const courseName = (id: string) => courses.find((c) => c.id === id)?.name;

  if (loading) return <p className="hint">Loading…</p>;

  return (
    <div>
      <p className="hint">
        Connect your D2L (or any ICS-based) calendar feed — due dates sync in automatically, but nothing reaches your tracker
        without your review here first.
      </p>
      {error && <div className="error-banner">{error}</div>}

      <div className="row" style={{ justifyContent: "flex-end" }}>
        <button type="button" onClick={() => setShowForm((v) => !v)}>
          <IconPlus /> Connect a feed
        </button>
      </div>

      {showForm && (
        <div className="card">
          <h3>Connect a calendar feed</h3>
          <p className="hint">
            In D2L, look for Calendar → Subscribe (or a gear icon on the calendar) to get your personal feed link — it'll start
            with <code>webcal://</code> or <code>https://</code>.
          </p>
          <div className="field-grid">
            <div>
              <label className="field-label">Name</label>
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="My D2L Calendar" />
            </div>
            <div>
              <label className="field-label">Feed URL</label>
              <input value={icsUrl} onChange={(e) => setIcsUrl(e.target.value)} placeholder="webcal://…" autoFocus />
            </div>
          </div>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn-ghost" onClick={() => setShowForm(false)}>
              Cancel
            </button>
            <button type="button" disabled={saving || !icsUrl.trim()} onClick={addFeed}>
              {saving ? "Connecting…" : "Connect"}
            </button>
          </div>
        </div>
      )}

      {feeds.length === 0 ? (
        <div className="empty-state">
          <IconCalendar width={32} height={32} />
          <p>No calendar feeds connected yet.</p>
        </div>
      ) : (
        feeds.map((feed) => (
          <div key={feed.id} className="card">
            <div className="card-header">
              <div className="card-header-title">
                <IconCalendar />
                <h3>{feed.name}</h3>
              </div>
              <button type="button" className="btn-secondary" disabled={syncing === feed.id} onClick={() => doSync(feed.id)}>
                <IconRefresh /> {syncing === feed.id ? "Syncing…" : "Sync now"}
              </button>
            </div>
            <p className="hint" style={{ marginTop: "-0.6rem" }}>
              {feed.last_synced_at ? `Last synced ${new Date(feed.last_synced_at).toLocaleString()}` : "Never synced yet"}
            </p>
            {feed.last_sync_error && <div className="error-banner">{feed.last_sync_error}</div>}

            {(batchesByFeed[feed.id] ?? []).length === 0 ? (
              <p className="hint">Nothing waiting on review from this feed.</p>
            ) : (
              batchesByFeed[feed.id].map(({ batch, extractions }) => (
                <div key={batch.id}>
                  {extractions.map((ex) => (
                    <div key={ex.id} className="extraction-card">
                      <div className="extraction-head">
                        <div>
                          <span className={`kind-pill kind-${ex.kind}`}>{ex.kind}</span> <span className="extraction-title">{ex.title}</span>
                        </div>
                        <span className="badge badge-warning">{Math.round(ex.confidence * 100)}% course match</span>
                      </div>
                      <div className="extraction-meta">
                        <span>{ex.due_date ?? "No date found"}</span>
                        {ex.due_time && <span>{ex.due_time}</span>}
                      </div>
                      <div className="row" style={{ margin: "0.4rem 0" }}>
                        <select
                          value={ex.course_id ?? ""}
                          onChange={(e) => setExtractionCourse(feed.id, ex.id, e.target.value)}
                          style={{ maxWidth: 260 }}
                        >
                          <option value="">Select a course…</option>
                          {courses.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name}
                            </option>
                          ))}
                        </select>
                        {ex.course_id && <span className="hint" style={{ margin: 0 }}>{courseName(ex.course_id)}</span>}
                      </div>
                      <div className="extraction-actions">
                        <button type="button" disabled={!ex.course_id} onClick={() => approve(feed.id, ex)}>
                          <IconCheck /> Approve
                        </button>
                        <button type="button" className="btn-danger" onClick={() => reject(feed.id, ex.id)}>
                          <IconX /> Reject
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              ))
            )}
          </div>
        ))
      )}

      {feeds.length > 0 && Object.values(batchesByFeed).every((b) => b.length === 0) && (
        <div className="empty-state">
          <IconInbox width={32} height={32} />
          <p>All caught up — nothing pending review.</p>
        </div>
      )}
    </div>
  );
}
