import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectFile } from "@/stores/document-store";
import type {
  CitationCandidate,
  CitationNeedDecisionDebug,
} from "@/lib/citation-api";

const mocks = vi.hoisted(() => {
  const texContent =
    "Na2TiO3 nanotubes were synthesized by hydrothermal treatment.";
  const selectionStart = texContent.indexOf("Na2TiO3");
  const selectionEnd = selectionStart + "Na2TiO3 nanotubes".length;

  const makeTex = (): ProjectFile => ({
    id: "main.tex",
    name: "main.tex",
    relativePath: "main.tex",
    absolutePath: "/project/main.tex",
    type: "tex",
    content: texContent,
    isDirty: false,
  });

  const makeBib = (): ProjectFile => ({
    id: "references.bib",
    name: "references.bib",
    relativePath: "references.bib",
    absolutePath: "/project/references.bib",
    type: "bib",
    content: "",
    isDirty: false,
  });

  const docState = {
    projectRoot: "/project/demo-paper",
    files: [makeTex(), makeBib()],
    activeFileId: "main.tex",
    selectionRange: { start: selectionStart, end: selectionEnd } as {
      start: number;
      end: number;
    } | null,
    lastSelectionRange: null as { fileId: string; start: number; end: number } | null,
    updateFileContent: vi.fn((id: string, content: string) => {
      const file = docState.files.find((f) => f.id === id);
      if (file) {
        file.content = content;
        file.isDirty = true;
      }
    }),
    refreshFiles: vi.fn(async () => {}),
  };

  const zoteroState = {
    apiKey: null as string | null,
    userID: null as string | null,
  };

  return {
    citationApi: {
      searchCitations: vi.fn(),
      searchCitationsDebug: vi.fn(),
    },
    fsApi: {
      createFileOnDisk: vi.fn(async () => {}),
      appendJsonLineToProject: vi.fn(
        async () => "/project/.workflow-local/citation_usage_baseline.jsonl",
      ),
    },
    zoteroApi: {
      upsertZoteroItemFromCitation: vi.fn(async () => {}),
    },
    docStore: {
      getState: vi.fn(() => docState),
      docState,
      makeTex,
      makeBib,
      selectionStart,
      selectionEnd,
    },
    zoteroStore: {
      getState: vi.fn(() => zoteroState),
      zoteroState,
    },
  };
});

