import { readFile as readBinaryFile } from "@tauri-apps/plugin-fs";
import { exists, mkdir, readTextFile, stat, writeTextFile } from "@tauri-apps/plugin-fs";
import { join } from "@tauri-apps/api/path";
import * as mammoth from "mammoth";
import { createLogger } from "@/lib/debug/logger";
import { getMupdfClient } from "@/lib/mupdf/mupdf-client";
import type { StructuredTextData } from "@/lib/mupdf/types";
import type { ProjectFile } from "@/stores/document-store";

const log = createLogger("resource-ingestion");

const PDF_EXCERPT_MAX_PAGES = 4;
const PDF_EXCERPT_MAX_CHARS = 6_000;
const DOCX_EXCERPT_MAX_CHARS = 6_000;
const GENERIC_TEXT_MAX_CHARS = 20_000;
const RESOURCE_MATCH_LIMIT = 3;
const RESOURCE_MATCH_SNIPPET_CHARS = 220;
const RESOURCE_ARTIFACT_DIR = ".claudeprism/agent-resources";

const ENGLISH_STOP_WORDS = new Set([
  "the",
  "and",
  "for",
  "with",
  "this",
  "that",
  "from",
  "have",
  "has",
  "had",
  "were",
  "was",
  "are",
  "what",
  "which",
  "when",
  "where",
  "does",
  "did",
  "into",
  "than",
  "then",
  "them",
  "they",
  "their",
  "there",
  "about",
  "mentioned",
  "article",
  "paper",
]);

export interface IngestedResourceSegment {
  label: string;
  text: string;
}

export interface IngestedProjectResource {
  filePath: string;
  absolutePath: string;
  sourceType: string;
  kind: "text_file" | "pdf_document" | "docx_document" | "unknown";
  extractionStatus: "ready" | "partial" | "image_only" | "failed";
  excerpt: string;
  searchableText: string;
  segments: IngestedResourceSegment[];
  pageCount?: number;
  artifactPath?: string;
  metadata?: Record<string, unknown>;
}

export interface IngestedResourceMatch {
  label: string;
  snippet: string;
  score: number;
}

export interface ResourceEvidenceGroup {
  filePath: string;
  sourceType?: string;
  matches: IngestedResourceMatch[];
}

const resourceCache = new Map<string, Promise<IngestedProjectResource | null>>();
const RESOURCE_ARTIFACT_VERSION = 2;

