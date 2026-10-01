export interface ReadingPosition {
  readonly anchor: string | null;
  readonly offset: number;
  readonly top: number;
}

// Layout offsets are independent of the deck's perspective/scale transitions.
function topWithin(element: HTMLElement, column: HTMLElement): number | null {
  let top = 0;
  let current: HTMLElement | null = element;
  while (current && current !== column) {
    top += current.offsetTop;
    current = current.offsetParent as HTMLElement | null;
  }
  return current === column ? top : null;
}

export function captureReading(column: HTMLElement): ReadingPosition {
  const top = column.scrollTop;
  for (const element of column.querySelectorAll<HTMLElement>("[data-reading-anchor]")) {
    const position = topWithin(element, column);
    if (position !== null && position + element.offsetHeight > top
      && position < top + column.clientHeight) {
      return { anchor: element.dataset.readingAnchor ?? null, offset: position - top, top };
    }
  }
  return { anchor: null, offset: 0, top };
}

export function restoreReading(column: HTMLElement, position: ReadingPosition): void {
  if (position.anchor !== null) {
    for (const element of column.querySelectorAll<HTMLElement>("[data-reading-anchor]")) {
      if (element.dataset.readingAnchor !== position.anchor) continue;
      const top = topWithin(element, column);
      if (top !== null) {
        column.scrollTop = Math.max(0, top - position.offset);
        return;
      }
    }
  }
  column.scrollTop = position.top;
}
