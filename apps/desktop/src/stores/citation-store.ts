import { create } from "zustand";
import {
  searchCitations,
  searchCitationsDebug,
  type CitationCandidate,
  type CitationNeedDecisionDebug,
  type CitationSearchDebug,
} from "@/lib/citation-api";
import { appendJsonLineToProject, createFileOnDisk } from "@/lib/tauri/fs";
import { upsertZoteroItemFromCitation } from "@/lib/zotero-api";
import { createLogger } from "@/lib/debug/logger";
import { useDocumentStore } from "@/stores/document-store";
import { useZoteroStore } from "@/stores/zotero-store";

const log = createLogger("citation");
const DEFAULT_AUTO_APPLY_THRESHOLD = 0.64;
const DEFAULT_REVIEW_THRESHOLD = 0.5;
const DEFAULT_SEARCH_LIMIT = 8;
const DEFAULT_ZOTERO_AUTO_SYNC_ON_APPLY = true;
const AUTO_APPLY_MAX_CONTRADICTION_PENALTY = 0.04;
const REVIEW_MAX_CONTRADICTION_PENALTY = 0.12;

interface CitationState {
  isSearching: boolean;
  isApplying: boolean;
  error: string | null;
  results: CitationCandidate[];
  autoCandidates: CitationCandidate[];
  reviewCandidates: CitationCandidate[];
  lastAutoAppliedTitle: string | null;
  lastInsertedCitekey: string | null;
  citationStylePolicy: CitationStylePolicy;
  autoApplyThreshold: number;
  reviewThreshold: number;
  searchLimit: number;
  zoteroAutoSyncOnApply: boolean;
  decisionHint: string | null;
  isDebugSearching: boolean;
  debugInfo: CitationSearchDebug | null;
  lastNeedDecision: CitationNeedDecisionDebug | null;
  setCitationStylePolicy: (policy: CitationStylePolicy) => void;
  setRuntimeConfig: (config: {
    autoApplyThreshold: number;
    reviewThreshold: number;
    searchLimit: number;
    zoteroAutoSyncOnApply: boolean;
  }) => void;
  runDebugFromSelection: () => Promise<void>;
  clearDebugInfo: () => void;
  searchFromSelection: () => Promise<void>;
  applyCandidate: (candidate: CitationCandidate) => Promise<void>;
  clear: () => void;
}

interface ParsedBibEntry {
  citekey: string;
  entryType: string;
  start: number;
  end: number;
  raw: string;
  fields: Map<string, string>;
  doi?: string;
  title?: string;
}

type CitationCommand = "\\cite" | "\\citep" | "\\autocite";
type CitationStylePolicy = "auto" | "cite" | "citep" | "autocite";

function contradictionPenalty(candidate: CitationCandidate): number {
  const explain = candidate.score_explain;
  if (!explain) return 0;
  return (explain.contradiction_penalty ?? 0) + (explain.formula_penalty ?? 0);
}

function isAutoApplyCandidate(
  candidate: CitationCandidate,
  autoThreshold: number,
  autoPenaltyCap: number,
): boolean {
  return (
    candidate.score >= autoThreshold &&
    contradictionPenalty(candidate) <= autoPenaltyCap
  );
}

function isReviewCandidate(
  candidate: CitationCandidate,
  reviewThreshold: number,
  autoThreshold: number,
  reviewPenaltyCap: number,
  autoPenaltyCap: number,
): boolean {
  const penalty = contradictionPenalty(candidate);
  if (penalty > reviewPenaltyCap) return false;
  return (
    candidate.score >= reviewThreshold &&
    !isAutoApplyCandidate(candidate, autoThreshold, autoPenaltyCap)
  );
}

