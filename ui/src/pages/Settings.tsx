import { useEffect, useState } from "react";
import invoke from "../api";
import type { UserProfile } from "../types";
import { IconAlert, IconCheck } from "../icons";

export function Settings() {
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [aiOnline, setAiOnline] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setError(null);
    try {
      const [p, ai] = await Promise.all([invoke<UserProfile | null>("get_profile"), invoke<boolean>("ai_status")]);
      setProfile(p);
      setAiOnline(ai);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function save() {
    if (!profile) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("save_profile", { profileInput: profile });
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  if (!profile) return <p className="hint">Loading…</p>;

  return (
    <div>
      {error && <div className="error-banner">{error}</div>}

      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            {aiOnline ? <IconCheck /> : <IconAlert />}
            <h3>AI engine</h3>
          </div>
          <span className={`badge ${aiOnline ? "badge-success" : "badge-danger"}`}>
            {aiOnline === null ? "Checking…" : aiOnline ? "Claude Code found" : "Not available"}
          </span>
        </div>
        <p className="hint">
          CourseMaster's server uses a signed-in Claude Code CLI for every AI feature — syllabus extraction, tutoring, study
          guides — instead of a separate API key. This only confirms the CLI is installed; if a feature still fails with
          "Not logged in", the server needs <code>claude setup-token</code> run once.
        </p>
      </div>

      <div className="card">
        <h3>Profile</h3>
        <div className="field-grid">
          <div>
            <label className="field-label">Name</label>
            <input value={profile.name} onChange={(e) => setProfile({ ...profile, name: e.target.value })} />
          </div>
          <div>
            <label className="field-label">University</label>
            <input value={profile.university ?? ""} onChange={(e) => setProfile({ ...profile, university: e.target.value || null })} />
          </div>
          <div>
            <label className="field-label">Major</label>
            <input value={profile.major ?? ""} onChange={(e) => setProfile({ ...profile, major: e.target.value || null })} />
          </div>
        </div>
        <label className="field-label">Goals</label>
        <textarea rows={2} value={profile.goals ?? ""} onChange={(e) => setProfile({ ...profile, goals: e.target.value || null })} />
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <button type="button" disabled={saving} onClick={save}>
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </div>
    </div>
  );
}
