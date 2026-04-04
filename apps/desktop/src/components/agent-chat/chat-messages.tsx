import { type FC, memo, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircleIcon } from "lucide-react";
import {
  useAgentChatStore,
  type AgentStreamMessage,
  type ContentBlock,
} from "@/stores/agent-chat-store";
import { useSettingsStore } from "@/stores/settings-store";
import { MarkdownRenderer } from "./markdown-renderer";
import { ThinkingWidget, ToolWidget } from "./tool-widgets";

const LEGACY_DOCUMENT_TOOL_NAMES = new Set([
  "inspect_resource",
  "read_document_excerpt",
  "search_document_text",
  "get_document_evidence",
]);

function normalizedToolName(block: ContentBlock): string {
  return (block.name || "").toLowerCase();
}

function isAnyDocumentToolUse(block: ContentBlock): boolean {
  if (block.type !== "tool_use") {
    return false;
  }
  const name = normalizedToolName(block);
  return name === "read_document" || LEGACY_DOCUMENT_TOOL_NAMES.has(name);
}

function hasMeaningfulTextBlock(blocks: ContentBlock[]): boolean {
  return blocks.some(
    (block) => block.type === "text" && typeof block.text === "string" && block.text.trim().length > 0,
  );
}

function isDocumentTraceOnlyAssistantMessage(message: AgentStreamMessage): boolean {
  if (message.type !== "assistant") {
    return false;
  }
  const content = message.message?.content;
  if (!Array.isArray(content) || content.length === 0) {
    return false;
  }

  let hasDocumentTool = false;
  for (const block of content) {
    if (block.type === "tool_use") {
      if (!isAnyDocumentToolUse(block)) {
        return false;
      }
      hasDocumentTool = true;
      continue;
    }
    if (block.type === "thinking") {
      continue;
    }
    if (block.type === "text") {
      continue;
    }
    return false;
  }

  return hasDocumentTool && !hasMeaningfulTextBlock(content);
}

function collapseDocumentTraceMessages(messages: AgentStreamMessage[]): AgentStreamMessage[] {
  const collapsed: AgentStreamMessage[] = [];
  for (const message of messages) {
    if (isDocumentTraceOnlyAssistantMessage(message)) {
      const last = collapsed[collapsed.length - 1];
      if (last && isDocumentTraceOnlyAssistantMessage(last)) {
        collapsed[collapsed.length - 1] = message;
        continue;
      }
    }
    collapsed.push(message);
  }
  return collapsed;
}

function pickRepresentativeDocumentToolUse(
  content: ContentBlock[],
  toolResultMap: Map<string, ContentBlock>,
): ContentBlock | null {
  const documentBlocks = content.filter(
    (block) => block.type === "tool_use" && isAnyDocumentToolUse(block),
  );
  if (documentBlocks.length === 0) {
    return null;
  }

  const withResult = [...documentBlocks]
    .reverse()
    .find((block) => block.id && toolResultMap.has(block.id));
  const representative = withResult ?? documentBlocks[documentBlocks.length - 1];
  return {
    ...representative,
    type: "tool_use",
    name: "read_document",
  };
}

// ─── Streaming Indicator (isolated to prevent re-render storms) ───

