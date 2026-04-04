import { describe, expect, it } from "vitest";
import {
  findMentionedAttachmentPaths,
  findMentionedProjectFiles,
} from "@/lib/agent-prompt-context";
import type { ProjectFile } from "@/stores/document-store";

function makeFile(relativePath: string, type: ProjectFile["type"] = "other"): ProjectFile {
  const name = relativePath.split("/").pop() ?? relativePath;
  return {
    id: relativePath,
    name,
    relativePath,
    absolutePath: `/tmp/${relativePath}`,
    type,
    isDirty: false,
    content: "",
  };
}

describe("findMentionedProjectFiles", () => {
  it("detects raw @attachments path mentions with spaces and commas", () => {
    const files = [
      makeFile("attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf", "pdf"),
      makeFile("attachments/NaTiO3 CeO2 PDA.pdf", "pdf"),
      makeFile("attachments/TiO2 CuS PDA.pdf", "pdf"),
    ];

    const prompt = [
      "@attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf,",
      "@attachments/NaTiO3 CeO2 PDA.pdf,",
      "@attachments/TiO2 CuS PDA.pdf",
      "哪篇文章提到疏水性相关实验",
    ].join(" ");

    expect(findMentionedProjectFiles(prompt, files).map((file) => file.relativePath)).toEqual([
      "attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf",
      "attachments/NaTiO3 CeO2 PDA.pdf",
      "attachments/TiO2 CuS PDA.pdf",
    ]);
  });

  it("deduplicates repeated raw mentions", () => {
    const files = [makeFile("attachments/NaTiO3 CeO2 PDA.pdf", "pdf")];
    const prompt =
      "@attachments/NaTiO3 CeO2 PDA.pdf\n请读这个 @attachments/NaTiO3 CeO2 PDA.pdf";

    expect(findMentionedProjectFiles(prompt, files)).toHaveLength(1);
  });
});

describe("findMentionedAttachmentPaths", () => {
  it("extracts attachment mentions with spaces and commas in file names", () => {
    const prompt =
      "@attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf, @attachments/NaTiO3 CeO2 PDA.pdf\nread and conclude";

    expect(findMentionedAttachmentPaths(prompt)).toEqual([
      "attachments/Materials Advances, 2025, 6, 7332 - 7354.pdf",
      "attachments/NaTiO3 CeO2 PDA.pdf",
    ]);
  });

  it("keeps attachment mentions unique", () => {
    const prompt =
      "@attachments/TiO2 CuS PDA.pdf\n请再次看 @attachments/TiO2 CuS PDA.pdf";

    expect(findMentionedAttachmentPaths(prompt)).toEqual([
      "attachments/TiO2 CuS PDA.pdf",
    ]);
  });
});