function percent(value: number): number {
  return Math.round(value * 100);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function buildDecisionHint(args: {
  topCandidate: CitationCandidate | null;
  autoCandidatesCount: number;
  reviewCandidatesCount: number;
  autoBlockedByNeed: boolean;
  needDecision: CitationNeedDecisionDebug | null;
  autoThreshold: number;
  reviewThreshold: number;
  autoPenaltyCap: number;
  reviewPenaltyCap: number;
}): string | null {
  const {
    topCandidate,
    autoCandidatesCount,
    reviewCandidatesCount,
    autoBlockedByNeed,
    needDecision,
    autoThreshold,
    reviewThreshold,
    autoPenaltyCap,
    reviewPenaltyCap,
  } = args;

  if (!topCandidate) return null;
  const score = topCandidate.score;
  const penalty = contradictionPenalty(topCandidate);

  if (autoBlockedByNeed) {
    const detail = needDecision
      ? ` (need=${needDecision.level}, type=${needDecision.claim_type})`
      : "";
    return `Auto blocked by citation-need gate${detail}; keeping manual review path open.`;
  }

  if (autoCandidatesCount > 0) {
    return `Top hit reaches auto policy (${percent(score)}% >= ${percent(autoThreshold)}%).`;
  }
  if (score >= autoThreshold && penalty > autoPenaltyCap) {
    return `Auto blocked by contradiction penalty (${percent(penalty)}% > ${percent(autoPenaltyCap)}%).`;
  }
  if (reviewCandidatesCount > 0) {
    return `Top hit is routed to review (${percent(score)}% >= ${percent(reviewThreshold)}%).`;
  }
  if (score < reviewThreshold) {
    return `Top hit score ${percent(score)}% is below review threshold ${percent(reviewThreshold)}%.`;
  }
  if (penalty > reviewPenaltyCap) {
    return `Review blocked by contradiction penalty (${percent(penalty)}% > ${percent(reviewPenaltyCap)}%).`;
  }
  return "No candidate passed current policy gates.";
}

function tokenizeMetric(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9_]+/g)
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
}

function normalize(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "");
}

function normalizeDoi(value?: string): string {
  if (!value) return "";
  return value
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/doi\.org\//, "");
}

function unwrapBibValue(value: string): string {
  let out = value.trim();
  while (
    (out.startsWith("{") && out.endsWith("}")) ||
    (out.startsWith('"') && out.endsWith('"'))
  ) {
    out = out.slice(1, -1).trim();
  }
  return sanitizeBibValue(out);
}

function parseBibFields(entryRaw: string): Map<string, string> {
  const fields = new Map<string, string>();
  const body = entryRaw.replace(/^@\w+\s*\{[^,]+,\s*/s, "").replace(/\}\s*$/s, "");
  const regex =
    /([a-zA-Z][a-zA-Z0-9_-]*)\s*=\s*(\{(?:[^{}]|\{[^{}]*\})*\}|"(?:[^"\\]|\\.)*"|[^,\n]+)\s*,?/gs;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(body)) !== null) {
    const key = match[1].toLowerCase();
    const value = unwrapBibValue(match[2]);
    if (value) fields.set(key, value);
  }
  return fields;
}

