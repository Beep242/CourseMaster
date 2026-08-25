import { useEffect, useState } from "react";
import invoke from "../api";
import type {
  PracticeAttempt,
  PracticeDifficulty,
  PracticeTest,
  PracticeTestSummary,
  StudyGuide,
  StudyGuideKind,
} from "../types";
import { IconDoc, IconSparkle } from "../icons";

interface Props {
  courseId: string;
}

const GUIDE_KINDS: { value: StudyGuideKind; label: string }[] = [
  { value: "quick_review", label: "Quick Review" },
  { value: "complete", label: "Complete Guide" },
  { value: "cram_sheet", label: "Exam Cram Sheet" },
  { value: "formula_sheet", label: "Formula Sheet" },
];

const DIFFICULTIES: { value: PracticeDifficulty; label: string }[] = [
  { value: "easy", label: "Easy" },
  { value: "medium", label: "Medium" },
  { value: "hard", label: "Hard" },
  { value: "exam_simulation", label: "Exam simulation" },
];

function renderInline(text: string, keyPrefix: string) {
  return text.split(/(\*\*[^*]+\*\*)/g).map((part, i) =>
    part.startsWith("**") && part.endsWith("**") ? (
      <strong key={`${keyPrefix}-${i}`}>{part.slice(2, -2)}</strong>
    ) : (
      <span key={`${keyPrefix}-${i}`}>{part}</span>
    ),
  );
}

/// Dependency-free Markdown-ish rendering for AI-generated study content —
/// headings, bullet lists, bold text, paragraphs. Covers what the study
/// guide prompts ask the model to produce; not a general Markdown parser.
function SimpleMarkdown({ content }: { content: string }) {
  const lines = content.split("\n");
  const blocks: React.ReactNode[] = [];
  let listBuffer: string[] = [];
  let key = 0;

  function flushList() {
    if (listBuffer.length === 0) return;
    const items = listBuffer;
    blocks.push(
      <ul key={`ul-${key++}`}>
        {items.map((item, i) => (
          <li key={i}>{renderInline(item, `li-${key}-${i}`)}</li>
        ))}
      </ul>,
    );
    listBuffer = [];
  }

  for (const rawLine of lines) {
    const line = rawLine.trimEnd();
    if (/^\s*[-*]\s+/.test(line)) {
      listBuffer.push(line.replace(/^\s*[-*]\s+/, ""));
      continue;
    }
    flushList();
    if (line.startsWith("### ")) {
      blocks.push(<h4 key={key++}>{renderInline(line.slice(4), `h-${key}`)}</h4>);
    } else if (line.startsWith("## ")) {
      blocks.push(<h3 key={key++}>{renderInline(line.slice(3), `h-${key}`)}</h3>);
    } else if (line.startsWith("# ")) {
      blocks.push(<h2 key={key++}>{renderInline(line.slice(2), `h-${key}`)}</h2>);
    } else if (line.trim() !== "") {
      blocks.push(<p key={key++}>{renderInline(line, `p-${key}`)}</p>);
    }
  }
  flushList();
  return <div className="markdown-content">{blocks}</div>;
}