const StreamingIndicator: FC = memo(() => {
  const [elapsed, setElapsed] = useState(0);
  const startRef = useRef(Date.now());

  useEffect(() => {
    startRef.current = Date.now();
    setElapsed(0);
    const timer = setInterval(() => {
      setElapsed(Math.floor((Date.now() - startRef.current) / 1000));
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  return (
    <div className="flex items-center gap-1.5 px-1 py-1.5 text-muted-foreground">
      <div className="flex gap-0.5">
        <span
          className="size-1.5 animate-bounce rounded-full bg-muted-foreground/50"
          style={{ animationDelay: "0ms" }}
        />
        <span
          className="size-1.5 animate-bounce rounded-full bg-muted-foreground/50"
          style={{ animationDelay: "150ms" }}
        />
        <span
          className="size-1.5 animate-bounce rounded-full bg-muted-foreground/50"
          style={{ animationDelay: "300ms" }}
        />
      </div>
      <span className="text-sm">
        Thinking...
        {elapsed >= 3 && (
          <span className="ml-1 text-muted-foreground/60 text-xs">
            {elapsed}s
          </span>
        )}
      </span>
    </div>
  );
});

// ─── Chat Messages (main component) ───

export const ChatMessages: FC = () => {
  const messages = useAgentChatStore((s) => s.messages) ?? [];
  const isStreaming = useAgentChatStore((s) => s.isStreaming);
  const debugEnabled = useSettingsStore(
    (s) => s.effective.advanced.debugEnabled,
  );
  const viewportRef = useRef<HTMLDivElement>(null);
  const shouldAutoScrollRef = useRef(true);
  const userHasScrolledRef = useRef(false);

  // Build a map of tool_use_id → tool_result for inline display
  const toolResultMap = useMemo(() => {
    const map = new Map<string, ContentBlock>();
    for (const msg of messages) {
      if (msg.type === "user" && Array.isArray(msg.message?.content)) {
        for (const block of msg.message.content) {
          if (block.type === "tool_result" && block.tool_use_id) {
            map.set(block.tool_use_id, block);
          }
        }
      }
    }
    return map;
  }, [messages]);

  // Filter displayable messages
  const displayMessages = useMemo(() => {
    const filtered = messages.filter((msg) => {
      if (msg.type === "system" && msg.subtype === "init") return false;
      if (
        msg.type !== "user" &&
        msg.type !== "assistant" &&
        msg.type !== "result"
      )
        return false;
      if (msg.type === "user" && msg.message?.content) {
        if (Array.isArray(msg.message.content)) {
          const hasOnlyToolResults = msg.message.content.every(
            (b: any) => b.type === "tool_result",
          );
          if (hasOnlyToolResults) return false;
        }
      }
      return true;
    });
    if (debugEnabled) {
      return filtered;
    }
    return collapseDocumentTraceMessages(filtered);
  }, [messages, debugEnabled]);

  // Auto-scroll to bottom (only if user hasn't scrolled up)
  useEffect(() => {
    if (shouldAutoScrollRef.current && viewportRef.current) {
      viewportRef.current.scrollTo({
        top: viewportRef.current.scrollHeight,
        behavior: "smooth",
      });
    }
  }, [displayMessages]);

  // Reset auto-scroll when streaming stops
  useEffect(() => {
    if (!isStreaming) {
      shouldAutoScrollRef.current = true;
      userHasScrolledRef.current = false;
    }
  }, [isStreaming]);

  const handleScroll = () => {
    if (!viewportRef.current) return;
    const el = viewportRef.current;
    const isAtBottom =
      Math.abs(el.scrollHeight - el.scrollTop - el.clientHeight) < 50;
    if (!isAtBottom) {
      userHasScrolledRef.current = true;
      shouldAutoScrollRef.current = false;
    } else if (userHasScrolledRef.current) {
      shouldAutoScrollRef.current = true;
      userHasScrolledRef.current = false;
    }
  };

  return (
    <div
      ref={viewportRef}
      onScroll={handleScroll}
      className="absolute inset-0 overflow-y-auto scroll-smooth px-4 py-2"
    >
      {displayMessages.length === 0 && !isStreaming && (
        <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
          Ask the agent about your LaTeX document...
        </div>
      )}

      {displayMessages.map((msg, idx) => (
        <MessageBubble
          key={idx}
          message={msg}
          toolResultMap={toolResultMap}
          isStreaming={isStreaming}
          debugEnabled={debugEnabled}
        />
      ))}

      {isStreaming && <StreamingIndicator />}
    </div>
  );
};

// ─── Message Bubble ───

const MessageBubble: FC<{
  message: AgentStreamMessage;
  toolResultMap: Map<string, ContentBlock>;
  isStreaming: boolean;
  debugEnabled: boolean;
}> = memo(({ message, toolResultMap, isStreaming, debugEnabled }) => {
  if (message.type === "user") {
    return <UserMessage message={message} />;
  }
  if (message.type === "assistant") {
    return (
      <AssistantMessage
        message={message}
        toolResultMap={toolResultMap}
        isStreaming={isStreaming}
        debugEnabled={debugEnabled}
      />
    );
  }
  if (message.type === "result") {
    return <ResultMessage message={message} />;
  }
  return null;
});

// ─── User Message ───

const UserMessage: FC<{ message: AgentStreamMessage }> = ({ message }) => {
  const rawContent = message.message?.content;
  const textContent = Array.isArray(rawContent)
    ? rawContent
        .filter((b) => b.type === "text")
        .map((b) => b.text)
        .join("\n")
    : typeof rawContent === "string"
      ? rawContent
      : "";

  if (!textContent) return null;

  // Parse leading @file:line:col or ~@file:line context reference
  const contextMatch = textContent.match(/^(~?@[^\n]+)\n([\s\S]*)$/);
  const contextLabel = contextMatch?.[1] ?? null;
  const bodyText = contextMatch ? contextMatch[2] : textContent;

  // Parse error block patterns for styled rendering:
  // Lint single: "[Lint error in FILE:LINE]\n[Error: MSG]\n\nPrompt"
  // Lint multi:  "[Lint errors in FILE]\n- FILE:LINE — MSG\n...\n\nPrompt"
  // Compile:     "[Compilation errors]\n- error1\n- error2\n...\n\nPrompt"
  const lintSingleMatch = bodyText.match(
    /^\[Lint error in ([^\]]+)\]\n\[Error: ([^\]]+)\]\n\n([\s\S]*)$/,
  );
  const lintMultiMatch = bodyText.match(
    /^\[Lint errors in ([^\]]+)\]\n((?:- .+\n?)+)\n([\s\S]*)$/,
  );
  const compileErrorMatch = bodyText.match(
    /^\[Compilation errors\]\n((?:- .+\n?)+)\n([\s\S]*)$/,
  );

  // Shared error block renderer
  const renderErrorBlock = (
    title: string,
    errors: { message: string; location?: string }[],
    prompt: string,
  ) => (
    <div className="flex w-full flex-col items-end py-1.5">
      <div className="max-w-[85%] rounded-xl bg-muted px-3 py-2 text-foreground text-sm">
        <div className="mb-2 rounded-lg border border-red-500/20 bg-red-500/10 px-2.5 py-2">
          <div className="mb-1.5 font-medium text-red-400 text-xs">{title}</div>
          <div className="space-y-1">
            {errors.map((e, i) => (
              <div key={i} className="flex items-start gap-1.5">
                <AlertCircleIcon className="mt-0.5 size-3 shrink-0 text-red-400/70" />
                <span className="flex-1 text-foreground/80 text-xs">
                  {e.message}
                </span>
                {e.location && (
                  <span className="shrink-0 font-mono text-muted-foreground text-xs">
                    {e.location}
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
        <span className="text-muted-foreground">{prompt}</span>
      </div>
    </div>
  );

  if (lintSingleMatch) {
    const [, location, errorMsg, prompt] = lintSingleMatch;
    return renderErrorBlock(
      `Lint Error`,
      [{ message: errorMsg, location }],
      prompt,
    );
  }

  if (lintMultiMatch) {
    const [, fileName, errorLines, prompt] = lintMultiMatch;
    const errors = errorLines
      .trim()
      .split("\n")
      .map((line) => {
        const m = line.match(/^- (.+?):(\d+) — (.+)$/);
        return m
          ? { message: m[3], location: `${m[1]}:${m[2]}` }
          : { message: line.replace(/^- /, "") };
      });
    return renderErrorBlock(`Lint Errors — ${fileName}`, errors, prompt);
  }

  if (compileErrorMatch) {
    const [, errorLines, prompt] = compileErrorMatch;
    const errors = errorLines
      .trim()
      .split("\n")
      .map((line) => ({
        message: line.replace(/^- /, ""),
      }));
    return renderErrorBlock(
      `Compilation ${errors.length === 1 ? "Error" : "Errors"}`,
      errors,
      prompt,
    );
  }

  return (
    <div className="flex w-full flex-col items-end py-1.5">
      <div className="max-w-[85%] rounded-xl bg-muted px-3 py-1.5 text-foreground text-sm">
        {contextLabel && (
          <span className="mb-1 inline-flex items-center rounded-md bg-background/60 px-1.5 py-0.5 font-mono text-muted-foreground text-xs">
            {contextLabel}
          </span>
        )}
        {contextLabel && bodyText && <br />}
        <MarkdownRenderer
          content={bodyText}
          className="prose prose-sm dark:prose-invert max-w-none [&>*:first-child]:mt-0 [&>*:last-child]:mb-0"
        />
      </div>
    </div>
  );
};

// ─── Assistant Message ───

const AssistantMessage: FC<{
  message: AgentStreamMessage;
  toolResultMap: Map<string, ContentBlock>;
  isStreaming: boolean;
  debugEnabled: boolean;
}> = ({ message, toolResultMap, isStreaming, debugEnabled }) => {
  const content = message.message?.content;
  if (!Array.isArray(content) || content.length === 0) return null;

  const hasRenderableContent = content.some(
    (block) =>
      (block.type === "text" && block.text) ||
      (block.type === "thinking" && block.thinking) ||
      (block.type === "tool_use" && block.id),
  );

  if (!hasRenderableContent) return null;

  const representativeDocumentStep =
    !debugEnabled && content.some(isAnyDocumentToolUse)
      ? pickRepresentativeDocumentToolUse(content, toolResultMap)
      : null;
  let renderedDocumentStep = false;

  return (
    <div className="w-full py-1.5">
      <div className="px-1 text-foreground text-sm leading-relaxed">
        {content.map((block, idx) => {
          if (block.type === "thinking" && block.thinking) {
            return (
              <ThinkingWidget
                key={idx}
                thinking={block.thinking}
                signature={block.signature}
              />
            );
          }
          if (block.type === "text" && block.text) {
            return (
              <MarkdownRenderer
                key={idx}
                content={block.text}
                className="prose prose-sm dark:prose-invert max-w-none"
              />
            );
          }
          if (block.type === "tool_use" && block.id) {
            if (!debugEnabled && isAnyDocumentToolUse(block)) {
              if (renderedDocumentStep) {
                return null;
              }
              renderedDocumentStep = true;
              const representative = representativeDocumentStep ?? {
                ...block,
                name: "read_document",
              };
              const result =
                representative.id && toolResultMap.has(representative.id)
                  ? toolResultMap.get(representative.id)
                  : toolResultMap.get(block.id);
              return (
                <ToolWidget
                  key={idx}
                  toolUse={representative}
                  toolResult={result}
                  isStreaming={isStreaming}
                />
              );
            }

            const result = toolResultMap.get(block.id);
            return (
              <ToolWidget
                key={idx}
                toolUse={block}
                toolResult={result}
                isStreaming={isStreaming}
              />
            );
          }
          return null;
        })}
      </div>
    </div>
  );
};

// ─── Result Message ───

const ResultMessage: FC<{ message: AgentStreamMessage }> = ({ message }) => {
  const isError = message.is_error || message.subtype === "error";
  const resultText = message.result;

  if (!resultText) return null;

  return (
    <div className="w-full py-1.5">
      <div className="px-1 text-foreground text-sm leading-relaxed">
        {isError ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-destructive text-sm">
            {resultText}
          </div>
        ) : (
          <MarkdownRenderer
            content={resultText}
            className="prose prose-sm dark:prose-invert max-w-none"
          />
        )}
      </div>
      {message.cost_usd != null && (
        <div className="mt-1 px-1 text-right text-muted-foreground text-xs">
          Cost: ${message.cost_usd.toFixed(4)}
        </div>
      )}
    </div>
  );
};
