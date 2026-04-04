import {
  ingestProjectResource,
} from "@/lib/resource-ingestion";
import type { ProjectFile } from "@/stores/document-store";
import type { AgentPromptContext } from "@/stores/agent-chat-store";

export async function buildPromptContextForProjectFile(
  file: ProjectFile,
): Promise<AgentPromptContext> {
  const isDocxFile = /\.docx$/i.test(file.relativePath);
  const isTextFile =
    file.type === "tex" ||
    file.type === "bib" ||
    file.type === "style" ||
    (file.type === "other" && !isDocxFile && typeof file.content === "string");

  if (isTextFile) {
    return {
      label: `@${file.relativePath}`,
      filePath: file.relativePath,
      absolutePath: file.absolutePath,
      selectedText: file.content ?? "",
      kind: "file",
      sourceType: file.type,
    };
  }

  const ingestedResource = await ingestProjectResource(file);
  if (ingestedResource) {
    return {
      label: `@${file.relativePath}`,
      filePath: file.relativePath,
      absolutePath: file.absolutePath,
      selectedText: ingestedResource.excerpt,
      kind: "attachment",
      sourceType: ingestedResource.sourceType,
    };
  }

  return {
    label: `@${file.relativePath}`,
    filePath: file.relativePath,
    absolutePath: file.absolutePath,
    selectedText: `[Attached file: ${file.relativePath} (${file.type} file)]`,
    kind: "attachment",
    sourceType: file.type,
  };
}

export function findMentionedProjectFiles(
  prompt: string,
  files: ProjectFile[],
): ProjectFile[] {
  if (!prompt.includes("@")) return [];

  const matches = files
    .filter((file) => prompt.includes(`@${file.relativePath}`))
    .sort((a, b) => b.relativePath.length - a.relativePath.length);

  const seen = new Set<string>();
  const unique: ProjectFile[] = [];
  for (const file of matches) {
    if (seen.has(file.relativePath)) continue;
    seen.add(file.relativePath);
    unique.push(file);
  }
  return unique;
}

function trimMentionPath(raw: string): string {
  return raw.trim().replace(/[),.;:!?]+$/g, "").trim();
}

export function findMentionedAttachmentPaths(prompt: string): string[] {
  if (!prompt.includes("@attachments/")) return [];

  const matches = Array.from(prompt.matchAll(/@attachments\/([\s\S]*?)(?=\n|@|$)/g))
    .map((match) => trimMentionPath(`attachments/${match[1] ?? ""}`))
    .filter((path) => path.length > "attachments/".length);

  return Array.from(new Set(matches));
}