function toHexKey(value: string): string {
  return Array.from(new TextEncoder().encode(value))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

function inferProjectRoot(file: ProjectFile): string | null {
  const absolutePath = normalizePath(file.absolutePath);
  const relativePath = normalizePath(file.relativePath);

  if (!relativePath) return null;
  if (absolutePath === relativePath) return null;
  if (!absolutePath.endsWith(relativePath)) return null;

  const root = absolutePath.slice(0, absolutePath.length - relativePath.length);
  return root.endsWith("/") ? root.slice(0, -1) : root;
}

async function persistResourceArtifact(
  file: ProjectFile,
  resource: IngestedProjectResource,
): Promise<string | undefined> {
  const projectRoot = inferProjectRoot(file);
  if (!projectRoot) return undefined;

  const artifactDir = await join(projectRoot, RESOURCE_ARTIFACT_DIR);
  if (!(await exists(artifactDir))) {
    await mkdir(artifactDir, { recursive: true });
  }

  const artifactPath = await join(artifactDir, `${toHexKey(file.relativePath)}.json`);
  await writeTextFile(
    artifactPath,
    JSON.stringify(
      {
        version: RESOURCE_ARTIFACT_VERSION,
        filePath: resource.filePath,
        absolutePath: resource.absolutePath,
        sourceType: resource.sourceType,
        kind: resource.kind,
        extractionStatus: resource.extractionStatus,
        excerpt: resource.excerpt,
        searchableText: resource.searchableText,
        segments: resource.segments,
        pageCount: resource.pageCount,
        metadata: resource.metadata ?? {},
      },
      null,
      2,
    ),
  );
  return artifactPath;
}

type PersistedResourceArtifact = {
  version: number;
  filePath: string;
  absolutePath?: string;
  sourceType: string;
  kind: IngestedProjectResource["kind"];
  extractionStatus: IngestedProjectResource["extractionStatus"];
  excerpt: string;
  searchableText: string;
  segments: IngestedResourceSegment[];
  pageCount?: number;
  metadata?: Record<string, unknown>;
};

async function artifactPathForFile(file: ProjectFile): Promise<string | null> {
  const projectRoot = inferProjectRoot(file);
  if (!projectRoot) return null;
  return join(projectRoot, RESOURCE_ARTIFACT_DIR, `${toHexKey(file.relativePath)}.json`);
}

async function loadPersistedResourceArtifact(
  file: ProjectFile,
): Promise<IngestedProjectResource | null> {
  if (!isPdfPath(file.relativePath) && !isDocxPath(file.relativePath)) {
    return null;
  }

  const artifactPath = await artifactPathForFile(file);
  if (!artifactPath || !(await exists(artifactPath))) {
    return null;
  }

  try {
    const [artifactInfo, sourceInfo] = await Promise.all([
      stat(artifactPath),
      stat(file.absolutePath),
    ]);
    if (
      artifactInfo.mtime != null &&
      sourceInfo.mtime != null &&
      artifactInfo.mtime.getTime() < sourceInfo.mtime.getTime()
    ) {
      return null;
    }
  } catch {
    // If stat fails we still try to read the artifact; readTextFile below will be the source of truth.
  }

  try {
    const parsed = JSON.parse(
      await readTextFile(artifactPath),
    ) as Partial<PersistedResourceArtifact>;

    if (parsed.version !== RESOURCE_ARTIFACT_VERSION) {
      return null;
    }
    if (
      typeof parsed.sourceType !== "string" ||
      typeof parsed.excerpt !== "string" ||
      typeof parsed.searchableText !== "string" ||
      !Array.isArray(parsed.segments)
    ) {
      return null;
    }

    return {
      filePath: file.relativePath,
      absolutePath: file.absolutePath,
      sourceType: parsed.sourceType,
      kind: parsed.kind ?? (isPdfPath(file.relativePath) ? "pdf_document" : "docx_document"),
      extractionStatus: parsed.extractionStatus ?? "failed",
      excerpt: parsed.excerpt,
      searchableText: parsed.searchableText,
      segments: parsed.segments
        .filter(
          (segment): segment is IngestedResourceSegment =>
            typeof segment?.label === "string" && typeof segment?.text === "string",
        )
        .map((segment) => ({
          label: segment.label,
          text: segment.text,
        })),
      pageCount: typeof parsed.pageCount === "number" ? parsed.pageCount : undefined,
      artifactPath,
      metadata:
        parsed.metadata && typeof parsed.metadata === "object" ? parsed.metadata : undefined,
    };
  } catch (error) {
    log.warn("Failed to load persisted resource artifact", {
      filePath: file.relativePath,
      error: String(error),
    });
    return null;
  }
}

function normalizeWhitespace(value: string): string {
  return value.replace(/\r\n/g, "\n").replace(/[ \t]+\n/g, "\n").trim();
}

function collapseForSnippet(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function flattenStructuredPdfText(data: StructuredTextData): string {
  const blocks = [...data.blocks].sort(
    (left, right) =>
      left.bbox.y - right.bbox.y ||
      left.bbox.x - right.bbox.x ||
      left.bbox.w - right.bbox.w,
  );
  const parts: string[] = [];
  let previousLineY: number | null = null;
  let previousLineHeight = 0;

  for (const block of blocks) {
    const lines = [...block.lines].sort(
      (left, right) =>
        left.bbox.y - right.bbox.y || left.bbox.x - right.bbox.x || left.x - right.x,
    );
    for (const line of lines) {
      const text = line.text.trim();
      if (!text) continue;

      if (parts.length > 0) {
        const paragraphGap =
          previousLineY == null
            ? false
            : line.bbox.y - previousLineY >
              Math.max(8, Math.max(previousLineHeight, line.bbox.h || 0) * 1.3);
        parts.push(paragraphGap ? "\n\n" : "\n");
      }

      parts.push(text);
      previousLineY = line.bbox.y;
      previousLineHeight = line.bbox.h || previousLineHeight;
    }
  }

  return parts.join("");
}

function isPdfPath(path: string): boolean {
  return /\.pdf$/i.test(path);
}

function isDocxPath(path: string): boolean {
  return /\.docx$/i.test(path);
}

function isGenericTextAttachment(path: string): boolean {
  return /\.(txt|md|markdown|csv|tsv|json|ya?ml|xml|html?)$/i.test(path);
}

function sliceAtBoundary(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value.trim();
  const slice = value.slice(0, maxChars);
  const lastBreak = Math.max(slice.lastIndexOf("\n"), slice.lastIndexOf(" "));
  if (lastBreak > maxChars * 0.6) {
    return slice.slice(0, lastBreak).trim();
  }
  return slice.trim();
}

function splitParagraphs(text: string): IngestedResourceSegment[] {
  return normalizeWhitespace(text)
    .split(/\n{2,}/)
    .map((paragraph) => paragraph.trim())
    .filter((paragraph) => paragraph.length > 0)
    .map((paragraph, index) => ({
      label: `Paragraph ${index + 1}`,
      text: paragraph,
    }));
}

function buildExcerpt(
  segments: IngestedResourceSegment[],
  maxChars: number,
  intro: string,
): string {
  if (segments.length === 0) {
    return intro;
  }

  const chunks: string[] = [];
  let remaining = maxChars;
  for (const segment of segments) {
    if (remaining <= 0) break;
    const header = `${segment.label}:\n`;
    const budget = Math.max(0, remaining - header.length);
    if (budget <= 24) break;
    const excerpt = sliceAtBoundary(segment.text, budget);
    if (!excerpt) continue;
    chunks.push(`${header}${excerpt}`);
    remaining -= header.length + excerpt.length + 2;
  }

  if (chunks.length === 0) {
    return intro;
  }

  return `${intro}\n${chunks.join("\n\n")}`;
}

function deriveSearchNeedles(query: string): string[] {
  const trimmed = query.trim();
  if (!trimmed) return [];

  const quoted = Array.from(trimmed.matchAll(/["“](.+?)["”]/g))
    .map((match) => match[1]?.trim())
    .filter((value): value is string => Boolean(value && value.length >= 2));

  const cjkTerms = trimmed.match(/[\u3400-\u9fff]{2,}/g) ?? [];
  const cjkNgrams = cjkTerms.flatMap((term) => {
    const chars = Array.from(term);
    if (chars.length <= 4) return [term];
    const grams: string[] = [term];
    for (let size = 4; size >= 2; size -= 1) {
      for (let index = 0; index <= chars.length - size; index += 1) {
        grams.push(chars.slice(index, index + size).join(""));
      }
    }
    return grams;
  });
  const englishTerms = (trimmed.toLowerCase().match(/[a-z0-9][a-z0-9_-]{2,}/g) ?? [])
    .filter((term) => !ENGLISH_STOP_WORDS.has(term));

  const phraseTerm =
    englishTerms.length >= 2 && englishTerms.length <= 6
      ? [englishTerms.join(" ")]
      : [];

  return Array.from(
    new Set(
      [...quoted, ...cjkTerms, ...phraseTerm, ...englishTerms]
        .concat(cjkNgrams)
        .map((term) => term.trim())
        .filter((term) => term.length >= 2),
    ),
  );
}

function makeSnippet(text: string, index: number, needleLength: number): string {
  const start = Math.max(0, index - Math.floor(RESOURCE_MATCH_SNIPPET_CHARS / 2));
  const end = Math.min(
    text.length,
    index + needleLength + Math.floor(RESOURCE_MATCH_SNIPPET_CHARS / 2),
  );
  const prefix = start > 0 ? "..." : "";
  const suffix = end < text.length ? "..." : "";
  return `${prefix}${collapseForSnippet(text.slice(start, end))}${suffix}`;
}

export function findRelevantResourceMatches(
  resource: IngestedProjectResource,
  query: string,
  limit = RESOURCE_MATCH_LIMIT,
): IngestedResourceMatch[] {
  const needles = deriveSearchNeedles(query);
  if (needles.length === 0) return [];

  const scored = resource.segments
    .map((segment) => {
      const haystack = segment.text.toLowerCase();
      let score = 0;
      let bestIndex = -1;
      let bestNeedleLength = 0;

      for (const needle of needles) {
        const normalizedNeedle = needle.toLowerCase();
        let from = 0;
        let localHits = 0;
        while (from < haystack.length) {
          const found = haystack.indexOf(normalizedNeedle, from);
          if (found === -1) break;
          localHits += 1;
          if (bestIndex === -1) {
            bestIndex = found;
            bestNeedleLength = normalizedNeedle.length;
          }
          from = found + normalizedNeedle.length;
        }

        if (localHits > 0) {
          const weight =
            normalizedNeedle.includes(" ") || /[\u3400-\u9fff]/.test(normalizedNeedle)
              ? 6
              : 2;
          score += localHits * weight;
        }
      }

      if (score <= 0 || bestIndex < 0) return null;
      return {
        label: segment.label,
        snippet: makeSnippet(segment.text, bestIndex, bestNeedleLength),
        score,
      } satisfies IngestedResourceMatch;
    })
    .filter((match): match is IngestedResourceMatch => Boolean(match))
    .sort((left, right) => right.score - left.score)
    .slice(0, limit);

  return scored;
}

export function formatRelevantResourceEvidence(
  groups: ResourceEvidenceGroup[],
): string[] {
  const normalized = groups
    .filter((group) => group.matches.length > 0)
    .map((group) => ({
      ...group,
      bestScore: Math.max(...group.matches.map((match) => match.score)),
    }))
    .sort((left, right) => right.bestScore - left.bestScore);

  if (normalized.length === 0) {
    return [];
  }

  const lines = ["[Relevant resource evidence:"];
  for (const group of normalized) {
    const sourceSuffix = group.sourceType ? ` (${group.sourceType})` : "";
    lines.push(`- Document: ${group.filePath}${sourceSuffix}`);
    for (const match of group.matches) {
      lines.push(`  - ${match.label}: ${match.snippet}`);
    }
  }
  lines.push("]");
  return lines;
}

async function ingestPdfResource(file: ProjectFile): Promise<IngestedProjectResource> {
  const raw = await readBinaryFile(file.absolutePath);
  const buffer = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);
  const client = getMupdfClient();
  const docId = await client.openDocument(buffer);

  try {
    const pageCount = await client.countPages(docId);
    const segments: IngestedResourceSegment[] = [];
    for (let pageIndex = 0; pageIndex < pageCount; pageIndex += 1) {
      const text = normalizeWhitespace(
        flattenStructuredPdfText(await client.getPageText(docId, pageIndex)),
      );
      if (!text) continue;
      segments.push({
        label: `Page ${pageIndex + 1}`,
        text,
      });
    }

    const excerptPages = segments.slice(0, PDF_EXCERPT_MAX_PAGES);
    const excerpt =
      excerptPages.length > 0
        ? buildExcerpt(
            excerptPages,
            PDF_EXCERPT_MAX_CHARS,
            `[Attached PDF excerpt from ${file.relativePath}]`,
          )
        : `[Attached PDF: ${file.relativePath}] No extractable text was found.`;
    const resource: IngestedProjectResource = {
      filePath: file.relativePath,
      absolutePath: file.absolutePath,
      sourceType: "pdf",
      kind: "pdf_document",
      extractionStatus:
        segments.length === 0
          ? "image_only"
          : segments.length < pageCount
            ? "partial"
            : "ready",
      excerpt,
      searchableText: segments.map((segment) => segment.text).join("\n\n"),
      segments,
      pageCount,
      metadata: {
        pagesRead: pageCount,
        pagesWithText: segments.length,
      },
    };
    resource.artifactPath = await persistResourceArtifact(file, resource).catch((error) => {
      log.warn("Failed to persist PDF resource artifact", {
        filePath: file.relativePath,
        error: String(error),
      });
      return undefined;
    });
    return resource;
  } finally {
    await client.closeDocument(docId).catch(() => {});
  }
}

async function ingestDocxResource(file: ProjectFile): Promise<IngestedProjectResource> {
  const raw = await readBinaryFile(file.absolutePath);
  const arrayBuffer = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);
  const rawTextExtractor = (
    mammoth as unknown as {
      extractRawText?: (input: { arrayBuffer: ArrayBuffer }) => Promise<{ value?: string }>;
    }
  ).extractRawText;
  const result = rawTextExtractor
    ? await rawTextExtractor({ arrayBuffer })
    : await mammoth.convertToHtml({ arrayBuffer });
  const segments = splitParagraphs(result.value ?? "");
  const excerpt =
    segments.length > 0
      ? buildExcerpt(segments, DOCX_EXCERPT_MAX_CHARS, `[Attached DOCX excerpt from ${file.relativePath}]`)
      : `[Attached DOCX: ${file.relativePath}] No extractable text was found.`;
  const resource: IngestedProjectResource = {
    filePath: file.relativePath,
    absolutePath: file.absolutePath,
    sourceType: "docx",
    kind: "docx_document",
    extractionStatus: segments.length > 0 ? "ready" : "failed",
    excerpt,
    searchableText: segments.map((segment) => segment.text).join("\n\n"),
    segments,
  };
  resource.artifactPath = await persistResourceArtifact(file, resource).catch((error) => {
    log.warn("Failed to persist DOCX resource artifact", {
      filePath: file.relativePath,
      error: String(error),
    });
    return undefined;
  });
  return resource;
}

