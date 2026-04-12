import { type FC } from "react";
import { type ContentBlock } from "@/stores/agent-chat-store";
import { WritingWidget } from "./widgets/writing-widget";
import { WriteWidget, PreciseEditWidget } from "./widgets/write-widgets";
import { ReadWidget } from "./widgets/read-widget";
import { BashWidget } from "./widgets/bash-widget";
import { GlobWidget, GrepWidget } from "./widgets/search-widgets";
import { GenericWidget } from "./widgets/generic-widget";

export { ThinkingWidget } from "./widgets/thinking-widget";

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
