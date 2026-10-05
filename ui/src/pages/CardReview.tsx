import { useEffect, useState } from "react";
import invoke from "../api";
import type { CandidateEdits, CardCandidate, CardImport, ImportTextResult } from "../types";
import { IconCheck, IconSparkle, IconX } from "../icons";

interface Props {
  deckId: string;
  deckName: string;
  /** Called after anything that changes the deck's real cards. */
  onCardsChanged: () => void;
}

interface Draft {
  front: string;
  back: string;
  explanation: string;
}

/**
 * Generate cards from pasted material, then review them before any of them
 * becomes a real card.
 *
 * The review step is not ceremony: these are model-written cards about the
 * student's own material, and a wrong card studied repeatedly is worse than no
 * card. Every candidate shows the passage it came from, so checking it does not
 * mean trusting the model.
 */
export function CardReview({ deckId, deckName, onCardsChanged }: Props) {
  const [imports, setImports] = useState<CardImport[]>([]);
  const [candidates, setCandidates] = useState<Record<string, CardCandidate[]>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [material, setMaterial] = useState("");
  const [label, setLabel] = useState("");
  const [count, setCount] = useState(12);
  const [generating, setGenerating] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);

  const [pasteText, setPasteText] = useState("");
  const [pasteResult, setPasteResult] = useState<ImportTextResult | null>(null);
  const [importing, setImporting] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [prepareResult, setPrepareResult] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      const list = await invoke<CardImport[]>("list_imports", { deckId });
      setImports(list);
      // Only pull candidates for imports that still have something to review —
      // a finished import's rows are of no further interest.
      const pending = list.filter((i) => i.pending_count > 0);
      const loaded = await Promise.all(
        pending.map(async (i) => [i.id, await invoke<CardCandidate[]>("list_candidates", { importId: i.id })] as const),
      );
      setCandidates(Object.fromEntries(loaded.map(([id, cs]) => [id, cs.filter((c) => c.review_status === "pending")])));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deckId]);

  async function generate() {
    if (!material.trim()) return;
    setGenerating(true);
    setError(null);
    try {
      await invoke<CardImport>("generate_cards", {
        deckId,
        input: { material: material.trim(), source_label: label.trim() || null, count },
      });
      setMaterial("");
      setLabel("");
      await load();
    } catch (e) {
      setError(String(e));
      // Still reload: the failure is recorded on the import row, and showing
      // it beats showing nothing.
      await load();
    } finally {
      setGenerating(false);
    }
  }

  async function approve(candidate: CardCandidate, edits?: CandidateEdits) {
    setBusy(candidate.id);
    setError(null);
    try {
      await invoke("approve_candidate", { id: candidate.id, edits: edits ?? null });
      setEditingId(null);
      setDraft(null);
      await load();
      onCardsChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function reject(candidate: CardCandidate) {
    setBusy(candidate.id);
    setError(null);
    try {
      await invoke("reject_candidate", { id: candidate.id });
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function approveAll(importId: string) {
    setBusy(importId);
    setError(null);
    try {
      await invoke("approve_all_candidates", { importId });
      await load();
      onCardsChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runImport(dryRun: boolean) {
    if (!pasteText.trim()) return;
    setImporting(true);
    setError(null);
    try {
      const result = await invoke<ImportTextResult>("import_text", {
        deckId,
        input: { text: pasteText, dry_run: dryRun },
      });
      setPasteResult(result);
      if (!dryRun) {
        setPasteText("");
        onCardsChanged();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function prepareChoice() {
    setPreparing(true);
    setError(null);
    try {
      const r = await invoke<{ considered: number; prepared: number; skipped: number; total_cost_usd: number | null }>(
        "prepare_distractors",
        { deckId },
      );
      setPrepareResult(
        r.considered === 0
          ? "Every card already has options."
          : `${r.prepared} of ${r.considered} cards ready for multiple choice` +
              (r.skipped ? ` · ${r.skipped} skipped` : "") +
              (r.total_cost_usd != null ? ` · $${r.total_cost_usd.toFixed(4)}` : ""),
      );
      onCardsChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setPreparing(false);
    }
  }

  const reviewable = imports.filter((i) => i.pending_count > 0);
  const failed = imports.filter((i) => i.status === "failed");

  return (
    <div>
      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <IconSparkle />
            <h3>Generate cards from your notes</h3>
          </div>
        </div>
        <p className="hint" style={{ marginTop: "-0.6rem" }}>
          Paste lecture notes, a chapter summary, anything. Claude writes cards from <em>that material only</em>, and you
          review every one before it joins “{deckName}”.
        </p>
        <div className="field-grid">
          <div>
            <label className="field-label">What is this? (optional)</label>
            <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Chapter 4 lecture notes" />
          </div>
          <div>
            <label className="field-label">How many cards</label>
            <input
              type="number"
              min={1}
              max={40}
              value={count}
              onChange={(e) => setCount(Math.max(1, Math.min(40, Number(e.target.value) || 1)))}
            />
          </div>
        </div>
        <label className="field-label">Material</label>
        <textarea rows={8} value={material} onChange={(e) => setMaterial(e.target.value)} placeholder="Paste your notes here…" />
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <button type="button" disabled={generating || !material.trim()} onClick={generate}>
            <IconSparkle /> {generating ? "Writing cards… (up to 3 min)" : "Generate cards"}
          </button>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <h3>Already have a deck?</h3>
          </div>
        </div>
        <p className="hint" style={{ marginTop: "-0.6rem" }}>
          Paste a Quizlet or Anki export — one card per line, term and definition separated by a tab, comma or
          &ldquo; - &rdquo;. The separator is detected for you. No AI, no cost, and re-pasting a corrected export
          won&rsquo;t double the deck.
        </p>
        <textarea
          rows={5}
          value={pasteText}
          onChange={(e) => setPasteText(e.target.value)}
          placeholder="mole → 6.022e23 particles, one card per line (tab, comma or dash between the two sides)"
        />
        {pasteResult && (
          <p className="hint">
            Detected <strong>{pasteResult.separator}</strong> · {pasteResult.parsed} cards found
            {pasteResult.created > 0 ? ` · ${pasteResult.created} added` : ""}
            {pasteResult.duplicates > 0 ? ` · ${pasteResult.duplicates} already in this deck` : ""}
            {pasteResult.skipped_lines > 0 ? ` · ${pasteResult.skipped_lines} lines skipped` : ""}
          </p>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <button type="button" className="btn-secondary" disabled={importing || !pasteText.trim()} onClick={() => runImport(true)}>
            Preview
          </button>
          <button type="button" disabled={importing || !pasteText.trim()} onClick={() => runImport(false)}>
            {importing ? "Importing…" : "Import"}
          </button>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <h3>Multiple choice</h3>
          </div>
          <button type="button" className="btn-secondary" disabled={preparing} onClick={prepareChoice}>
            <IconSparkle /> {preparing ? "Writing options…" : "Prepare options"}
          </button>
        </div>
        <p className="hint" style={{ marginTop: "-0.6rem" }}>
          Writes three plausible wrong answers for each card, once, so answering during a review stays instant and
          free. Cards that already have options are left alone.
        </p>
        {prepareResult && <p className="hint">{prepareResult}</p>}
      </div>

      {error && <div className="error-banner">{error}</div>}

      {failed.map((imp) => (
        <div key={imp.id} className="card">
          <div className="extraction-head">
            <span className="extraction-title">{imp.source_label ?? "Generation"} failed</span>
            <span className="badge badge-danger">failed</span>
          </div>
          <p className="hint">{imp.error_message ?? "No reason recorded."}</p>
        </div>
      ))}

      {loading ? (
        <p className="hint">Loading…</p>
      ) : (
        reviewable.map((imp) => {
          const pending = candidates[imp.id] ?? [];
          return (
            <div key={imp.id} className="card">
              <div className="card-header">
                <div className="card-header-title">
                  <h3>{imp.source_label ?? "Generated cards"}</h3>
                </div>
                <button type="button" className="btn-secondary" disabled={busy === imp.id} onClick={() => approveAll(imp.id)}>
                  <IconCheck /> Approve all {pending.length}
                </button>
              </div>
              <p className="hint" style={{ marginTop: "-0.6rem" }}>
                {pending.length} to review
                {imp.approved_count > 0 ? ` · ${imp.approved_count} already added` : ""}
                {imp.total_cost_usd != null ? ` · cost $${imp.total_cost_usd.toFixed(4)}` : ""}
              </p>

              {pending.map((candidate) =>
                editingId === candidate.id && draft ? (
                  <div key={candidate.id} className="extraction-card">
                    <div className="field-grid">
                      <div>
                        <label className="field-label">Front</label>
                        <textarea rows={2} value={draft.front} onChange={(e) => setDraft({ ...draft, front: e.target.value })} />
                      </div>
                      <div>
                        <label className="field-label">Back</label>
                        <textarea rows={2} value={draft.back} onChange={(e) => setDraft({ ...draft, back: e.target.value })} />
                      </div>
                      <div>
                        <label className="field-label">Explanation</label>
                        <textarea
                          rows={2}
                          value={draft.explanation}
                          onChange={(e) => setDraft({ ...draft, explanation: e.target.value })}
                        />
                      </div>
                    </div>
                    <div className="extraction-actions">
                      <button
                        type="button"
                        disabled={!draft.front.trim() || !draft.back.trim()}
                        onClick={() =>
                          approve(candidate, {
                            front: draft.front.trim(),
                            back: draft.back.trim(),
                            explanation: draft.explanation.trim() || undefined,
                          })
                        }
                      >
                        <IconCheck /> Save and add
                      </button>
                      <button
                        type="button"
                        className="btn-ghost"
                        onClick={() => {
                          setEditingId(null);
                          setDraft(null);
                        }}
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                ) : (
                  <div key={candidate.id} className="extraction-card">
                    <div className="extraction-head">
                      <span className="extraction-title">{candidate.front}</span>
                    </div>
                    <div className="extraction-meta">
                      <span>{candidate.back}</span>
                    </div>
                    {candidate.explanation && (
                      <p className="hint" style={{ margin: "0.4rem 0 0" }}>{candidate.explanation}</p>
                    )}
                    {candidate.tags.length > 0 && (
                      <div className="row" style={{ margin: "0.4rem 0 0", gap: 4 }}>
                        {candidate.tags.map((tag) => (
                          <span key={tag} className="badge badge-neutral">
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                    {candidate.source_excerpt && (
                      <p className="extraction-excerpt" title="The passage this card came from">
                        “{candidate.source_excerpt}”
                      </p>
                    )}
                    <div className="extraction-actions">
                      <button type="button" disabled={busy === candidate.id} onClick={() => approve(candidate)}>
                        <IconCheck /> Add
                      </button>
                      <button
                        type="button"
                        className="btn-secondary"
                        onClick={() => {
                          setEditingId(candidate.id);
                          setDraft({
                            front: candidate.front,
                            back: candidate.back,
                            explanation: candidate.explanation ?? "",
                          });
                        }}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        className="btn-danger"
                        disabled={busy === candidate.id}
                        onClick={() => reject(candidate)}
                      >
                        <IconX /> Discard
                      </button>
                    </div>
                  </div>
                ),
              )}
            </div>
          );
        })
      )}
    </div>
  );
}
