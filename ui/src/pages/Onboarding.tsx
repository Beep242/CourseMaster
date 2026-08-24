import { useState } from "react";
import invoke from "../api";
import type { UserProfile } from "../types";
import { IconBook, IconChevronRight } from "../icons";

interface Props {
  onComplete: () => void;
}

const STEPS = ["About you", "This semester", "Study rhythm"] as const;

export function Onboarding({ onComplete }: Props) {
  const [step, setStep] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [university, setUniversity] = useState("");
  const [major, setMajor] = useState("");
  const [goals, setGoals] = useState("");
  const [semesterName, setSemesterName] = useState("");
  const [preferredTime, setPreferredTime] = useState("evening");
  const [wakeTime, setWakeTime] = useState("08:00");
  const [sleepTime, setSleepTime] = useState("00:00");

  const canAdvance = step === 0 ? name.trim().length > 0 : step === 1 ? semesterName.trim().length > 0 : true;

  async function finish() {
    setSaving(true);
    setError(null);
    try {
      const profile: UserProfile = {
        name: name.trim(),
        university: university.trim() || null,
        major: major.trim() || null,
        weekly_availability: {},
        preferred_study_times: [preferredTime],
        sleep_schedule: { wake: wakeTime, sleep: sleepTime },
        goals: goals.trim() || null,
        onboarding_complete: true,
      };
      await invoke("save_profile", { profileInput: profile });
      await invoke("create_semester", { input: { name: semesterName.trim(), start_date: null, end_date: null } });
      onComplete();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="onboarding-shell">
      <div className="card onboarding-card">
        <div className="card-header">
          <div className="card-header-title">
            <IconBook />
            <h3>Welcome to CourseMaster</h3>
          </div>
        </div>
        <div className="step-dots">
          {STEPS.map((label, i) => (
            <span key={label} className={`step-dot ${i < step ? "done" : i === step ? "active" : ""}`} />
          ))}
        </div>
        <p className="hint">
          {step + 1}. {STEPS[step]}
        </p>

        {error && <div className="error-banner">{error}</div>}

        {step === 0 && (
          <>
            <label className="field-label">Your name</label>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Alex Rivera" autoFocus />
            <div className="field-grid">
              <div>
                <label className="field-label">University</label>
                <input value={university} onChange={(e) => setUniversity(e.target.value)} placeholder="Slippery Rock University" />
              </div>
              <div>
                <label className="field-label">Major</label>
                <input value={major} onChange={(e) => setMajor(e.target.value)} placeholder="Computer Science" />
              </div>
            </div>
            <label className="field-label">What do you want to get out of this semester?</label>
            <textarea rows={2} value={goals} onChange={(e) => setGoals(e.target.value)} placeholder="e.g. keep my GPA above 3.5, actually stay ahead on CS301" />
          </>
        )}

        {step === 1 && (
          <>
            <label className="field-label">What's this semester called?</label>
            <input value={semesterName} onChange={(e) => setSemesterName(e.target.value)} placeholder="Fall 2026" autoFocus />
            <p className="hint">You'll add your courses next, from the dashboard.</p>
          </>
        )}

        {step === 2 && (
          <>
            <label className="field-label">When do you focus best?</label>
            <select value={preferredTime} onChange={(e) => setPreferredTime(e.target.value)}>
              <option value="morning">Morning</option>
              <option value="afternoon">Afternoon</option>
              <option value="evening">Evening</option>
              <option value="late_night">Late night</option>
            </select>
            <div className="field-grid">
              <div>
                <label className="field-label">Wake time</label>
                <input type="time" value={wakeTime} onChange={(e) => setWakeTime(e.target.value)} />
              </div>
              <div>
                <label className="field-label">Sleep time</label>
                <input type="time" value={sleepTime} onChange={(e) => setSleepTime(e.target.value)} />
              </div>
            </div>
          </>
        )}

        <div className="row" style={{ justifyContent: "space-between", marginTop: "1.4rem" }}>
          <button type="button" className="btn-ghost" disabled={step === 0} onClick={() => setStep((s) => s - 1)}>
            Back
          </button>
          {step < STEPS.length - 1 ? (
            <button type="button" disabled={!canAdvance} onClick={() => setStep((s) => s + 1)}>
              Continue <IconChevronRight />
            </button>
          ) : (
            <button type="button" disabled={saving} onClick={finish}>
              {saving ? "Setting up…" : "Start using CourseMaster"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
