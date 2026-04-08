import { type FC, useState } from "react";
import {
  BotIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CircleIcon,
  ClockIcon,
  FileEditIcon,
  FileIcon,
  FileOutputIcon,
  LoaderIcon,
  SparklesIcon,
  TerminalIcon,
  WrenchIcon,
} from "lucide-react";
import {
  adaptToolResultDisplayContent,
  getToolResultDisplayApproval,
  getToolResultDisplayPreview,
  type AgentToolResultDisplayContent,
} from "@/lib/agent-message-adapter";
import { type ContentBlock } from "@/stores/agent-chat-store";

interface ToolWidgetProps {
  toolUse: ContentBlock;
  toolResult?: ContentBlock;
  isStreaming?: boolean;
}

export const ToolWidget: FC<ToolWidgetProps> = ({
  toolUse,
  toolResult,
  isStreaming = false,
}) => {
  const name = toolUse.name?.toLowerCase() || "";

  if (name === "write_file") {
    return (
      <WriteWidget input={toolUse.input} result={toolResult} isStreaming={isStreaming} />
    );
  }
  if (name === "replace_selected_text" || name === "apply_text_patch") {
    return (
      <PreciseEditWidget
        toolName={toolUse.name || "edit"}
        input={toolUse.input}
        result={toolResult}
        isStreaming={isStreaming}
      />
    );
  }
  if (
    name === "read_file" ||
    name === "read_document" ||
    name === "read_document_excerpt" ||
    name === "inspect_resource"
  )
    return (
      <ReadWidget
        toolName={name}
        input={toolUse.input}
        result={toolResult}
        isStreaming={isStreaming}
      />
    );
  if (name === "run_shell_command")
    return <BashWidget input={toolUse.input} result={toolResult} isStreaming={isStreaming} />;
  if (name === "list_files")
    return <GlobWidget input={toolUse.input} result={toolResult} isStreaming={isStreaming} />;
  if (
    name === "search_project" ||
    name === "search_document_text" ||
    name === "get_document_evidence"
  )
    return (
      <GrepWidget
        toolName={name}
        input={toolUse.input}
        result={toolResult}
        isStreaming={isStreaming}
      />
    );
  if (
    name === "draft_section" ||
    name === "restructure_outline" ||
    name === "check_consistency" ||
    name === "generate_abstract" ||
    name === "insert_citation"
  ) {
    return (
      <WritingWidget
        toolName={name}
        input={toolUse.input}
        result={toolResult}
        isStreaming={isStreaming}
      />
    );
  }

  return (
    <GenericWidget
      name={toolUse.name || "unknown"}
      input={toolUse.input}
      result={toolResult}
      isStreaming={isStreaming}
    />
  );
};

const WritingWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const actionLabel =
    toolName === "draft_section"
      ? result
        ? "Drafted section"
        : "Drafting section"
      : toolName === "restructure_outline"
        ? result
          ? "Restructured outline"
          : "Restructuring outline"
        : toolName === "check_consistency"
          ? result
            ? "Checked consistency"
            : "Checking consistency"
          : toolName === "generate_abstract"
            ? result
              ? "Generated abstract"
              : "Generating abstract"
            : result
              ? "Inserted citation"
              : "Inserting citation";
  const context =
    input?.section_type ||
    input?.manuscript_type ||
    input?.path ||
    input?.citation_key ||
    "";
  const preview = getToolDisplay(result)?.textPreview ?? "";

  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <div className="flex items-center gap-2 px-3 py-2">
        <StatusIcon result={result} isStreaming={isStreaming} />
        <SparklesIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {actionLabel}
          {context ? (
            <>
              {" "}
              <code className="rounded bg-muted px-1 text-xs">{context}</code>
            </>
          ) : null}
        </span>
      </div>
      {!!preview && (
        <div className="border-border border-t px-3 py-2">
          <pre
            className={`max-h-40 overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
              result?.is_error
                ? "bg-destructive/10 text-destructive"
                : "bg-background/60 text-foreground"
            }`}
          >
            {truncate(preview, 1200)}
          </pre>
        </div>
      )}
    </div>
  );
};

// ─── Status Icon ───

const StatusIcon: FC<{ result?: ContentBlock; isStreaming?: boolean }> = ({
  result,
  isStreaming = false,
}) => {
  const display = getToolDisplay(result);
  if (!result) {
    if (!isStreaming) {
      // Tool was cancelled (stop pressed) — show stopped state
      return <CircleIcon className="size-3.5 text-muted-foreground" />;
    }
    return (
      <LoaderIcon className="size-3.5 animate-spin text-muted-foreground" />
    );
  }
  if (display?.status === "review_ready" || display?.status === "awaiting_approval") {
    return <ClockIcon className="size-3.5 text-amber-500" />;
  }
  if (result.is_error) {
    return <span className="text-destructive text-sm">!</span>;
  }
  return <CheckIcon className="size-3.5 text-green-600" />;
};