vi.mock("@/lib/debug/logger", () => ({
  createLogger: () => ({
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

vi.mock("@/lib/citation-api", () => mocks.citationApi);
vi.mock("@/lib/tauri/fs", () => mocks.fsApi);
vi.mock("@/lib/zotero-api", () => mocks.zoteroApi);
vi.mock("@/stores/document-store", () => ({
  useDocumentStore: mocks.docStore,
}));
vi.mock("@/stores/zotero-store", () => ({
  useZoteroStore: mocks.zoteroStore,
}));

let useCitationStore: (typeof import("@/stores/citation-store"))["useCitationStore"];

function makeCandidate(overrides: Partial<CitationCandidate> = {}): CitationCandidate {
  return {
    paper_id: "paper-1",
    title: "Hydrothermal synthesis of sodium titanate nanotubes",
    authors: ["Rui Liu"],
    score: 0.88,
    year: 2025,
    doi: "10.1000/xyz123",
    url: "https://doi.org/10.1000/xyz123",
    ...overrides,
  };
}

function makeNeedDecision(
  overrides: Partial<CitationNeedDecisionDebug> = {},
): CitationNeedDecisionDebug {
  return {
    needs_citation: true,
    level: "must",
    claim_type: "method",
    recommended_refs: 1,
    score: 0.72,
    reasons: ["contains method/material anchor terms"],
    ...overrides,
  };
}

function getFile(id: string): ProjectFile {
  const file = mocks.docStore.docState.files.find((f) => f.id === id);
  if (!file) throw new Error(`file ${id} not found`);
  return file;
}

function resetDocState() {
  mocks.docStore.docState.projectRoot = "/project/demo-paper";
  mocks.docStore.docState.files = [mocks.docStore.makeTex(), mocks.docStore.makeBib()];
  mocks.docStore.docState.activeFileId = "main.tex";
  mocks.docStore.docState.selectionRange = {
    start: mocks.docStore.selectionStart,
    end: mocks.docStore.selectionEnd,
  };
  mocks.docStore.docState.lastSelectionRange = null;
  mocks.zoteroStore.zoteroState.apiKey = null;
  mocks.zoteroStore.zoteroState.userID = null;
}

function resetCitationStoreState() {
  useCitationStore.setState({
    isSearching: false,
    isApplying: false,
    error: null,
    results: [],
    autoCandidates: [],
    reviewCandidates: [],
    lastAutoAppliedTitle: null,
    lastInsertedCitekey: null,
    citationStylePolicy: "auto",
    autoApplyThreshold: 0.78,
    reviewThreshold: 0.62,
    searchLimit: 8,
    zoteroAutoSyncOnApply: true,
    decisionHint: null,
    isDebugSearching: false,
    debugInfo: null,
    lastNeedDecision: null,
  });
}

describe("citation-store integration flows", () => {
  beforeAll(async () => {
    ({ useCitationStore } = await import("@/stores/citation-store"));
  });

  beforeEach(() => {
    mocks.citationApi.searchCitations.mockReset();
    mocks.citationApi.searchCitationsDebug.mockReset();
    mocks.fsApi.createFileOnDisk.mockReset();
    mocks.fsApi.appendJsonLineToProject.mockReset();
    mocks.fsApi.appendJsonLineToProject.mockResolvedValue(
      "/project/.workflow-local/citation_usage_baseline.jsonl",
    );
    mocks.zoteroApi.upsertZoteroItemFromCitation.mockReset();
    mocks.docStore.docState.updateFileContent.mockClear();
    mocks.docStore.docState.refreshFiles.mockClear();
    resetDocState();
    resetCitationStoreState();
  });

  it("auto-applies top candidate from search and updates tex+bib", async () => {
    const candidate = makeCandidate();
    mocks.citationApi.searchCitations.mockResolvedValueOnce({
      results: [candidate],
      need_decision: makeNeedDecision(),
    });

    await useCitationStore.getState().searchFromSelection();

    const tex = getFile("main.tex").content ?? "";
    const bib = getFile("references.bib").content ?? "";
    const state = useCitationStore.getState();

    expect(state.error).toBeNull();
    expect(state.lastAutoAppliedTitle).toBe(candidate.title);
    expect(state.lastInsertedCitekey).toBeTruthy();
    expect(tex).toContain("\\cite");
    expect(bib).toContain("@article{");
    expect(bib.toLowerCase()).toContain("doi = {10.1000/xyz123}");
  });

  it("does not duplicate bib entry or sentence cite on repeated apply", async () => {
    const candidate = makeCandidate();

    await useCitationStore.getState().applyCandidate(candidate);
    const firstTex = getFile("main.tex").content ?? "";
    const firstBib = getFile("references.bib").content ?? "";

    await useCitationStore.getState().applyCandidate(candidate);
    const secondTex = getFile("main.tex").content ?? "";
    const secondBib = getFile("references.bib").content ?? "";

    expect(secondTex).toBe(firstTex);
    expect((secondBib.match(/@article\{/g) ?? []).length).toBe(1);
    expect(secondBib).toBe(firstBib);
  });

  it("sets user-facing error when citation provider fails", async () => {
    mocks.citationApi.searchCitations.mockRejectedValueOnce(
      new Error("Semantic Scholar timeout"),
    );

    await useCitationStore.getState().searchFromSelection();
    const state = useCitationStore.getState();

    expect(state.isSearching).toBe(false);
    expect(state.error).toContain("Semantic Scholar timeout");
  });

  it("triggers Zotero sync when credentials are configured", async () => {
    mocks.zoteroStore.zoteroState.apiKey = "zotero-key";
    mocks.zoteroStore.zoteroState.userID = "12345";
    const candidate = makeCandidate();

    await useCitationStore.getState().applyCandidate(candidate);

    expect(mocks.zoteroApi.upsertZoteroItemFromCitation).toHaveBeenCalledTimes(1);
    expect(mocks.zoteroApi.upsertZoteroItemFromCitation).toHaveBeenCalledWith(
      "zotero-key",
      "12345",
      candidate,
      { collectionName: "ClaudePrism - demo-paper" },
    );
  });

  it("e2e-lite: selection -> search -> auto apply -> bib + zotero stay consistent", async () => {
    mocks.zoteroStore.zoteroState.apiKey = "zotero-key";
    mocks.zoteroStore.zoteroState.userID = "12345";
    const candidate = makeCandidate({
      score: 0.93,
      score_explain: {
        sem_title: 0.9,
        sem_abstract: 0.9,
        phrase: 0.8,
        recency: 0.8,
        strength: 0.7,
        contradiction_penalty: 0.02,
        context_factor: 1.1,
        final_score: 0.93,
      },
    });
    mocks.citationApi.searchCitations.mockResolvedValueOnce({
      results: [candidate],
      need_decision: makeNeedDecision(),
    });

    await useCitationStore.getState().searchFromSelection();

    const tex = getFile("main.tex").content ?? "";
    const bib = getFile("references.bib").content ?? "";
    const state = useCitationStore.getState();

    expect(state.error).toBeNull();
    expect(state.lastAutoAppliedTitle).toBe(candidate.title);
    expect(state.lastInsertedCitekey).toBeTruthy();
    expect(tex).toContain("\\cite");
    expect(tex).toContain(state.lastInsertedCitekey ?? "");
    expect(bib.toLowerCase()).toContain("doi = {10.1000/xyz123}");
    expect(mocks.zoteroApi.upsertZoteroItemFromCitation).toHaveBeenCalledTimes(1);
  });

  it("S2 soft gate blocks auto-apply when need level is no, while keeping results visible", async () => {
    const candidate = makeCandidate({ score: 0.94 });
    mocks.citationApi.searchCitations.mockResolvedValueOnce({
      results: [candidate],
      need_decision: makeNeedDecision({
        needs_citation: false,
        level: "no",
        claim_type: "background",
        recommended_refs: 0,
        score: 0.18,
        reasons: ["conservative no-citation decision"],
      }),
    });

    await useCitationStore.getState().searchFromSelection();

    const state = useCitationStore.getState();
    const tex = getFile("main.tex").content ?? "";
    const bib = getFile("references.bib").content ?? "";

    expect(state.results.length).toBe(1);
    expect(state.autoCandidates.length).toBe(0);
    expect(state.lastAutoAppliedTitle).toBeNull();
    expect(state.lastNeedDecision?.level).toBe("no");
    expect(state.decisionHint).toContain("Auto blocked by citation-need gate");
    expect(tex).not.toContain("\\cite");
    expect(bib.trim()).toBe("");
  });
});