async function ingestGenericTextResource(
  file: ProjectFile,
): Promise<IngestedProjectResource | null> {
  let text = typeof file.content === "string" ? file.content : "";
  if (!text && isGenericTextAttachment(file.relativePath)) {
    const raw = await readBinaryFile(file.absolutePath);
    text = new TextDecoder("utf-8").decode(raw).slice(0, GENERIC_TEXT_MAX_CHARS);
  }
  const segments = splitParagraphs(text);
  if (segments.length === 0) return null;

  const resource: IngestedProjectResource = {
    filePath: file.relativePath,
    absolutePath: file.absolutePath,
    sourceType: file.type,
    kind: "text_file",
    extractionStatus: "ready",
    excerpt: buildExcerpt(segments, 4_000, `[Attached text excerpt from ${file.relativePath}]`),
    searchableText: segments.map((segment) => segment.text).join("\n\n"),
    segments,
  };
  resource.artifactPath = await persistResourceArtifact(file, resource).catch((error) => {
    log.warn("Failed to persist text resource artifact", {
      filePath: file.relativePath,
      error: String(error),
    });
    return undefined;
  });
  return resource;
}

async function ingestProjectResourceInternal(
  file: ProjectFile,
): Promise<IngestedProjectResource | null> {
  if (isPdfPath(file.relativePath) || file.type === "pdf") {
    return ingestPdfResource(file);
  }
  if (isDocxPath(file.relativePath)) {
    return ingestDocxResource(file);
  }
  return ingestGenericTextResource(file);
}