// ─── Write Widget ───

const WriteWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({
  input,
  result,
  isStreaming,
}) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const approval = getApprovalPayload(result);
  const statusLabel = approval
    ? approval.reviewReady
      ? "Review ready for"
      : "Approval required for"
    : result
      ? "Wrote"
      : "Writing";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileOutputIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-muted-foreground">
          {statusLabel}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
        </div>
        {approval ? (
          <div className="mt-1 text-xs text-muted-foreground/80">
            {approval.reviewReady
              ? "Review is ready in the diff panel."
              : "Approval controls are shown in the chat panel."}
          </div>
        ) : null}
      </div>
    </div>
  );
};

const PreciseEditWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ toolName, input, result, isStreaming }) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const approval = getApprovalPayload(result);
  const statusLabel = approval
    ? approval.reviewReady
      ? "Review ready for"
      : "Approval required for"
    : result
      ? "Edited"
      : "Editing";

  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileEditIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-muted-foreground">
          {statusLabel}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
        </div>
        <div className="mt-1 text-xs text-muted-foreground/80">
          {toolName === "replace_selected_text"
            ? "Selection-scoped edit"
            : toolName === "apply_text_patch"
              ? "Exact text patch"
              : "Precise file edit"}
        </div>
        {approval ? (
          <div className="mt-1 text-xs text-muted-foreground/80">
            {approval.reviewReady
              ? "Review is ready in the diff panel."
              : "Approval controls are shown in the chat panel."}
          </div>
        ) : null}
      </div>
    </div>
  );
};

// ─── Read Widget ───

const ReadWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({
  toolName,
  input,
  result,
  isStreaming,
}) => {
  const targetPath = input?.file_path || input?.path || "(missing path)";
  const preview = getToolDisplay(result)?.textPreview ?? "";
  const [expanded, setExpanded] = useState(false);
  const verb =
    toolName === "read_document"
      ? result
        ? "Read document"
        : "Reading document"
      : toolName === "inspect_resource"
      ? result
        ? "Inspected"
        : "Inspecting"
      : toolName === "read_document_excerpt"
        ? result
          ? "Read excerpt from"
          : "Reading excerpt from"
        : result
          ? "Read"
          : "Reading";
  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {verb}{" "}
          <code className="rounded bg-muted px-1 text-xs">{targetPath}</code>
        </span>
        {expanded ? (
          <ChevronDownIcon className="ml-auto size-3.5 text-muted-foreground" />
        ) : (
          <ChevronRightIcon className="ml-auto size-3.5 text-muted-foreground" />
        )}
      </button>
      {expanded && (
        <div className="space-y-2 border-border border-t px-3 py-2">
          <pre className="whitespace-pre-wrap font-mono text-muted-foreground text-xs">
            {JSON.stringify(input ?? {}, null, 2)}
          </pre>
          {!!preview && (
            <pre
              className={`max-h-40 overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
                result?.is_error
                  ? "bg-destructive/10 text-destructive"
                  : "bg-background/60 text-foreground"
              }`}
            >
              {truncate(preview, 2000)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
};

// ─── Bash Widget ───

const BashWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({
  input,
  result,
  isStreaming,
}) => {
  const [expanded, setExpanded] = useState(false);
  const command = input?.command || input?.description || "";
  const resultContent = getToolDisplay(result)?.textPreview ?? "";
  const approval = getApprovalPayload(result);

  return (
    <div className="my-1.5 rounded-lg border border-border bg-[#1e1e2e] text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <TerminalIcon className="size-3.5 shrink-0 text-green-400" />
        <code className="min-w-0 truncate text-green-300 text-xs">
          $ {truncate(command, 80)}
        </code>
        {result &&
          (expanded ? (
            <ChevronDownIcon className="ml-auto size-3.5 text-muted-foreground" />
          ) : (
            <ChevronRightIcon className="ml-auto size-3.5 text-muted-foreground" />
          ))}
      </button>
      {expanded && (resultContent || approval) && (
        <div className="max-h-40 overflow-auto border-border/50 border-t px-3 py-2">
          {!!resultContent && (
            <pre className="whitespace-pre-wrap font-mono text-gray-300 text-xs">
              {truncate(resultContent, 2000)}
            </pre>
          )}
          {approval ? (
            <div className="mt-2 text-xs text-gray-400">
              Approval controls are shown in the chat panel.
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
};

// ─── Glob Widget ───

const GlobWidget: FC<{
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({
  input,
  result,
  isStreaming,
}) => {
  const pattern = input?.pattern || input?.path || ".";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
      <StatusIcon result={result} isStreaming={isStreaming} />
      <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 truncate text-muted-foreground">
        {result ? "Searched" : "Searching"}{" "}
        <code className="rounded bg-muted px-1 text-xs">{pattern}</code>
      </span>
    </div>
  );
};

// ─── Grep Widget ───

const GrepWidget: FC<{
  toolName: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({
  toolName,
  input,
  result,
  isStreaming,
}) => {
  const pattern = input?.pattern || input?.query || "(missing query)";
  const verb =
    toolName === "get_document_evidence"
      ? result
        ? "Gathered evidence for"
        : "Gathering evidence for"
      : toolName === "search_document_text"
        ? result
          ? "Searched document for"
          : "Searching document for"
        : result
          ? "Grepped"
          : "Grepping";
  return (
    <div className="my-1.5 flex items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 py-2 text-sm">
        <StatusIcon result={result} isStreaming={isStreaming} />
        <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate text-muted-foreground">
          {verb}{" "}
          <code className="rounded bg-muted px-1 text-xs">{pattern}</code>
        </span>
    </div>
  );
};

// ─── Generic Widget ───

const GenericWidget: FC<{
  name: string;
  input: any;
  result?: ContentBlock;
  isStreaming?: boolean;
}> = ({ name, input, result, isStreaming }) => {
  const [expanded, setExpanded] = useState(false);
  const resultPreview = extractResultTextPreview(result);
  const display = getToolDisplay(result);

  return (
    <div className="my-1.5 rounded-lg border border-border bg-muted/50 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <StatusIcon result={result} isStreaming={isStreaming} />
        <WrenchIcon className="size-3.5 text-muted-foreground" />
        <span className="text-muted-foreground">
          {result ? "Ran" : "Running"} <code className="text-xs">{name}</code>
        </span>
        {expanded ? (
          <ChevronDownIcon className="ml-auto size-3.5 text-muted-foreground" />
        ) : (
          <ChevronRightIcon className="ml-auto size-3.5 text-muted-foreground" />
        )}
      </button>
      {expanded && (
        <div className="space-y-2 border-border border-t px-3 py-2">
          <pre className="whitespace-pre-wrap font-mono text-muted-foreground text-xs">
            {JSON.stringify(input ?? {}, null, 2)}
          </pre>
          {!!resultPreview && (
            <pre
              className={`max-h-40 overflow-auto whitespace-pre-wrap rounded-md px-2 py-1.5 font-mono text-xs ${
                display?.isError
                  ? "bg-destructive/10 text-destructive"
                  : "bg-background/60 text-foreground"
              }`}
            >
              {truncate(resultPreview, 2000)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
};

// ─── Thinking Widget ───

export const ThinkingWidget: FC<{ thinking: string; signature?: string }> = ({
  thinking,
}) => {
  const [expanded, setExpanded] = useState(false);
  const trimmed = thinking.trim();

  return (
    <div className="my-1.5 overflow-hidden rounded-lg border border-muted-foreground/20 bg-muted-foreground/5">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between px-3 py-2 transition-colors hover:bg-muted-foreground/10"
      >
        <div className="flex items-center gap-2">
          <div className="relative">
            <BotIcon className="size-4 text-muted-foreground" />
            <SparklesIcon className="absolute -top-1 -right-1 size-2.5 animate-pulse text-muted-foreground/70" />
          </div>
          <span className="font-medium text-muted-foreground text-sm italic">
            Thinking...
          </span>
        </div>
        <ChevronRightIcon
          className={`size-4 text-muted-foreground transition-transform ${expanded ? "rotate-90" : ""}`}
        />
      </button>
      {expanded && (
        <div className="border-muted-foreground/20 border-t px-3 pt-2 pb-3">
          <pre className="whitespace-pre-wrap rounded-lg bg-muted-foreground/5 p-3 font-mono text-muted-foreground text-xs italic">
            {trimmed}
          </pre>
        </div>
      )}
    </div>
  );
};

// ─── Helpers ───

function isApprovalRequired(result?: ContentBlock): boolean {
  return getApprovalPayload(result) !== null;
}

function getToolDisplay(
  result?: ContentBlock,
): AgentToolResultDisplayContent | null {
  if (!result) return null;
  return adaptToolResultDisplayContent(result.content, {
    preview: typeof result.content === "string" ? result.content : undefined,
    isError: result.is_error === true,
  });
}

function getApprovalPayload(result?: ContentBlock): {
  reason?: string;
  reviewReady: boolean;
  approvalToolName?: string;
} | null {
  if (!result?.is_error) {
    return null;
  }
  return getToolResultDisplayApproval(result.content);
}

function extractResultTextPreview(result?: ContentBlock): string {
  return getToolDisplay(result)?.textPreview ?? getToolResultDisplayPreview(result?.content);
}

function truncate(str: string, max: number): string {
  if (!str) return "";
  return str.length > max ? `${str.slice(0, max)}...` : str;
}
