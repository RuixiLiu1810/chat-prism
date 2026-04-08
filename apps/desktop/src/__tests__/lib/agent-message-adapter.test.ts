import { describe, expect, it } from "vitest";
import {
  adaptAgentStreamMessageForUi,
  adaptToolResultDisplayContent,
  getToolResultDisplayApproval,
  getToolResultDisplayTarget,
  isAgentToolResultDisplayContent,
  stripPseudoToolCallMarkup,
} from "@/lib/agent-message-adapter";

describe("agent-message-adapter", () => {
  it("normalizes raw tool-result payloads into a UI-safe display shape", () => {
    const adapted = adaptToolResultDisplayContent({
      approvalRequired: true,
      reviewArtifact: true,
      path: "main.tex",
      absolutePath: "/tmp/main.tex",
      reason: "Waiting for approval.",
      oldContent: "old",
      newContent: "new",
      reviewArtifactPayload: {
        targetPath: "main.tex",
        summary: "Diff is ready.",
      },
    });

    expect(adapted.kind).toBe("tool_result_display");
    expect(adapted.approvalRequired).toBe(true);
    expect(adapted.reviewReady).toBe(true);
    expect(adapted.targetPath).toBe("main.tex");
    expect(adapted.summary).toBe("Diff is ready.");
    expect(adapted.textPreview).toBe("Waiting for approval.");
    expect(getToolResultDisplayApproval(adapted)?.reviewReady).toBe(true);
  });

  it("passes through backend-generated display payloads without losing status metadata", () => {
    const adapted = adaptToolResultDisplayContent({
      kind: "tool_result_display",
      displayKind: "document_search",
      toolName: "search_document_text",
      status: "completed",
      textPreview: "Relevant evidence from paper A",
      isError: false,
      approvalRequired: false,
      reviewReady: false,
      query: "hydrophobic",
    });

    expect(adapted.displayKind).toBe("document_search");
    expect(adapted.toolName).toBe("search_document_text");
    expect(adapted.status).toBe("completed");
    expect(adapted.textPreview).toContain("paper A");
  });

  it("adapts history tool_result strings into the same display shape", () => {
    const message = adaptAgentStreamMessageForUi({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "call_1",
            content: "Read main.tex successfully.",
            is_error: false,
          },
        ],
      },
    });

    const block = message.message?.content?.[0] as { content: unknown };
    expect(isAgentToolResultDisplayContent(block.content)).toBe(true);
    expect(getToolResultDisplayTarget(block.content)).toBe(null);
    expect(
      (block.content as { textPreview: string }).textPreview,
    ).toBe("Read main.tex successfully.");
  });

  it("extracts a target from normalized command and file payloads", () => {
    const command = adaptToolResultDisplayContent({
      approvalRequired: true,
      input: { command: "rg hydrophobic ." },
      reason: "Shell approval required.",
    });
    const file = adaptToolResultDisplayContent({
      path: "chapter1.tex",
      content: "Read file content.",
    });

    expect(getToolResultDisplayTarget(command)).toBe("rg hydrophobic .");
    expect(getToolResultDisplayTarget(file)).toBe("chapter1.tex");
  });
  it("strips pseudo tool-call markup from assistant text", () => {
    const sanitized = stripPseudoToolCallMarkup(
      "让我尝试提取文本：\n<tool_call> {\"name\": \"shell\"} </tool_call>\n结论待定",
    );

    expect(sanitized).toBe("让我尝试提取文本：\n结论待定");
  });

  it("strips bracketed pseudo tool-call markup from assistant text", () => {
    const sanitized = stripPseudoToolCallMarkup(
      "我先试着提取内容。\n[TOOL_CALL] {tool => \"bash\", args => { --command \"pdftotext file.pdf\" }} [/TOOL_CALL]\n再继续分析。",
    );

    expect(sanitized).toBe("我先试着提取内容。\n再继续分析。");
  });

  it("derives a readable preview from structured writing-tool payloads", () => {
    const adapted = adaptToolResultDisplayContent({
      toolName: "check_consistency",
      findings: [
        {
          severity: "major",
          message: "Abbreviation 'MRI' appears multiple times without definition.",
        },
      ],
      approvalRequired: false,
      reviewArtifact: false,
    });

    expect(adapted.textPreview).toContain("Consistency findings");
    expect(adapted.textPreview).toContain("MRI");
  });
});
