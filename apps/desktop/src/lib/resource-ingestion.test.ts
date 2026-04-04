import { describe, expect, it } from "vitest";
import {
  formatRelevantResourceEvidence,
  findRelevantResourceMatches,
  type IngestedProjectResource,
} from "./resource-ingestion";

function makeResource(segments: Array<{ label: string; text: string }>): IngestedProjectResource {
  return {
    filePath: "attachments/paper.pdf",
    absolutePath: "/tmp/paper.pdf",
    sourceType: "pdf",
    kind: "pdf_document",
    extractionStatus: "ready",
    excerpt: "excerpt",
    searchableText: segments.map((segment) => segment.text).join("\n\n"),
    segments,
  };
}

describe("resource-ingestion", () => {
  it("finds relevant English attachment matches with labeled snippets", () => {
    const resource = makeResource([
      {
        label: "Page 1",
        text: "This section discusses photocatalysis and morphology only.",
      },
      {
        label: "Page 4",
        text: "Hydrophobic surface treatment was evaluated through contact angle measurements after stearic acid modification.",
      },
    ]);

    const matches = findRelevantResourceMatches(
      resource,
      "Which paper mentions hydrophobic experiments?",
    );

    expect(matches).toHaveLength(1);
    expect(matches[0].label).toBe("Page 4");
    expect(matches[0].snippet.toLowerCase()).toContain("hydrophobic");
  });

  it("finds relevant Chinese attachment matches", () => {
    const resource = makeResource([
      {
        label: "Paragraph 1",
        text: "本文主要讨论材料合成方法。",
      },
      {
        label: "Paragraph 2",
        text: "通过水接触角测试评估样品表面的疏水性，并比较不同修饰条件。",
      },
    ]);

    const matches = findRelevantResourceMatches(resource, "哪篇文章提到疏水性相关实验");

    expect(matches).toHaveLength(1);
    expect(matches[0].label).toBe("Paragraph 2");
    expect(matches[0].snippet).toContain("疏水性");
  });

  it("formats grouped resource evidence by document and segment label", () => {
    const lines = formatRelevantResourceEvidence([
      {
        filePath: "attachments/a.pdf",
        sourceType: "pdf",
        matches: [
          {
            label: "Page 4",
            snippet: "...hydrophobic surface treatment...",
            score: 12,
          },
        ],
      },
      {
        filePath: "attachments/b.docx",
        sourceType: "docx",
        matches: [
          {
            label: "Paragraph 2",
            snippet: "...contact angle measurements...",
            score: 8,
          },
        ],
      },
    ]);

    expect(lines).toEqual([
      "[Relevant resource evidence:",
      "- Document: attachments/a.pdf (pdf)",
      "  - Page 4: ...hydrophobic surface treatment...",
      "- Document: attachments/b.docx (docx)",
      "  - Paragraph 2: ...contact angle measurements...",
      "]",
    ]);
  });
});