function parseBibEntries(content: string): ParsedBibEntry[] {
  const out: ParsedBibEntry[] = [];
  let cursor = 0;

  while (cursor < content.length) {
    const at = content.indexOf("@", cursor);
    if (at === -1) break;
    const openBrace = content.indexOf("{", at);
    if (openBrace === -1) break;

    let depth = 0;
    let end = -1;
    for (let i = openBrace; i < content.length; i += 1) {
      const ch = content[i];
      if (ch === "{") depth += 1;
      if (ch === "}") {
        depth -= 1;
        if (depth === 0) {
          end = i + 1;
          break;
        }
      }
    }
    if (end === -1) break;

    const raw = content.slice(at, end);
    const header = raw.match(/^@([a-zA-Z][a-zA-Z0-9_-]*)\s*\{\s*([^,\s]+)\s*,?/s);
    if (!header) {
      cursor = end;
      continue;
    }
    const entryType = header[1].toLowerCase();
    const key = header[2].trim();
    if (!key) continue;
    const fields = parseBibFields(raw);
    const doi = fields.get("doi");
    const title = fields.get("title");
    out.push({
      citekey: key,
      entryType,
      start: at,
      end,
      raw,
      fields,
      doi,
      title,
    });
    cursor = end;
  }

  return out;
}

function findExistingEntry(
  entries: ParsedBibEntry[],
  candidate: CitationCandidate,
): ParsedBibEntry | null {
  const doi = candidate.doi?.trim();
  if (doi) {
    const normDoi = normalizeDoi(doi);
    const hit = entries.find(
      (e) => e.doi && normalizeDoi(e.doi) === normDoi,
    );
    if (hit) return hit;
  }

  const title = candidate.title?.trim();
  if (!title) return null;
  const titleNorm = normalize(title);
  const hit = entries.find(
    (e) => e.title && normalize(e.title) === titleNorm,
  );
  return hit ?? null;
}

function sanitizeBibValue(value: string): string {
  return value.replace(/[{}]/g, "").replace(/\s+/g, " ").trim();
}

function detectCitationCommand(content: string): CitationCommand {
  if (
    /\\usepackage(?:\[[^\]]*\])?\{biblatex\}/.test(content) ||
    /\\addbibresource\{/.test(content) ||
    /\\autocite\{/.test(content)
  ) {
    return "\\autocite";
  }
  if (
    /\\usepackage(?:\[[^\]]*\])?\{natbib\}/.test(content) ||
    /\\citep\{/.test(content)
  ) {
    return "\\citep";
  }
  return "\\cite";
}

function resolveCitationCommand(
  content: string,
  policy: CitationStylePolicy,
): CitationCommand {
  if (policy === "auto") return detectCitationCommand(content);
  return `\\${policy}`;
}

function toProjectCollectionName(projectRoot: string): string {
  const base = projectRoot.split(/[/\\]/).filter(Boolean).pop() || "Project";
  return `ClaudePrism - ${base}`;
}

function buildBaseCitekey(candidate: CitationCandidate): string {
  const firstAuthor = candidate.authors[0] ?? "ref";
  const lastNameRaw = firstAuthor.split(/\s+/).pop() ?? "ref";
  const lastName = normalize(lastNameRaw) || "ref";
  const year = String(candidate.year ?? "nd");
  const titleToken =
    sanitizeBibValue(candidate.title)
      .toLowerCase()
      .split(/\s+/)
      .find((t) => t.length >= 4) ?? "paper";
  const token = normalize(titleToken) || "paper";
  return `${lastName}${year}${token}`;
}

function ensureUniqueCitekey(base: string, existingKeys: Set<string>): string {
  if (!existingKeys.has(base.toLowerCase())) return base;
  const alphabet = "abcdefghijklmnopqrstuvwxyz";
  for (const ch of alphabet) {
    const next = `${base}${ch}`;
    if (!existingKeys.has(next.toLowerCase())) return next;
  }
  let i = 1;
  while (existingKeys.has(`${base}${i}`.toLowerCase())) i += 1;
  return `${base}${i}`;
}

function buildCandidateFields(candidate: CitationCandidate): Map<string, string> {
  const fields = new Map<string, string>();
  const title = sanitizeBibValue(candidate.title);
  const authors =
    candidate.authors.length > 0
      ? candidate.authors.map(sanitizeBibValue).join(" and ")
      : "Unknown";
  const year = String(candidate.year ?? "").trim();
  const venue = candidate.venue ? sanitizeBibValue(candidate.venue) : "";
  const doi = candidate.doi ? sanitizeBibValue(normalizeDoi(candidate.doi)) : "";
  const url = candidate.url ? sanitizeBibValue(candidate.url) : "";

  if (title) fields.set("title", title);
  if (authors) fields.set("author", authors);
  if (year) fields.set("year", year);
  if (venue) fields.set("journal", venue);
  if (doi) fields.set("doi", doi);
  if (url) fields.set("url", url);
  return fields;
}

function shouldReplaceField(current: string | undefined): boolean {
  if (!current) return true;
  const v = current.trim().toLowerCase();
  return !v || v === "unknown" || v === "n.d.";
}

function mergeBibFields(
  existingFields: Map<string, string>,
  candidate: CitationCandidate,
): Map<string, string> {
  const next = new Map(existingFields);
  const candidateFields = buildCandidateFields(candidate);
  for (const [key, value] of candidateFields) {
    const current = next.get(key);
    if (shouldReplaceField(current)) {
      next.set(key, value);
    }
    if (
      key === "doi" &&
      !current &&
      value
    ) {
      next.set(key, value);
    }
  }
  return next;
}

function buildBibEntryFromFields(
  entryType: string,
  citekey: string,
  fields: Map<string, string>,
): string {
  const ordered = ["title", "author", "year", "journal", "doi", "url"];
  const extras = Array.from(fields.keys()).filter((k) => !ordered.includes(k));
  extras.sort();
  const lines = [`@${entryType}{${citekey},`];
  for (const key of [...ordered, ...extras]) {
    const value = fields.get(key);
    if (!value) continue;
    lines.push(`  ${key} = {${sanitizeBibValue(value)}},`);
  }
  if (lines[lines.length - 1]?.endsWith(",")) {
    lines[lines.length - 1] = lines[lines.length - 1].slice(0, -1);
  }
  lines.push("}");
  return lines.join("\n");
}

function upsertBibEntry(
  bibContent: string,
  candidate: CitationCandidate,
): { nextBib: string; citekey: string; changed: boolean } {
  const entries = parseBibEntries(bibContent);
  const existing = findExistingEntry(entries, candidate);
  if (!existing) {
    const base = buildBaseCitekey(candidate);
    const key = ensureUniqueCitekey(
      base,
      new Set(entries.map((e) => e.citekey.toLowerCase())),
    );
    const entry = buildBibEntryFromFields("article", key, buildCandidateFields(candidate));
    const nextBib = bibContent.trim()
      ? `${bibContent.trimEnd()}\n\n${entry}\n`
      : `${entry}\n`;
    return { nextBib, citekey: key, changed: nextBib !== bibContent };
  }

  const mergedFields = mergeBibFields(existing.fields, candidate);
  const mergedEntry = buildBibEntryFromFields(
    existing.entryType || "article",
    existing.citekey,
    mergedFields,
  );
  if (mergedEntry.trim() === existing.raw.trim()) {
    return { nextBib: bibContent, citekey: existing.citekey, changed: false };
  }
  const nextBib =
    bibContent.slice(0, existing.start) +
    mergedEntry +
    bibContent.slice(existing.end);
  return {
    nextBib,
    citekey: existing.citekey,
    changed: nextBib !== bibContent,
  };
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function sentenceContainsCitekey(sentence: string, citekey: string): boolean {
  const key = escapeRegExp(citekey);
  const re = new RegExp(
    String.raw`\\(?:cite|citep|citet|autocite|parencite|textcite)\s*(?:\[[^\]]*\]\s*)?\{[^}]*\b${key}\b[^}]*\}`,
    "i",
  );
  return re.test(sentence);
}

function isSentenceEndChar(ch: string): boolean {
  return ".!?。！？;；".includes(ch);
}

function isClosingChar(ch: string): boolean {
  return `"'”’)]}】）》`.includes(ch);
}

function findSentenceBounds(
  content: string,
  start: number,
  end: number,
): { sentenceStart: number; sentenceEnd: number; punctuationIndex: number | null } {
  const safeStart = Math.max(0, Math.min(start, content.length));
  const safeEnd = Math.max(safeStart, Math.min(end, content.length));

  let sentenceStart = safeStart;
  while (sentenceStart > 0) {
    const prev = content[sentenceStart - 1];
    if (isSentenceEndChar(prev) || prev === "\n") break;
    sentenceStart -= 1;
  }

  let punctuationIndex: number | null = null;
  let sentenceEnd = safeEnd;
  for (let i = safeEnd; i < content.length; i += 1) {
    const ch = content[i];
    if (isSentenceEndChar(ch)) {
      punctuationIndex = i;
      sentenceEnd = i + 1;
      while (sentenceEnd < content.length && isClosingChar(content[sentenceEnd])) {
        sentenceEnd += 1;
      }
      break;
    }
    if (ch === "\n") {
      sentenceEnd = i;
      break;
    }
  }

  return { sentenceStart, sentenceEnd, punctuationIndex };
}

function buildCitationInsertion(
  content: string,
  insertionIndex: number,
  command: CitationCommand,
  citekey: string,
): string {
  const prev = insertionIndex > 0 ? content[insertionIndex - 1] : "";
  const needsLeadingSpace = !!prev && !/\s/.test(prev);
  return `${needsLeadingSpace ? " " : ""}${command}{${citekey}}`;
}

function pickSelectedText() {
  const doc = useDocumentStore.getState();
  const active = doc.files.find((f) => f.id === doc.activeFileId);
  if (!active || active.type !== "tex" || !active.content) return null;
  const range = doc.selectionRange;
  if (!range || range.end <= range.start) return null;
  const selected = active.content.slice(range.start, range.end).trim();
  if (!selected) return null;
  return { active, range, selected };
}

function pickSelectionWithFallback() {
  const current = pickSelectedText();
  if (current) return current;

  const doc = useDocumentStore.getState();
  const active = doc.files.find((f) => f.id === doc.activeFileId);
  if (!active || active.type !== "tex" || !active.content) return null;
  const last = doc.lastSelectionRange;
  if (!last || last.fileId !== active.id) return null;
  if (last.end <= last.start || last.start < 0) return null;
  const max = active.content.length;
  const start = Math.min(last.start, max);
  const end = Math.min(last.end, max);
  if (end <= start) return null;
  const selected = active.content.slice(start, end).trim();
  if (!selected) return null;
  return {
    active,
    range: { start, end },
    selected,
  };
}

export const useCitationStore = create<CitationState>()((set, get) => ({
  isSearching: false,
  isApplying: false,
  error: null,
  results: [],
  autoCandidates: [],
  reviewCandidates: [],
  lastAutoAppliedTitle: null,
  lastInsertedCitekey: null,
  citationStylePolicy: "auto",
  autoApplyThreshold: DEFAULT_AUTO_APPLY_THRESHOLD,
  reviewThreshold: DEFAULT_REVIEW_THRESHOLD,
  searchLimit: DEFAULT_SEARCH_LIMIT,
  zoteroAutoSyncOnApply: DEFAULT_ZOTERO_AUTO_SYNC_ON_APPLY,
  decisionHint: null,
  isDebugSearching: false,
  debugInfo: null,
  lastNeedDecision: null,
  setCitationStylePolicy: (policy) => set({ citationStylePolicy: policy }),
  setRuntimeConfig: (config) =>
    set({
      autoApplyThreshold: clamp(config.autoApplyThreshold, 0, 1),
      reviewThreshold: clamp(
        Math.min(config.reviewThreshold, config.autoApplyThreshold),
        0,
        1,
      ),
      searchLimit: Math.round(clamp(config.searchLimit, 1, 20)),
      zoteroAutoSyncOnApply: !!config.zoteroAutoSyncOnApply,
    }),

  searchFromSelection: async () => {
    const selected = pickSelectionWithFallback();
    if (!selected) {
      set({
        error:
          "Please select a sentence in this .tex file first, then run citation search.",
      });
      return;
    }
    set({ isSearching: true, error: null });
    try {
      const state = get();
      const projectRoot = useDocumentStore.getState().projectRoot ?? null;
      const searchLimit = Math.round(clamp(state.searchLimit, 1, 20));
      const autoThreshold = clamp(state.autoApplyThreshold, 0, 1);
      const reviewThreshold = clamp(
        Math.min(state.reviewThreshold, autoThreshold),
        0,
        1,
      );
      const response = await searchCitations(
        selected.selected,
        searchLimit,
        projectRoot,
      );
      const results = response.results;
      const needDecision = response.need_decision;
      const autoCandidatesRaw = results.filter((candidate) =>
        isAutoApplyCandidate(
          candidate,
          autoThreshold,
          AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
        ),
      );
      const autoBlockedByNeed = needDecision.level === "no";
      const autoCandidates = autoBlockedByNeed ? [] : autoCandidatesRaw;
      const reviewCandidates = results.filter((candidate) =>
        isReviewCandidate(
          candidate,
          reviewThreshold,
          autoThreshold,
          REVIEW_MAX_CONTRADICTION_PENALTY,
          AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
        ),
      );
      const decisionHint = buildDecisionHint({
        topCandidate: results[0] ?? null,
        autoCandidatesCount: autoCandidates.length,
        reviewCandidatesCount: reviewCandidates.length,
        autoBlockedByNeed,
        needDecision,
        autoThreshold,
        reviewThreshold,
        autoPenaltyCap: AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
        reviewPenaltyCap: REVIEW_MAX_CONTRADICTION_PENALTY,
      });
      set({
        results,
        autoCandidates,
        reviewCandidates,
        lastAutoAppliedTitle: null,
        lastNeedDecision: needDecision,
        decisionHint,
        isSearching: false,
        error: results.length > 0 ? null : "No matching papers found.",
      });
      if (projectRoot) {
        const sentenceCount = selected.selected
          .split(/[\.\!\?;\n。！？；]+/g)
          .map((s) => s.trim())
          .filter((s) => s.length > 0).length;
        const metricPayload = {
          ts: new Date().toISOString(),
          selected_chars: selected.selected.length,
          selected_tokens: tokenizeMetric(selected.selected).length,
          sentence_count: sentenceCount,
          paragraph_like: sentenceCount > 1 || selected.selected.includes("\n"),
          has_newline: selected.selected.includes("\n"),
          need_level: needDecision.level,
          claim_type: needDecision.claim_type,
          need_score: needDecision.score,
          recommended_refs: needDecision.recommended_refs,
          results_count: results.length,
          auto_count: autoCandidates.length,
          review_count: reviewCandidates.length,
          top_score: results[0]?.score ?? null,
        };
        appendJsonLineToProject(
          projectRoot,
          ".workflow-local/citation_usage_baseline.jsonl",
          metricPayload,
        ).catch((err) => {
          log.debug("failed to append citation usage baseline", {
            error: String(err),
          });
        });
      }
      if (autoCandidates.length > 0) {
        await get().applyCandidate(autoCandidates[0]);
        set({ lastAutoAppliedTitle: autoCandidates[0].title });
      }
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : typeof err === "string"
            ? err
            : "Citation search failed.";
      log.warn("citation search failed", { error: String(err) });
      set({
        isSearching: false,
        lastNeedDecision: null,
        error: msg,
      });
    }
  },

  runDebugFromSelection: async () => {
    const selected = pickSelectionWithFallback();
    if (!selected) {
      set({
        error:
          "Please select a sentence in this .tex file first, then run debug search.",
      });
      return;
    }
    set({ isDebugSearching: true, error: null });
    try {
      const state = get();
      const projectRoot = useDocumentStore.getState().projectRoot ?? null;
      const searchLimit = Math.round(clamp(state.searchLimit, 1, 20));
      const debugInfo = await searchCitationsDebug(
        selected.selected,
        searchLimit,
        projectRoot,
      );
      set({
        isDebugSearching: false,
        debugInfo,
        error: debugInfo.final_error ?? null,
      });
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : typeof err === "string"
            ? err
            : "Citation debug search failed.";
      set({
        isDebugSearching: false,
        error: msg,
      });
    }
  },

  clearDebugInfo: () => set({ debugInfo: null }),

  applyCandidate: async (candidate) => {
    if (get().isApplying) return;
    const selected = pickSelectionWithFallback();
    if (!selected) {
      set({ error: "Selection changed. Please select text again." });
      return;
    }

    const doc = useDocumentStore.getState();
    const { projectRoot } = doc;
    if (!projectRoot) {
      set({ error: "No project is open." });
      return;
    }

    set({ isApplying: true, error: null });

    try {
      let bibFile =
        doc.files.find((f) => f.relativePath === "references.bib") ??
        doc.files.find((f) => f.name === "references.bib");

      if (!bibFile) {
        await createFileOnDisk(projectRoot, "references.bib", "");
        await doc.refreshFiles();
        bibFile = useDocumentStore
          .getState()
          .files.find((f) => f.relativePath === "references.bib");
      }
      if (!bibFile) {
        throw new Error("Failed to initialize references.bib.");
      }

      const bibContent = bibFile.content ?? "";
      const bibUpdate = upsertBibEntry(bibContent, candidate);
      const citekey = bibUpdate.citekey;
      if (bibUpdate.changed) {
        const nextBib = bibUpdate.nextBib;
        doc.updateFileContent(bibFile.id, nextBib);
      }

      const activeNow = useDocumentStore
        .getState()
        .files.find((f) => f.id === selected.active.id);
      const activeContent = activeNow?.content ?? selected.active.content ?? "";
      const bounds = findSentenceBounds(
        activeContent,
        selected.range.start,
        selected.range.end,
      );
      const sentenceText = activeContent.slice(
        bounds.sentenceStart,
        bounds.sentenceEnd,
      );
      if (!sentenceContainsCitekey(sentenceText, citekey)) {
        const insertionIndex = (() => {
          if (bounds.punctuationIndex == null) return selected.range.end;
          let idx = bounds.punctuationIndex;
          while (idx > bounds.sentenceStart && /\s/.test(activeContent[idx - 1])) {
            idx -= 1;
          }
          return idx;
        })();
        const citeCommand = resolveCitationCommand(
          activeContent,
          get().citationStylePolicy,
        );
        const insertion = buildCitationInsertion(
          activeContent,
          insertionIndex,
          citeCommand,
          citekey,
        );
        const updated =
          activeContent.slice(0, insertionIndex) +
          insertion +
          activeContent.slice(insertionIndex);
        doc.updateFileContent(selected.active.id, updated);
      }

      const zotero = useZoteroStore.getState();
      if (get().zoteroAutoSyncOnApply && zotero.apiKey && zotero.userID) {
        upsertZoteroItemFromCitation(zotero.apiKey, zotero.userID, candidate, {
          collectionName: toProjectCollectionName(projectRoot),
        })
          .catch((err) => {
            log.warn("Failed to sync citation to Zotero", {
              error: String(err),
            });
          });
      }

      set({
        isApplying: false,
        lastInsertedCitekey: citekey,
        decisionHint: null,
      });
    } catch (err) {
      set({
        isApplying: false,
        error:
          err instanceof Error
            ? err.message
            : "Failed to apply citation suggestion.",
      });
    }
  },

  clear: () =>
    set({
      results: [],
      autoCandidates: [],
      reviewCandidates: [],
      lastAutoAppliedTitle: null,
      decisionHint: null,
      error: null,
      lastInsertedCitekey: null,
      isDebugSearching: false,
      debugInfo: null,
      lastNeedDecision: null,
    }),
}));

export const citationStoreTestUtils = {
  AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
  REVIEW_MAX_CONTRADICTION_PENALTY,
  contradictionPenalty,
  isAutoApplyCandidate,
  isReviewCandidate,
  buildDecisionHint,
  normalizeDoi,
  resolveCitationCommand,
  sentenceContainsCitekey,
  findSentenceBounds,
  buildCitationInsertion,
  upsertBibEntry,
};
