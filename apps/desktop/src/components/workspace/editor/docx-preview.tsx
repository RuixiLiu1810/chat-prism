import { useEffect, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import * as mammoth from "mammoth";
import { FileDownIcon, Loader2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ProjectFile } from "@/stores/document-store";

interface DocxPreviewProps {
  file: ProjectFile;
  onImportEditable?: (content: string) => Promise<void>;
}

function normalizeSpaces(value: string) {
  return value.replace(/\s+/g, " ").trim();
}

function convertInlineNodeToMarkdown(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? "";
  }
  if (node.nodeType !== Node.ELEMENT_NODE) {
    return "";
  }
  const element = node as HTMLElement;
  const tag = element.tagName.toLowerCase();
  const content = Array.from(element.childNodes)
    .map((child) => convertInlineNodeToMarkdown(child))
    .join("");
  const clean = normalizeSpaces(content);

  if (tag === "strong" || tag === "b") {
    return clean ? `**${clean}**` : "";
  }
  if (tag === "em" || tag === "i") {
    return clean ? `*${clean}*` : "";
  }
  if (tag === "code") {
    return clean ? `\`${clean}\`` : "";
  }
  if (tag === "br") {
    return "  \n";
  }
  if (tag === "a") {
    const href = element.getAttribute("href")?.trim();
    if (!clean) return href ?? "";
    if (!href) return clean;
    return `[${clean}](${href})`;
  }
  return content;
}

function convertListToMarkdown(element: HTMLElement, ordered: boolean) {
  const items = Array.from(element.children).filter(
    (child) => child.tagName.toLowerCase() === "li",
  );
  return items
    .map((item, index) => {
      const text = normalizeSpaces(
        Array.from(item.childNodes)
          .map((child) => convertInlineNodeToMarkdown(child))
          .join(""),
      );
      if (!text) return "";
      return ordered ? `${index + 1}. ${text}` : `- ${text}`;
    })
    .filter((line) => line.length > 0)
    .join("\n");
}

function convertTableToMarkdown(element: HTMLElement) {
  const rows = Array.from(element.querySelectorAll("tr"))
    .map((row) =>
      Array.from(row.querySelectorAll("th,td"))
        .map((cell) => normalizeSpaces(cell.textContent ?? ""))
        .filter((text) => text.length > 0),
    )
    .filter((row) => row.length > 0);
  if (rows.length === 0) return "";

  const header = rows[0];
  const body = rows.slice(1);
  const headerLine = `| ${header.join(" | ")} |`;
  const separator = `| ${header.map(() => "---").join(" | ")} |`;
  const bodyLines = body.map((row) => `| ${row.join(" | ")} |`).join("\n");
  return bodyLines ? `${headerLine}\n${separator}\n${bodyLines}` : headerLine;
}

function convertBlockNodeToMarkdown(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return normalizeSpaces(node.textContent ?? "");
  }
  if (node.nodeType !== Node.ELEMENT_NODE) {
    return "";
  }
  const element = node as HTMLElement;
  const tag = element.tagName.toLowerCase();

  if (/^h[1-6]$/.test(tag)) {
    const level = Number(tag.charAt(1));
    const title = normalizeSpaces(
      Array.from(element.childNodes)
        .map((child) => convertInlineNodeToMarkdown(child))
        .join(""),
    );
    return title ? `${"#".repeat(level)} ${title}` : "";
  }
  if (tag === "p") {
    return normalizeSpaces(
      Array.from(element.childNodes)
        .map((child) => convertInlineNodeToMarkdown(child))
        .join(""),
    );
  }
  if (tag === "ul") return convertListToMarkdown(element, false);
  if (tag === "ol") return convertListToMarkdown(element, true);
  if (tag === "pre") {
    const text = (element.textContent ?? "").trim();
    return text ? `\`\`\`\n${text}\n\`\`\`` : "";
  }
  if (tag === "blockquote") {
    const text = normalizeSpaces(element.textContent ?? "");
    return text ? `> ${text}` : "";
  }
  if (tag === "table") return convertTableToMarkdown(element);
  if (tag === "hr") return "---";

  // Fallback for unknown block nodes: flatten text.
  return normalizeSpaces(
    Array.from(element.childNodes)
      .map((child) => convertInlineNodeToMarkdown(child))
      .join(""),
  );
}

