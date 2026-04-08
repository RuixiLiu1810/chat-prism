export interface AgentToolResultDisplayContent {
  kind: "tool_result_display";
  displayKind?: string;
  toolName?: string;
  status?: string;
  textPreview: string;
  isError: boolean;
  targetPath?: string;
  command?: string;
  query?: string;
  approvalRequired: boolean;
  reviewReady: boolean;
  approvalReason?: string;
  approvalToolName?: string;
  written?: boolean;
  summary?: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readStringField(
  record: Record<string, unknown>,
  keys: string[],
): string | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value.trim();
    }
  }
  return undefined;
}

function readNestedStringField(
  record: Record<string, unknown>,
  parentKey: string,
  childKeys: string[],
): string | undefined {
  const parent = record[parentKey];
  if (!isRecord(parent)) {
    return undefined;
  }
  return readStringField(parent, childKeys);
}

function truncatePreview(value: string, maxChars = 300): string {
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars)}...`;
}

function deriveStructuredPreview(record: Record<string, unknown>): string | undefined {
  const summary = readStringField(record, ["summary"]);
  if (summary) {
    return summary;
  }

  const draft = readStringField(record, ["draft", "abstract"]);
  if (draft) {
    return truncatePreview(draft);
  }

  const findings = record.findings;
  if (Array.isArray(findings) && findings.length > 0) {
    const topMessages = findings
      .slice(0, 3)
      .map((entry) =>
        isRecord(entry) ? readStringField(entry, ["message"]) : undefined,
      )
      .filter((value): value is string => typeof value === "string");
    if (topMessages.length > 0) {
      return `Consistency findings: ${topMessages.join(" | ")}`;
    }
  }

  const revisedOutline = record.revisedOutline;
  if (Array.isArray(revisedOutline) && revisedOutline.length > 0) {
    return `Restructured outline with ${revisedOutline.length} sections.`;
  }

  return undefined;
}

export function stripPseudoToolCallMarkup(content: string): string {
  if (!content.includes("<tool_call>") && !content.includes("[TOOL_CALL]")) {
    return content;
  }

  let sanitized = content
    .replace(/\s*<tool_call>[\s\S]*?<\/tool_call>\s*/g, "\n")
    .replace(/\s*\[TOOL_CALL\][\s\S]*?\[\/TOOL_CALL\]\s*/g, "\n");
  const danglingStart = sanitized.indexOf("<tool_call>");
  if (danglingStart !== -1) {
    sanitized = sanitized.slice(0, danglingStart);
  }
  const danglingBracketStart = sanitized.indexOf("[TOOL_CALL]");
  if (danglingBracketStart !== -1) {
    sanitized = sanitized.slice(0, danglingBracketStart);
  }

  return sanitized
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{2,}/g, "\n")
    .trim();
}

export function isAgentToolResultDisplayContent(
  value: unknown,
): value is AgentToolResultDisplayContent {
  return isRecord(value) && value.kind === "tool_result_display";
}

export function adaptToolResultDisplayContent(
  content: unknown,
  options?: {
    preview?: string;
    isError?: boolean;
  },
): AgentToolResultDisplayContent {
  if (isAgentToolResultDisplayContent(content)) {
    return content;
  }

  if (typeof content === "string") {
    return {
      kind: "tool_result_display",
      textPreview: content,
      isError: Boolean(options?.isError),
      approvalRequired: false,
      reviewReady: false,
    };
  }

  if (!isRecord(content)) {
    return {
      kind: "tool_result_display",
      textPreview: options?.preview ?? "",
      isError: Boolean(options?.isError),
      approvalRequired: false,
      reviewReady: false,
    };
  }

  const reviewArtifactPayload = isRecord(content.reviewArtifactPayload)
    ? content.reviewArtifactPayload
    : null;
  const targetPath =
    readStringField(content, ["path", "file_path", "targetPath", "filePath"]) ??
    (reviewArtifactPayload
      ? readStringField(reviewArtifactPayload, ["targetPath"])
      : undefined);
  const command =
    readStringField(content, ["command"]) ??
    readNestedStringField(content, "input", ["command"]);
  const query =
    readStringField(content, ["query"]) ??
    readNestedStringField(content, "input", ["query"]);
  const approvalRequired = content.approvalRequired === true;
  const reviewReady =
    content.reviewArtifact === true ||
    (reviewArtifactPayload
      ? readStringField(reviewArtifactPayload, ["targetPath"]) !== undefined
      : false);
  const errorText = readStringField(content, ["error"]);
  const contentText = readStringField(content, ["content"]);
  const approvalReason = readStringField(content, ["reason"]);
  const summary =
    (reviewArtifactPayload
      ? readStringField(reviewArtifactPayload, ["summary"])
      : undefined) ?? readStringField(content, ["summary"]);

  const derivedStructured = deriveStructuredPreview(content);

  return {
    kind: "tool_result_display",
    displayKind: readStringField(content, ["displayKind"]),
    toolName: readStringField(content, ["toolName"]),
    status: readStringField(content, ["status"]),
    textPreview:
      errorText ??
      contentText ??
      approvalReason ??
      summary ??
      derivedStructured ??
      options?.preview ??
      "",
    isError: Boolean(options?.isError),
    targetPath,
    command,
    query,
    approvalRequired,
    reviewReady,
    approvalReason,
    approvalToolName: readStringField(content, [
      "approvalToolName",
      "toolName",
    ]),
    written:
      typeof content.written === "boolean" ? content.written : undefined,
    summary,
  };
}

export function getToolResultDisplayPreview(content: unknown): string {
  return adaptToolResultDisplayContent(content).textPreview;
}

export function getToolResultDisplayTarget(content: unknown): string | null {
  const adapted = adaptToolResultDisplayContent(content);
  return adapted.targetPath ?? adapted.command ?? adapted.query ?? null;
}

export function getToolResultDisplayApproval(content: unknown): {
  reason?: string;
  reviewReady: boolean;
  approvalToolName?: string;
} | null {
  const adapted = adaptToolResultDisplayContent(content);
  if (!adapted.approvalRequired) {
    return null;
  }
  return {
    reason: adapted.approvalReason,
    reviewReady: adapted.reviewReady,
    approvalToolName: adapted.approvalToolName,
  };
}

export function adaptAgentStreamMessageForUi<
  T extends { message?: { content?: unknown } },
>(message: T): T {
  const blocks = message.message?.content;
  if (!Array.isArray(blocks)) {
    return message;
  }

  let changed = false;
  const nextBlocks = blocks.map((block) => {
    if (!isRecord(block) || block.type !== "tool_result") {
      return block;
    }
    const adaptedContent = adaptToolResultDisplayContent(block.content, {
      preview: typeof block.content === "string" ? block.content : undefined,
      isError: block.is_error === true,
    });
    if (block.content === adaptedContent) {
      return block;
    }
    changed = true;
    return {
      ...block,
      content: adaptedContent,
    };
  });

  if (!changed) {
    return message;
  }

  return {
    ...message,
    message: {
      ...message.message,
      content: nextBlocks,
    },
  };
}
