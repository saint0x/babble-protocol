import type { FeedCard } from "./protocol";
import type { ReadingPosition } from "./reading";

export interface ObjectVisitSnapshot {
  readonly cards: readonly FeedCard[];
  readonly index: number;
  readonly source: string | null;
  readonly reading: ReadingPosition | null;
}

interface Visit {
  readonly snapshot: ObjectVisitSnapshot;
  readonly opener: HTMLElement | null;
}

interface VisitHost {
  capture(): ObjectVisitSnapshot;
  beforeVisit(): void;
  show(snapshot: ObjectVisitSnapshot): void;
  focus(opener: HTMLElement | null): void;
  availability(back: boolean): void;
}

const HISTORY_LIMIT = 32;

/** Visiting a quoted Object never replaces the feed we must return to. */
export class ObjectVisits {
  private path: Visit[] = [];

  constructor(private readonly host: VisitHost) {}

  get active(): boolean { return this.path.length > 0; }

  open(card: FeedCard, opener: HTMLElement | null, source = "Shared post"): void {
    const current = this.host.capture();
    if (current.cards[current.index]?.id === card.id) return;
    this.host.beforeVisit();
    // At the bound, preserve the original feed and the most recent visits.
    if (this.path.length === HISTORY_LIMIT) this.path.splice(1, 1);
    this.path.push({ snapshot: this.host.capture(), opener });
    this.host.availability(true);
    this.host.show({ cards: [card], index: 0, source, reading: null });
    this.host.focus(null);
  }

  back(): void {
    if (!this.active) return;
    this.host.beforeVisit();
    const visit = this.path.pop()!;
    this.host.availability(this.active);
    this.host.show(visit.snapshot);
    this.host.focus(visit.opener);
  }

  checkpoint(): readonly Visit[] { return [...this.path]; }

  restore(checkpoint: readonly Visit[]): void {
    this.path = [...checkpoint];
    this.host.availability(this.active);
  }

  clear(): void {
    this.path = [];
    this.host.availability(false);
  }
}