function convertHtmlToMarkdown(html: string) {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const blocks = Array.from(parsed.body.childNodes)
    .map((node) => convertBlockNodeToMarkdown(node))
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  return blocks.join("\n\n");
}

export function DocxPreview({ file, onImportEditable }: DocxPreviewProps) {
  const [html, setHtml] = useState<string>("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      setError(null);
      setHtml("");
      setWarnings([]);

      try {
        const raw = await readFile(file.absolutePath);
        const arrayBuffer = raw.buffer.slice(
          raw.byteOffset,
          raw.byteOffset + raw.byteLength,
        );
        const result = await mammoth.convertToHtml(
          { arrayBuffer },
          {
            includeDefaultStyleMap: true,
            includeEmbeddedStyleMap: true,
          },
        );
        if (cancelled) return;

        setHtml(result.value || "");
        setWarnings(
          result.messages
            .map((message) => message.message)
            .filter((msg) => msg && msg.trim().length > 0),
        );
      } catch (err) {
        if (cancelled) return;
        setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [file.absolutePath]);

  const handleImportEditable = async () => {
    if (!onImportEditable || importing) return;
    setImportError(null);
    setImporting(true);
    try {
      const raw = await readFile(file.absolutePath);
      const arrayBuffer = raw.buffer.slice(
        raw.byteOffset,
        raw.byteOffset + raw.byteLength,
      );
      const result = await mammoth.convertToHtml(
        { arrayBuffer },
        {
          includeDefaultStyleMap: true,
          includeEmbeddedStyleMap: true,
        },
      );
      const htmlValue = result.value || "";
      let markdown = convertHtmlToMarkdown(htmlValue);
      if (!markdown.trim()) {
        const fallbackDoc = new DOMParser().parseFromString(htmlValue, "text/html");
        markdown = normalizeSpaces(fallbackDoc.body.textContent ?? "");
      }
      await onImportEditable(markdown);
    } catch (err) {
      setImportError(String(err));
    } finally {
      setImporting(false);
    }
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
        Loading DOCX...
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center px-6 text-center text-muted-foreground text-sm">
        Failed to render DOCX: {error}
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-6">
      {onImportEditable && (
        <div className="mb-4 flex items-center justify-between rounded-md border border-border/60 bg-muted/30 px-3 py-2">
          <div className="text-muted-foreground text-xs">
            Convert this Word file into an editable Markdown copy.
          </div>
          <Button
            size="sm"
            variant="outline"
            className="h-7 gap-1.5 text-xs"
            disabled={importing}
            onClick={() => void handleImportEditable()}
          >
            {importing ? (
              <Loader2Icon className="size-3.5 animate-spin" />
            ) : (
              <FileDownIcon className="size-3.5" />
            )}
            {importing ? "Importing..." : "Import Editable Copy"}
          </Button>
        </div>
      )}
      {importError && (
        <div className="mb-4 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-destructive text-xs">
          Failed to import editable copy: {importError}
        </div>
      )}
      {warnings.length > 0 && (
        <div className="mb-4 rounded-md border border-border bg-muted/40 px-3 py-2 text-muted-foreground text-xs">
          Some formatting may not be fully preserved ({warnings.length} notice
          {warnings.length > 1 ? "s" : ""}).
        </div>
      )}
      {html.trim() ? (
        <div
          className="docx-preview-content text-sm leading-6 [&_a]:text-primary [&_img]:max-w-full [&_li]:my-1 [&_ol]:my-3 [&_ol]:pl-6 [&_p]:my-2 [&_table]:my-4 [&_table]:w-full [&_table]:border-collapse [&_td]:border [&_td]:border-border [&_td]:p-2 [&_th]:border [&_th]:border-border [&_th]:bg-muted/50 [&_th]:p-2 [&_ul]:my-3 [&_ul]:list-disc [&_ul]:pl-6"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <div className="text-muted-foreground text-sm">DOCX has no visible text.</div>
      )}
    </div>
  );
}
