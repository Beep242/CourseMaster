import { useEffect, useState } from "react";
import invoke from "../api";
import type { CardSearchHit, Course, Deck } from "../types";
import { IconChevronRight, IconInbox, IconPlus, IconSearch, IconX } from "../icons";

interface Props {
  onOpenDeck: (id: string) => void;
}

const SWATCHES = ["#8b5cf6", "#d946ef", "#34d399", "#fbbf24", "#38bdf8", "#fb7185"];

export function Decks({ onOpenDeck }: Props) {
  const [decks, setDecks] = useState<Deck[]>([]);
  const [courses, setCourses] = useState<Course[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [showForm, setShowForm] = useState(false);
  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const [courseId, setCourseId] = useState("");
  const [color, setColor] = useState(SWATCHES[0]);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<CardSearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [d, c] = await Promise.all([
        invoke<Deck[]>("list_decks", { courseId: null }),
        invoke<Course[]>("list_courses", { semesterId: null }),
      ]);
      setDecks(d);
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

  async function createDeck() {
    if (!name.trim()) return;
    setSaving(true);
    setError(null);
    try {
      // An empty course selection means "not filed yet", which the API models
      // as a null course_id rather than refusing the deck.
      await invoke("create_deck", { input: { name: name.trim(), course_id: courseId || null, color } });
      setName("");
      setShowForm(false);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function removeDeck(deck: Deck) {
    const count = deck.card_count === 1 ? "1 card" : `${deck.card_count} cards`;
    if (!window.confirm(`Delete "${deck.name}" and its ${count}? This can't be undone.`)) return;
    setError(null);
    try {
      await invoke("delete_deck", { id: deck.id });
      await load();
    } catch (e) {
      setError(String(e));
    }
  }

  async function runSearch(e: React.FormEvent) {
    e.preventDefault();
    if (!query.trim()) {
      setHits(null);
      return;
    }
    setSearching(true);
    setError(null);
    try {
      setHits(await invoke<CardSearchHit[]>("search_cards", { query: query.trim() }));
    } catch (err) {
      setError(String(err));
    } finally {
      setSearching(false);
    }
  }

  function clearSearch() {
    setQuery("");
    setHits(null);
  }

  const courseName = (id: string | null) => (id ? courses.find((c) => c.id === id)?.name : undefined);

  return (
    <div>
      <p className="hint">Decks of flashcards, grouped by course. A deck doesn&rsquo;t need a course — unfiled is fine.</p>
      {error && <div className="error-banner">{error}</div>}

      <form className="row" style={{ gap: 8 }} onSubmit={runSearch}>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search every card in every deck…"
          aria-label="Search cards"
        />
        <button type="submit" className="btn-secondary" disabled={searching}>
          <IconSearch /> {searching ? "Searching…" : "Search"}
        </button>
        {hits !== null && (
          <button type="button" className="btn-ghost" onClick={clearSearch}>
            Clear
          </button>
        )}
      </form>

      {hits !== null && (
        <div className="card">
          <div className="card-header">
            <div className="card-header-title">
              <IconSearch />
              <h3>
                {hits.length} {hits.length === 1 ? "match" : "matches"}
              </h3>
            </div>
          </div>
          {hits.length === 0 ? (
            <p className="hint">Nothing matched “{query}”.</p>
          ) : (
            hits.map((hit) => (
              <div key={hit.id} className="extraction-card" role="button" onClick={() => onOpenDeck(hit.deck_id)}>
                <div className="extraction-head">
                  <span className="extraction-title">{hit.front}</span>
                  <span className="badge badge-neutral">
                    {hit.deck_name}
                    {hit.course_name ? ` · ${hit.course_name}` : ""}
                  </span>
                </div>
                <div className="extraction-meta">
                  <span>{hit.back}</span>
                </div>
              </div>
            ))
          )}
        </div>
      )}

      <div className="row" style={{ justifyContent: "flex-end" }}>
        <button type="button" onClick={() => setShowForm((v) => !v)}>
          <IconPlus /> New deck
        </button>
      </div>

      {showForm && (
        <div className="card">
          <h3>Add a deck</h3>
          <div className="field-grid">
            <div>
              <label className="field-label">Deck name</label>
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Chapter 4 — acids and bases" autoFocus />
            </div>
            <div>
              <label className="field-label">Course</label>
              <select value={courseId} onChange={(e) => setCourseId(e.target.value)}>
                <option value="">No course yet</option>
                {courses.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="field-label">Colour</label>
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
            <button type="button" disabled={saving || !name.trim()} onClick={createDeck}>
              {saving ? "Adding…" : "Add deck"}
            </button>
          </div>
        </div>
      )}

      {loading ? (
        <p className="hint">Loading…</p>
      ) : decks.length === 0 ? (
        <div className="empty-state">
          <IconInbox width={32} height={32} />
          <p>No decks yet. Add one above, then start adding cards.</p>
        </div>
      ) : (
        <div className="course-grid">
          {decks.map((deck) => (
            <div key={deck.id} className="card course-card" onClick={() => onOpenDeck(deck.id)}>
              <div className="course-card-title">
                <span className="course-color-dot" style={{ background: deck.color }} />
                {deck.name}
                <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 4 }}>
                  <button
                    type="button"
                    className="btn-ghost"
                    style={{ padding: "0.2em" }}
                    title="Delete deck"
                    onClick={(e) => {
                      e.stopPropagation();
                      removeDeck(deck);
                    }}
                  >
                    <IconX width={15} height={15} />
                  </button>
                  <IconChevronRight />
                </span>
              </div>
              <div className="course-card-meta">
                {deck.card_count === 1 ? "1 card" : `${deck.card_count} cards`}
                {courseName(deck.course_id) ? ` · ${courseName(deck.course_id)}` : " · unfiled"}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
