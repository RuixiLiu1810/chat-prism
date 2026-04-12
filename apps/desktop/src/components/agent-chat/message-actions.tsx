import { type FC, useState, useCallback, useEffect, useMemo } from "react";
import { Copy, RotateCcw, Edit2, X, Check } from "lucide-react";
import { toast } from "sonner";
import { useAgentChatStore, type AgentStreamMessage } from "@/stores/agent-chat-store";

interface MessageActionsProps {
  message: AgentStreamMessage;
  sourceIndex?: number;
  canRetry?: boolean;
  isLastMessage?: boolean;
  className?: string;
}

/**
 * Extract plain text from a message for copying/editing
 */
export function extractMessageText(message: AgentStreamMessage): string {
  if (message.type === "user") {
    const rawContent = message.message?.content;
    if (Array.isArray(rawContent)) {
      return rawContent
        .filter((b: any) => b.type === "text")
        .map((b: any) => b.text)
        .filter(Boolean)
        .join("\n");
    }
    return typeof rawContent === "string" ? rawContent : "";
  }

  if (message.type === "assistant") {
    const content = message.message?.content;
    if (Array.isArray(content)) {
      return content
        .filter((b: any) => b.type === "text" && b.text)
        .map((b: any) => b.text)
        .join("\n");
    }
  }

  if (message.type === "result") {
    return message.result || "";
  }

  return "";
}

/**
 * MessageActions component - compact actions row for message-level operations.
 */
export const MessageActions: FC<MessageActionsProps> = ({
  message,
  sourceIndex,
  canRetry = true,
  isLastMessage = false,
  className = "",
}) => {
  const [isEditing, setIsEditing] = useState(false);
  const [editedText, setEditedText] = useState(extractMessageText(message));
  const sendPrompt = useAgentChatStore((s) => s.sendPrompt);
  const messages = useAgentChatStore((s) => s.messages);
  const isStreaming = useAgentChatStore((s) => s.isStreaming);
  const text = useMemo(() => extractMessageText(message), [message]);

  useEffect(() => {
    setEditedText(text);
  }, [text]);

  const resolvedIndex = useMemo(() => {
    if (typeof sourceIndex === "number" && sourceIndex >= 0) {
      return sourceIndex;
    }
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i] === message) {
        return i;
      }
    }
    return -1;
  }, [sourceIndex, messages, message]);

  const handleCopy = useCallback(async () => {
    if (!text) {
      toast.error("No text to copy");
      return;
    }

    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard");
    } catch (err) {
      toast.error("Failed to copy");
    }
  }, [text]);

  const handleEdit = useCallback(() => {
    if (message.type !== "user") {
      toast.error("Can only edit user messages");
      return;
    }
    setIsEditing(true);
  }, [message.type]);

  const handleSaveEdit = useCallback(async () => {
    const trimmed = editedText.trim();
    if (!trimmed) {
      toast.error("Message cannot be empty");
      return;
    }

    try {
      // Send the edited message as a new prompt
      await sendPrompt(trimmed);
      setIsEditing(false);
      toast.success("Message sent");
    } catch (err) {
      toast.error("Failed to send message");
    }
  }, [editedText, sendPrompt]);

  const handleRetry = useCallback(async () => {
    if (isStreaming) {
      toast.error("Please wait for the current response to finish");
      return;
    }
    try {
      if (!messages || resolvedIndex < 0 || resolvedIndex >= messages.length) {
        toast.error("Cannot find message to retry");
        return;
      }

      // Search backwards for the last user message
      for (let i = resolvedIndex - 1; i >= 0; i--) {
        if (messages[i].type === "user") {
          const userText = extractMessageText(messages[i]);
          if (userText) {
            // Resend the same prompt
            await sendPrompt(userText);
            toast.success("Regenerating response...");
            return;
          }
        }
      }

      toast.error("No user message found to retry");
    } catch (err) {
      toast.error("Failed to retry");
    }
  }, [messages, resolvedIndex, sendPrompt, isStreaming]);

  const showCopy = Boolean(text);
  const showEdit = message.type === "user";
  const showRetry = message.type === "assistant" && canRetry;

  if (isEditing && message.type === "user") {
    return (
      <div className="flex flex-col gap-2 w-full">
        <textarea
          value={editedText}
          onChange={(e) => setEditedText(e.target.value)}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-sm"
          rows={Math.max(3, editedText.split("\n").length)}
          autoFocus
        />
        <div className="flex gap-2 justify-end">
          <button
            onClick={handleSaveEdit}
            className="inline-flex items-center gap-1.5 rounded-md bg-primary px-2.5 py-1.5 text-primary-foreground text-xs hover:bg-primary/90 transition-colors"
            title="Save and send"
          >
            <Check className="size-3" />
            Send
          </button>
          <button
            onClick={() => setIsEditing(false)}
            className="inline-flex items-center gap-1.5 rounded-md bg-muted px-2.5 py-1.5 text-foreground text-xs hover:bg-muted/80 transition-colors"
            title="Cancel editing"
          >
            <X className="size-3" />
            Cancel
          </button>
        </div>
      </div>
    );
  }

  if (!showCopy && !showEdit && !showRetry) {
    return null;
  }

  return (
    <div
      className={`inline-flex items-center gap-0.5 transition-opacity ${isLastMessage ? "opacity-100" : "opacity-100 md:opacity-0 md:group-hover:opacity-100 md:group-focus-within:opacity-100"} ${className}`}
    >
      {showCopy ? (
        <button
          onClick={handleCopy}
          className="inline-flex items-center justify-center rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          title="Copy message"
          aria-label="Copy"
        >
          <Copy className="size-3" />
        </button>
      ) : null}

      {showEdit && (
        <button
          onClick={handleEdit}
          className="inline-flex items-center justify-center rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          title="Edit and resend message"
          aria-label="Edit"
        >
          <Edit2 className="size-3" />
        </button>
      )}

      {showRetry && (
        <button
          onClick={handleRetry}
          className="inline-flex items-center justify-center rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          title="Regenerate response"
          aria-label="Retry"
        >
          <RotateCcw className="size-3" />
        </button>
      )}
    </div>
  );
};
