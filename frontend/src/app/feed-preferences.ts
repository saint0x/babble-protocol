import type { Accounts } from "./accounts";
import { clearLocalHistory, defaultPreferences, readPreferences, writePreferences, type LocalPreferences } from "./local-preferences";

interface PreferencesPort {
  show(input: { scope: string; preferences: LocalPreferences; warning: string | null; seenCount: number; preserveTab?: boolean }): void;
  message(text: string, error?: boolean): void;
  setSeenCount(count: number): void;
}

interface PreferencesHost {
  changed(): void;
  historyCleared(): void;
  seenCount(): number;
}

/** Local data is partitioned by API origin and account, including guest state. */
export class FeedPreferences {
  private owner: string | null = null;
  private shownOwner: string | null = null;
  private snapshot = "";

  constructor(
    private readonly accounts: Pick<Accounts, "current" | "localDataKey">,
    private readonly storage: () => Storage | null,
    private readonly view: PreferencesPort,
    private readonly host: PreferencesHost,
  ) {}

  read(): LocalPreferences {
    return readPreferences(this.storage(), this.accounts.localDataKey("preferences")).preferences;
  }

  show(preserveTab = false): void {
    const key = this.accounts.localDataKey("preferences");
    const result = readPreferences(this.storage(), key);
    this.shownOwner = key;
    this.snapshot = JSON.stringify(result.preferences);
    const handle = this.accounts.current?.identity.handle;
    this.view.show({ scope: handle ? `This device - @${handle}` : "This device - Guest",
      ...result, seenCount: this.host.seenCount(), preserveTab });
  }

  account(): void {
    const key = this.accounts.localDataKey("preferences");
    if (key === this.owner) return;
    this.owner = key;
    this.show();
  }

  save(value: LocalPreferences): void {
    const key = this.editableKey();
    writePreferences(this.storage(), key, value);
    this.show(true);
    this.view.message("Saved on this device.");
    this.host.changed();
  }

  reset(): void {
    this.save(defaultPreferences());
    this.view.message("Feed preferences reset. Reading history kept.");
  }

  clearHistory(): void {
    this.assertOwner();
    clearLocalHistory(this.storage(), this.accounts.localDataKey("seen"));
    this.host.historyCleared();
    this.view.setSeenCount(0);
    this.view.message("Local reading history cleared.");
    this.host.changed();
  }

  /** Undo removes only this addition; unrelated preferences remain untouched. */
  hideAuthor(author: string): (() => void) | null {
    const key = this.accounts.localDataKey("preferences");
    const current = readPreferences(this.storage(), key);
    if (current.warning) throw new Error("Open Feed controls to resolve the local preferences warning first.");
    if (current.preferences.hiddenAuthors.includes(author)) return null;
    writePreferences(this.storage(), key, {
      ...current.preferences, hiddenAuthors: [...current.preferences.hiddenAuthors, author],
    });
    this.show();
    this.host.changed();
    let undone = false;
    return () => {
      if (undone) return;
      if (key !== this.accounts.localDataKey("preferences")) throw new Error("The account changed. Open its Feed controls to edit preferences.");
      const latest = readPreferences(this.storage(), key);
      if (latest.warning) throw new Error("Local preferences could not be read. Open Feed controls before retrying.");
      writePreferences(this.storage(), key, {
        ...latest.preferences, hiddenAuthors: latest.preferences.hiddenAuthors.filter(id => id !== author),
      });
      undone = true;
      this.show();
      this.host.changed();
    };
  }

  externalChange(key: string | null): void {
    if (key !== null && key !== this.accounts.localDataKey("preferences") && key !== this.accounts.localDataKey("seen")) return;
    this.view.setSeenCount(this.host.seenCount());
    if (key === this.accounts.localDataKey("seen")) return;
    this.view.message("Local feed data changed in another tab. Reopen Feed controls before saving.", true);
    this.host.changed();
  }

  private assertOwner(): string {
    const key = this.accounts.localDataKey("preferences");
    if (key !== this.shownOwner) throw new Error("The account changed. Reopen Feed controls before saving.");
    return key;
  }

  private editableKey(): string {
    const key = this.assertOwner();
    const latest = readPreferences(this.storage(), key);
    if (JSON.stringify(latest.preferences) !== this.snapshot) {
      throw new Error("Preferences changed in another tab. Reopen Feed controls before saving.");
    }
    return key;
  }
}
