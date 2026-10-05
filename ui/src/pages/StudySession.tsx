import { useCallback, useEffect, useRef, useState } from "react";
import invoke from "../api";
import type { Card, Deck } from "../types";
import { IconChevronRight, IconInbox, IconRefresh } from "../icons";

interface Props {
  deckId: string;
  onExit: () => void;
}

/**
 * Fisher-Yates, seeded by nothing in particular — shuffling is a convenience,
 * not something that needs to be reproducible. Returns a new array so the
 * caller's card list is never mutated underneath React.
 */
function shuffled<T>(items: T[]): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/**
 * A plain flip-through, deliberately with no scheduling in it.
 *
 * "Study this deck tonight" needs cards and a way to go through them, which is
 * why this lands before spaced repetition rather than after: the grade buttons
 * and the due-today queue build on top of this screen, they are not a
 * prerequisite for it.
 */
export function StudySession({ deckId, onExit }: Props) {
  const [deck, setDeck] = useState<Deck | null>(null);
  const [cards, setCards] = useState<Card[]>([]);
  const [index, setIndex] = useState(0);
  const [flipped, setFlipped] = useState(false);
  const [seen, setSeen] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const touchStartX = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      setError(null);
      try {
        const [d, c] = await Promise.all([
          invoke<Deck>("get_deck", { id: deckId }),
          invoke<Card[]>("list_cards", { deckId }),
        ]);
        if (cancelled) return;
        setDeck(d);
        setCards(c);
        setIndex(0);
        setFlipped(false);
        setSeen(c.length > 0 ? 1 : 0);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    // Guards against a stale response from a previous deck landing after a
    // faster one for the deck now on screen.
    return () => {
      cancelled = true;
    };
  }, [deckId]);

  const total = cards.length;

  const go = useCallback(
    (delta: number) => {
      setIndex((current) => {
        const next = Math.min(Math.max(current + delta, 0), Math.max(total - 1, 0));
        if (next !== current) {
          setFlipped(false);
          // Highest card reached, so going back and forth does not inflate it.
          setSeen((s) => Math.max(s, next + 1));
        }
        return next;
      });
    },
    [total],
  );

  const restart = useCallback((shuffle: boolean) => {
    setCards((current) => (shuffle ? shuffled(current) : current));
    setIndex(0);
    setFlipped(false);
    setSeen(1);
  }, []);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === " " || e.key === "Enter") {
        // Space scrolls the page by default, which is the opposite of useful
        // when it is also the flip key.
        e.preventDefault();
        setFlipped((f) => !f);
      } else if (e.key === "ArrowRight") {
        go(1);
      } else if (e.key === "ArrowLeft") {
        go(-1);
      } else if (e.key === "Escape") {
        onExit();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, onExit]);

  function onTouchStart(e: React.TouchEvent) {
    touchStartX.current = e.changedTouches[0]?.clientX ?? null;
  }

  function onTouchEnd(e: React.TouchEvent) {
    const start = touchStartX.current;
    touchStartX.current = null;
    if (start === null) return;
    const dx = (e.changedTouches[0]?.clientX ?? start) - start;
    // Below this a swipe is indistinguishable from a tap that moved slightly,
    // and stealing those would make the card impossible to flip on a phone.
    if (Math.abs(dx) < 48) return;
    go(dx < 0 ? 1 : -1);
  }

  if (loading) return <p className="hint">Loading…</p>;

  if (error) {
    return (
      <div>
        <div className="error-banner">{error}</div>
        <button type="button" className="btn-secondary" onClick={onExit}>
          Back to deck
        </button>
      </div>
    );
  }

  if (total === 0) {
    return (
      <div className="empty-state">
        <IconInbox width={32} height={32} />
        <p>This deck has no cards to study yet.</p>
        <button type="button" className="btn-secondary" onClick={onExit}>
          Back to deck
        </button>
      </div>
    );
  }

  const card = cards[index];
  const atEnd = index === total - 1;

  return (
    <div className="study-stage">
      <div className="row" style={{ justifyContent: "space-between", width: "100%", maxWidth: 640 }}>
        <button type="button" className="btn-ghost" onClick={onExit}>
          ← {deck?.name ?? "Deck"}
        </button>
        <span className="hint" style={{ margin: 0 }}>
          {index + 1} / {total}
        </span>
      </div>

      <div className="study-progress">
        <div className="progress-track">
          <div className="progress-fill" style={{ width: `${((index + 1) / total) * 100}%` }} />
        </div>
        <span>{seen} seen</span>
      </div>

      <button
        type="button"
        className={`flashcard ${flipped ? "flipped" : ""}`}
        onClick={() => setFlipped((f) => !f)}
        onTouchStart={onTouchStart}
        onTouchEnd={onTouchEnd}
        aria-label={flipped ? "Showing the answer. Activate to show the question." : "Showing the question. Activate to reveal the answer."}
      >
        <div className="flashcard-inner">
          <div className="flashcard-face">
            <div className="flashcard-text">{card.front}</div>
            <span className="flashcard-hint">Tap, or press Space, to flip</span>
          </div>
          <div className="flashcard-face back">
            <div className="flashcard-text">{card.back}</div>
            {card.explanation && <p className="hint" style={{ margin: 0, textAlign: "center" }}>{card.explanation}</p>}
          </div>
        </div>
      </button>

      <div className="study-controls">
        <button type="button" className="btn-secondary" disabled={index === 0} onClick={() => go(-1)}>
          ← Previous
        </button>
        <button type="button" className="btn-secondary" onClick={() => setFlipped((f) => !f)}>
          {flipped ? "Show question" : "Show answer"}
        </button>
        {atEnd ? (
          <button type="button" onClick={() => restart(true)}>
            <IconRefresh /> Shuffle and restart
          </button>
        ) : (
          <button type="button" onClick={() => go(1)}>
            Next <IconChevronRight />
          </button>
        )}
      </div>

      <p className="hint" style={{ textAlign: "center" }}>
        Space flips · ← → move · Esc exits. Grading and scheduling come next; this is just a read-through.
      </p>
    </div>
  );
}