export function Study({ courseId }: Props) {
  const [guides, setGuides] = useState<StudyGuide[]>([]);
  const [tests, setTests] = useState<PracticeTestSummary[]>([]);
  const [openGuide, setOpenGuide] = useState<StudyGuide | null>(null);
  const [openTest, setOpenTest] = useState<PracticeTest | null>(null);
  const [attempt, setAttempt] = useState<PracticeAttempt | null>(null);
  const [responses, setResponses] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [material, setMaterial] = useState("");
  const [generatingGuide, setGeneratingGuide] = useState<StudyGuideKind | null>(null);
  const [testDifficulty, setTestDifficulty] = useState<PracticeDifficulty>("medium");
  const [questionCount, setQuestionCount] = useState(8);
  const [generatingTest, setGeneratingTest] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [guideList, testList] = await Promise.all([
        invoke<StudyGuide[]>("list_study_guides", { courseId }),
        invoke<PracticeTestSummary[]>("list_practice_tests", { courseId }),
      ]);
      setGuides(guideList);
      setTests(testList);
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

  async function createGuide(kind: StudyGuideKind) {
    setGeneratingGuide(kind);
    setError(null);
    try {
      const guide = await invoke<StudyGuide>("generate_study_guide", { courseId, input: { kind, material: material.trim() || null } });
      await load();
      setOpenGuide(guide);
    } catch (e) {
      setError(String(e));
    } finally {
      setGeneratingGuide(null);
    }
  }

  function openTestForTaking(test: PracticeTest) {
    setOpenTest(test);
    setAttempt(null);
    setResponses({});
  }

  async function createTest() {
    setGeneratingTest(true);
    setError(null);
    try {
      const test = await invoke<PracticeTest>("generate_practice_test", {
        courseId,
        input: { difficulty: testDifficulty, question_count: questionCount, material: material.trim() || null },
      });
      await load();
      openTestForTaking(test);
    } catch (e) {
      setError(String(e));
    } finally {
      setGeneratingTest(false);
    }
  }

  async function openExistingTest(summary: PracticeTestSummary) {
    setError(null);
    try {
      const test = await invoke<PracticeTest>("get_practice_test", { id: summary.id });
      openTestForTaking(test);
    } catch (e) {
      setError(String(e));
    }
  }

  async function openExistingGuide(id: string) {
    setError(null);
    try {
      const guide = await invoke<StudyGuide>("get_study_guide", { id });
      setOpenGuide(guide);
    } catch (e) {
      setError(String(e));
    }
  }

  async function submitTest() {
    if (!openTest) return;
    setSubmitting(true);
    setError(null);
    try {
      const answers = openTest.questions.map((q) => ({ question_id: q.id, response: responses[q.id] ?? "" }));
      const result = await invoke<PracticeAttempt>("submit_attempt", { id: openTest.id, answers });
      setAttempt(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  }

  if (loading) return <p className="hint">Loading…</p>;

  if (openGuide) {
    return (
      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <IconDoc />
            <h3>{openGuide.title}</h3>
          </div>
          <button type="button" className="btn-ghost" onClick={() => setOpenGuide(null)}>
            Back
          </button>
        </div>
        <SimpleMarkdown content={openGuide.content} />
      </div>
    );
  }

  if (openTest) {
    return (
      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <IconSparkle />
            <h3>{openTest.title}</h3>
          </div>
          <button
            type="button"
            className="btn-ghost"
            onClick={() => {
              setOpenTest(null);
              setAttempt(null);
            }}
          >
            Back
          </button>
        </div>
        {error && <div className="error-banner">{error}</div>}

        {attempt ? (
          <div>
            <p className="stat-value" style={{ marginBottom: "1rem" }}>
              {Math.round(attempt.score_percentage)}%
            </p>
            {attempt.answers.map((a) => (
              <div key={a.question_id} className="extraction-card">
                <div className="extraction-head">
                  <span className="extraction-title">{a.question_text}</span>
                  <span className={`badge ${a.is_correct ? "badge-success" : "badge-danger"}`}>{a.is_correct ? "Correct" : "Incorrect"}</span>
                </div>
                <p className="hint">Your answer: {a.submitted || "(blank)"}</p>
                {!a.is_correct && <p className="hint">Correct answer: {a.correct_answer}</p>}
                {a.feedback && <p className="extraction-excerpt">{a.feedback}</p>}
              </div>
            ))}
          </div>
        ) : (
          <div>
            {openTest.questions.map((q, i) => (
              <div key={q.id} className="extraction-card">
                <p className="extraction-title">
                  {i + 1}. {q.question_text}
                </p>
                {q.kind === "multiple_choice" && q.options ? (
                  <div>
                    {q.options.map((opt) => (
                      <label key={opt} className="toggle-item" style={{ marginBottom: "0.3rem" }}>
                        <input
                          type="radio"
                          name={q.id}
                          checked={responses[q.id] === opt}
                          onChange={() => setResponses((r) => ({ ...r, [q.id]: opt }))}
                        />
                        <span>{opt}</span>
                      </label>
                    ))}
                  </div>
                ) : q.kind === "true_false" ? (
                  <div className="row">
                    {["True", "False"].map((opt) => (
                      <label key={opt} className="toggle-item">
                        <input
                          type="radio"
                          name={q.id}
                          checked={responses[q.id] === opt}
                          onChange={() => setResponses((r) => ({ ...r, [q.id]: opt }))}
                        />
                        <span>{opt}</span>
                      </label>
                    ))}
                  </div>
                ) : (
                  <textarea rows={2} value={responses[q.id] ?? ""} onChange={(e) => setResponses((r) => ({ ...r, [q.id]: e.target.value }))} />
                )}
              </div>
            ))}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <button type="button" disabled={submitting} onClick={submitTest}>
                {submitting ? "Grading…" : "Submit"}
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div>
      {error && <div className="error-banner">{error}</div>}

      <div className="card">
        <h3>Focus material (optional)</h3>
        <p className="hint">Paste lecture notes or textbook excerpts to ground generated content in your actual course material.</p>
        <textarea rows={4} value={material} onChange={(e) => setMaterial(e.target.value)} placeholder="Paste notes here…" />
      </div>

      <div className="card">
        <h3>Study guides</h3>
        <div className="row">
          {GUIDE_KINDS.map((k) => (
            <button
              key={k.value}
              type="button"
              className="btn-secondary"
              disabled={generatingGuide === k.value}
              onClick={() => createGuide(k.value)}
            >
              {generatingGuide === k.value ? "Writing…" : k.label}
            </button>
          ))}
        </div>
        {guides.length === 0 ? (
          <p className="hint">No study guides yet.</p>
        ) : (
          <table>
            <tbody>
              {guides.map((g) => (
                <tr key={g.id} onClick={() => openExistingGuide(g.id)} style={{ cursor: "pointer" }}>
                  <td>{g.title}</td>
                  <td className="num">{new Date(g.created_at).toLocaleDateString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="card">
        <h3>Practice tests</h3>
        <div className="field-grid">
          <div>
            <label className="field-label">Difficulty</label>
            <select value={testDifficulty} onChange={(e) => setTestDifficulty(e.target.value as PracticeDifficulty)}>
              {DIFFICULTIES.map((d) => (
                <option key={d.value} value={d.value}>
                  {d.label}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="field-label">Questions</label>
            <input type="number" min={3} max={20} value={questionCount} onChange={(e) => setQuestionCount(Number(e.target.value))} />
          </div>
        </div>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <button type="button" disabled={generatingTest} onClick={createTest}>
            {generatingTest ? "Writing…" : "Generate practice test"}
          </button>
        </div>
        {tests.length === 0 ? (
          <p className="hint">No practice tests yet.</p>
        ) : (
          <table>
            <tbody>
              {tests.map((t) => (
                <tr key={t.id} onClick={() => openExistingTest(t)} style={{ cursor: "pointer" }}>
                  <td>{t.title}</td>
                  <td className="num">{new Date(t.created_at).toLocaleDateString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
