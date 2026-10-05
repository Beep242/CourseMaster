import { useEffect, useState } from "react";
import invoke from "../api";
import type { Card, CardUpdate, Course, Deck } from "../types";
import { IconCheck, IconInbox, IconPencil, IconPlus, IconX } from "../icons";

interface Props {
  deckId: string;
  onBack: () => void;
}

interface Draft {
  front: string;
  back: string;
  explanation: string;
  tags: string;
}

function draftFrom(card: Card): Draft {
  return {
    front: card.front,
    back: card.back,
    explanation: card.explanation ?? "",
    tags: card.tags.join(", "),
  };
}

function parseTags(raw: string): string[] {
  return raw
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
}

export function DeckDetail({ deckId, onBack }: Props) {
  const [deck, setDeck] = useState<Deck | null>(null);
  const [cards, setCards] = useState<Card[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [adding, setAdding] = useState(false);
  const [newFront, setNewFront] = useState("");
  const [newBack, setNewBack] = useState("");
  const [saving, setSaving] = useState(false);

  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [d, c, courseList] = await Promise.all([
        invoke<Deck>("get_deck", { id: deckId }),
        invoke<Card[]>("list_cards", { deckId }),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setDeck(d);
      setCards(c);
      setCourses(courseList);
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

  async function addCard() {
    if (!newFront.trim() || !newBack.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("create_card", { deckId, input: { front: newFront.trim(), back: newBack.trim() } });
      setNewFront("");
      setNewBack("");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  function startEdit(card: Card) {
    setEditingId(card.id);
    setDraft(draftFrom(card));
  }

  function cancelEdit() {
    setEditingId(null);
    setDraft(null);
  }

  async function saveEdit(card: Card) {
    if (!draft) return;
    setError(null);
    try {
      // `null` here is meaningful, not a missing value: the API clears a field
      // on an explicit null and leaves it alone when the key is absent, which
      // is what lets a bad generated explanation actually be deleted.
      const patch: CardUpdate = {
        front: draft.front.trim(),
        back: draft.back.trim(),
        explanation: draft.explanation.trim() ? draft.explanation.trim() : null,
        tags: parseTags(draft.tags).length ? parseTags(draft.tags) : null,
      };
      await invoke("update_card", { id: card.id, patch });
      cancelEdit();
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeCard(card: Card) {
    if (!window.confirm(`Delete this card?\n\n${card.front}`)) return;
    setError(null);
    try {
      await invoke("delete_card", { id: card.id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function moveDeckToCourse(courseId: string) {
    setError(null);
    try {
      await invoke("update_deck", { id: deckId, patch: { course_id: courseId || null } });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  if (loading) return <p className="hint">Loading…</p>;
  if (!deck) {
    return (
      <div>
        {error && <div className="error-banner">{error}</div>}
        <div className="empty-state">
          <IconInbox width={32} height={32} />
          <p>That deck no longer exists.</p>
          <button type="button" className="btn-secondary" onClick={onBack}>
            Back to decks
          </button>
        </div>
      </div>
    );
  }

  return (
    <div>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <button type="button" className="btn-ghost" onClick={onBack}>
          ← All decks
        </button>
        <span className="hint" style={{ margin: 0 }}>
          {cards.length === 1 ? "1 card" : `${cards.length} cards`}
        </span>
      </div>

      <div className="card">
        <div className="card-header">
          <div className="card-header-title">
            <span className="course-color-dot" style={{ background: deck.color }} />
            <h3>{deck.name}</h3>
          </div>
        </div>
        {deck.description && <p className="hint" style={{ marginTop: "-0.6rem" }}>{deck.description}</p>}
        <label className="field-label">Course</label>
        <select value={deck.course_id ?? ""} onChange={(e) => moveDeckToCourse(e.target.value)} style={{ maxWidth: 280 }}>
          <option value="">No course yet</option>
          {courses.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      {error && <div className="error-banner">{error}</div>}

      <div className="row" style={{ justifyContent: "flex-end" }}>
        <button type="button" onClick={() => setAdding((v) => !v)}>
          <IconPlus /> New card
        </button>
      </div>

      {adding && (
        <div className="card">
          <h3>Add a card</h3>
          <div className="field-grid">
            <div>
              <label className="field-label">Front (the prompt)</label>
              <textarea rows={3} value={newFront} onChange={(e) => setNewFront(e.target.value)} autoFocus />
            </div>
            <div>
              <label className="field-label">Back (the answer)</label>
              <textarea rows={3} value={newBack} onChange={(e) => setNewBack(e.target.value)} />
            </div>
          </div>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn-ghost" onClick={() => setAdding(false)}>
              Cancel
            </button>
            <button type="button" disabled={saving || !newFront.trim() || !newBack.trim()} onClick={addCard}>
              {saving ? "Adding…" : "Add card"}
            </button>
          </div>
        </div>
      )}

      {cards.length === 0 ? (
        <div className="empty-state">
          <IconInbox width={32} height={32} />
          <p>No cards in this deck yet.</p>
        </div>
      ) : (
        cards.map((card) =>
          editingId === card.id && draft ? (
            <div key={card.id} className="card">
              <div className="field-grid">
                <div>
                  <label className="field-label">Front</label>
                  <textarea rows={3} value={draft.front} onChange={(e) => setDraft({ ...draft, front: e.target.value })} />
                </div>
                <div>
                  <label className="field-label">Back</label>
                  <textarea rows={3} value={draft.back} onChange={(e) => setDraft({ ...draft, back: e.target.value })} />
                </div>
                <div>
                  <label className="field-label">Explanation (blank to remove)</label>
                  <textarea
                    rows={2}
                    value={draft.explanation}
                    onChange={(e) => setDraft({ ...draft, explanation: e.target.value })}
                  />
                </div>
                <div>
                  <label className="field-label">Tags, comma separated</label>
                  <input value={draft.tags} onChange={(e) => setDraft({ ...draft, tags: e.target.value })} />
                </div>
              </div>
              <div className="extraction-actions">
                <button type="button" disabled={!draft.front.trim() || !draft.back.trim()} onClick={() => saveEdit(card)}>
                  <IconCheck /> Save
                </button>
                <button type="button" className="btn-ghost" onClick={cancelEdit}>
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <div key={card.id} className="extraction-card">
              <div className="extraction-head">
                <span className="extraction-title">{card.front}</span>
                {card.kind !== "basic" && <span className="badge badge-neutral">{card.kind.replace("_", " ")}</span>}
              </div>
              <div className="extraction-meta">
                <span>{card.back}</span>
              </div>
              {card.explanation && <p className="hint" style={{ margin: "0.4rem 0 0" }}>{card.explanation}</p>}
              {card.tags.length > 0 && (
                <div className="row" style={{ margin: "0.4rem 0 0", gap: 4 }}>
                  {card.tags.map((tag) => (
                    <span key={tag} className="badge badge-neutral">
                      {tag}
                    </span>
                  ))}
                </div>
              )}
              {card.source_excerpt && (
                <p className="extraction-excerpt" title="Where this card came from">
                  {card.source_excerpt}
                </p>
              )}
              <div className="extraction-actions">
                <button type="button" className="btn-secondary" onClick={() => startEdit(card)}>
                  <IconPencil /> Edit
                </button>
                <button type="button" className="btn-danger" onClick={() => removeCard(card)}>
                  <IconX /> Delete
                </button>
              </div>
            </div>
          ),
        )
      )}
    </div>
  );
}
