import { beforeAll, describe, expect, it, vi } from "vitest";
import type { CitationCandidate } from "@/lib/citation-api";

vi.mock("@/lib/debug/logger", () => ({
  createLogger: () => ({
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

vi.mock("@/lib/citation-api", () => ({
  searchCitations: vi.fn(),
  searchCitationsDebug: vi.fn(),
}));

vi.mock("@/lib/tauri/fs", () => ({
  createFileOnDisk: vi.fn(),
  appendJsonLineToProject: vi.fn(async () => "/project/.workflow-local/citation_usage_baseline.jsonl"),
}));

vi.mock("@/lib/zotero-api", () => ({
  upsertZoteroItemFromCitation: vi.fn(),
}));

vi.mock("@/stores/document-store", () => ({
  useDocumentStore: {
    getState: vi.fn(() => ({
      files: [],
      activeFileId: null,
      selectionRange: null,
      lastSelectionRange: null,
      projectRoot: null,
    })),
  },
}));

vi.mock("@/stores/zotero-store", () => ({
  useZoteroStore: {
    getState: vi.fn(() => ({
      apiKey: null,
      userID: null,
    })),
  },
}));

let utils: (typeof import("@/stores/citation-store"))["citationStoreTestUtils"];

beforeAll(async () => {
  ({ citationStoreTestUtils: utils } = await import("@/stores/citation-store"));
});

function makeCandidate(
  overrides: Partial<CitationCandidate> = {},
): CitationCandidate {
  return {
    paper_id: "paper-1",
    title: "Hydrothermal synthesis of Na2TiO3 nanotubes",
    authors: ["Liu Rui"],
    score: 0.8,
    ...overrides,
  };
}

describe("citation-store policy gates", () => {
  it("blocks auto-apply when contradiction penalty exceeds cap", () => {
    const candidate = makeCandidate({
      score: 0.92,
      score_explain: {
        sem_title: 0.9,
        sem_abstract: 0.9,
        phrase: 0.8,
        recency: 0.7,
        strength: 0.6,
        contradiction_penalty: 0.09,
        context_factor: 1,
        final_score: 0.92,
      },
    });

    const ok = utils.isAutoApplyCandidate(
      candidate,
      0.78,
      utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
    );
    expect(ok).toBe(false);
  });

  it("routes candidate to review when auto gate is blocked by penalty", () => {
    const candidate = makeCandidate({
      score: 0.84,
      score_explain: {
        sem_title: 0.8,
        sem_abstract: 0.8,
        phrase: 0.7,
        recency: 0.7,
        strength: 0.6,
        contradiction_penalty: 0.1,
        context_factor: 1,
        final_score: 0.84,
      },
    });

    const ok = utils.isReviewCandidate(
      candidate,
      0.62,
      0.78,
      utils.REVIEW_MAX_CONTRADICTION_PENALTY,
      utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
    );
    expect(ok).toBe(true);
  });

  it("rejects review candidate when contradiction penalty is too high", () => {
    const candidate = makeCandidate({
      score: 0.74,
      score_explain: {
        sem_title: 0.7,
        sem_abstract: 0.7,
        phrase: 0.7,
        recency: 0.7,
        strength: 0.6,
        contradiction_penalty: 0.3,
        context_factor: 1,
        final_score: 0.74,
      },
    });

    const ok = utils.isReviewCandidate(
      candidate,
      0.62,
      0.78,
      utils.REVIEW_MAX_CONTRADICTION_PENALTY,
      utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
    );
    expect(ok).toBe(false);
  });
});

describe("citation-store decision hints", () => {
  it("explains when auto is blocked by contradiction penalty", () => {
    const candidate = makeCandidate({
      score: 0.84,
      score_explain: {
        sem_title: 0.8,
        sem_abstract: 0.8,
        phrase: 0.7,
        recency: 0.7,
        strength: 0.6,
        contradiction_penalty: 0.1,
        context_factor: 1,
        final_score: 0.84,
      },
    });

    const hint = utils.buildDecisionHint({
      topCandidate: candidate,
      autoCandidatesCount: 0,
      reviewCandidatesCount: 1,
      autoBlockedByNeed: false,
      needDecision: null,
      autoThreshold: 0.78,
      reviewThreshold: 0.62,
      autoPenaltyCap: utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
      reviewPenaltyCap: utils.REVIEW_MAX_CONTRADICTION_PENALTY,
    });

    expect(hint).toContain("Auto blocked by contradiction penalty");
  });

  it("explains low-score rejection", () => {
    const candidate = makeCandidate({ score: 0.4 });
    const hint = utils.buildDecisionHint({
      topCandidate: candidate,
      autoCandidatesCount: 0,
      reviewCandidatesCount: 0,
      autoBlockedByNeed: false,
      needDecision: null,
      autoThreshold: 0.78,
      reviewThreshold: 0.62,
      autoPenaltyCap: utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
      reviewPenaltyCap: utils.REVIEW_MAX_CONTRADICTION_PENALTY,
    });

    expect(hint).toContain("below review threshold");
  });

  it("explains when auto is blocked by citation-need gate", () => {
    const candidate = makeCandidate({ score: 0.91 });
    const hint = utils.buildDecisionHint({
      topCandidate: candidate,
      autoCandidatesCount: 0,
      reviewCandidatesCount: 0,
      autoBlockedByNeed: true,
      needDecision: {
        needs_citation: false,
        level: "no",
        claim_type: "background",
        recommended_refs: 0,
        score: 0.2,
        reasons: ["weak cues"],
      },
      autoThreshold: 0.78,
      reviewThreshold: 0.62,
      autoPenaltyCap: utils.AUTO_APPLY_MAX_CONTRADICTION_PENALTY,
      reviewPenaltyCap: utils.REVIEW_MAX_CONTRADICTION_PENALTY,
    });

    expect(hint).toContain("Auto blocked by citation-need gate");
  });
});

describe("citation-store bib upsert", () => {
  it("is idempotent when the same candidate is upserted twice", () => {
    const candidate = makeCandidate({
      year: 2025,
      venue: "Materials Advances",
      doi: "10.1000/xyz123",
      url: "https://doi.org/10.1000/xyz123",
    });

    const first = utils.upsertBibEntry("", candidate);
    const second = utils.upsertBibEntry(first.nextBib, candidate);

    expect(first.citekey).toBeTruthy();
    expect(second.citekey).toBe(first.citekey);
    expect(second.changed).toBe(false);
    expect((second.nextBib.match(/@article\{/g) ?? []).length).toBe(1);
  });

  it("matches existing entry by normalized DOI", () => {
    const bib = `@article{li2024nano,
  title = {Some title},
  author = {Li, Rui},
  year = {2024},
  doi = {10.1000/XYZ123}
}
`;
    const candidate = makeCandidate({
      doi: "https://doi.org/10.1000/xyz123",
      title: "A different title but same DOI",
      year: 2024,
    });

    const updated = utils.upsertBibEntry(bib, candidate);
    expect(updated.citekey).toBe("li2024nano");
    expect((updated.nextBib.match(/@article\{/g) ?? []).length).toBe(1);
  });
});

describe("citation-store citation command resolution", () => {
  it("detects natbib as citep in auto mode", () => {
    const cmd = utils.resolveCitationCommand(
      "\\usepackage{natbib}\nSome text",
      "auto",
    );
    expect(cmd).toBe("\\citep");
  });

  it("detects biblatex as autocite in auto mode", () => {
    const cmd = utils.resolveCitationCommand(
      "\\usepackage{biblatex}\n\\addbibresource{references.bib}",
      "auto",
    );
    expect(cmd).toBe("\\autocite");
  });

  it("respects explicit style policy", () => {
    const cmd = utils.resolveCitationCommand("plain content", "cite");
    expect(cmd).toBe("\\cite");
  });
});

describe("citation-store sentence helpers", () => {
  it("detects citekeys inside citation commands", () => {
    const sentence = "The method was validated \\citep[see] {foo2020,bar2021}.";
    expect(utils.sentenceContainsCitekey(sentence, "bar2021")).toBe(true);
    expect(utils.sentenceContainsCitekey(sentence, "missing-key")).toBe(false);
  });

  it("inserts citation with proper spacing and sentence bounds", () => {
    const content = "This is a test sentence.";
    const bounds = utils.findSentenceBounds(content, 5, 8);
    expect(bounds.punctuationIndex).toBe(content.length - 1);

    const insertion = utils.buildCitationInsertion(
      content,
      bounds.punctuationIndex ?? content.length,
      "\\cite",
      "liu2025paper",
    );
    expect(insertion).toBe(" \\cite{liu2025paper}");
  });
});