export async function ingestProjectResource(
  file: ProjectFile,
): Promise<IngestedProjectResource | null> {
  const cacheKey = file.absolutePath;
  const cached = resourceCache.get(cacheKey);
  if (cached) {
    return cached;
  }

  const pending = (async () => {
    const persisted = await loadPersistedResourceArtifact(file);
    if (persisted) {
      return persisted;
    }

    return ingestProjectResourceInternal(file);
  })().catch((error) => {
    log.warn("Failed to ingest attached resource", {
      filePath: file.relativePath,
      error: String(error),
    });
    resourceCache.delete(cacheKey);
    return null;
  });
  resourceCache.set(cacheKey, pending);
  return pending;
}

export async function findRelevantAttachmentMatches(
  query: string,
  options: { absolutePath?: string | null; file?: ProjectFile | null },
): Promise<IngestedResourceMatch[]> {
  const file = options.file;
  let resource: IngestedProjectResource | null = null;

  if (file) {
    resource = await ingestProjectResource(file);
  } else if (options.absolutePath) {
    const cached = resourceCache.get(options.absolutePath);
    resource = cached ? await cached : null;
  }

  if (!resource) return [];
  return findRelevantResourceMatches(resource, query);
}

export function clearResourceIngestionCache(): void {
  resourceCache.clear();
}
