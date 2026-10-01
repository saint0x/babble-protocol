import type { BabbleFrontendClient, ObjectJudgment } from "./protocol";

/** Independent observations stay separate; the legacy composite is inspection data. */
export function agreementSummary(output: ObjectJudgment["output"]): HTMLElement | null {
  if (!output || typeof output !== "object" || Array.isArray(output) || output.kind !== "source_agreement") return null;
  const section = document.createElement("section");
  section.setAttribute("aria-label", "Uncalibrated source comparison");
  const note = document.createElement("p");
  note.className = "judgment-caveat";
  note.textContent = output.validation_count === 0
    ? "No eligible public sources."
    : "Lexical overlap, not a truth assessment. Source weights are unverified priors.";
  section.append(note);
  if (output.validation_count === 0) return section;
  const metrics = document.createElement("dl");
  metrics.className = "judgment-metrics";
  for (const [label, key] of [
    ["Term overlap", "term_agreement"], ["Sentence overlap", "fact_agreement"],
    ["Source prior", "reliability_score"], ["Age weight", "temporal_weight"],
  ] as const) {
    const value = output[key];
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) continue;
    const item = document.createElement("div");
    item.className = "judgment-metric";
    const term = document.createElement("dt"); term.textContent = label;
    const detail = document.createElement("dd"); detail.textContent = `${Math.round(value * 100)}%`;
    item.append(term, detail); metrics.append(item);
  }
  section.append(metrics);
  return section;
}

export function judgmentInputView(client: Pick<BabbleFrontendClient, "judgmentInput">, id: string): HTMLDetailsElement {
  const details = document.createElement("details");
  details.className = "judgment-input";
  const summary = document.createElement("summary"); summary.textContent = "Evaluation inputs";
  const status = document.createElement("p"); status.setAttribute("role", "status");
  const data = document.createElement("pre");
  const retry = document.createElement("button"); retry.type = "button"; retry.textContent = "Retry"; retry.hidden = true;
  let loaded = false, pending = false;
  const load = async (): Promise<void> => {
    if (loaded || pending) return;
    pending = true; retry.hidden = true; status.textContent = "Loading evaluation inputs...";
    details.setAttribute("aria-busy", "true");
    try {
      const input = await client.judgmentInput(id);
      data.textContent = input ? JSON.stringify(input.request, null, 2) : "";
      status.textContent = input ? "" : "Input record unavailable for this historical evaluation.";
      loaded = true;
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "Could not load evaluation inputs.";
      retry.hidden = false;
    } finally { pending = false; details.setAttribute("aria-busy", "false"); }
  };
  details.addEventListener("toggle", () => { if (details.open) void load(); });
  retry.addEventListener("click", () => void load());
  details.append(summary, status, data, retry);
  return details;
}
