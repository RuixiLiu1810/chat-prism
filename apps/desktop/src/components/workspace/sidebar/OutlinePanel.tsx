import { HashIcon } from "lucide-react";

// ─── Types ───

export interface TocItem {
  level: number;
  title: string;
  line: number;
}

// ─── Parser ───

export function parseTableOfContents(content: string): TocItem[] {
  const lines = content.split("\n");
  const toc: TocItem[] = [];
  const sectionRegex =
    /\\(section|subsection|subsubsection|chapter|part)\*?\s*\{([^}]*)\}/;
  const levelMap: Record<string, number> = {
    part: 0,
    chapter: 1,
    section: 2,
    subsection: 3,
    subsubsection: 4,
  };
  lines.forEach((line, index) => {
    const match = line.match(sectionRegex);
    if (match) {
      const [, type, title] = match;
      toc.push({
        level: levelMap[type] ?? 2,
        title: title.trim(),
        line: index + 1,
      });
    }
  });
  return toc;
}

// ─── Outline Panel Content ───

interface OutlinePanelProps {
  toc: TocItem[];
  onTocClick: (line: number) => void;
}

export function OutlinePanelContent({ toc, onTocClick }: OutlinePanelProps) {
  if (toc.length === 0) {
    return (
      <div className="px-2 py-1 text-muted-foreground text-xs">
        No sections found
      </div>
    );
  }

  return (
    <>
      {toc.map((item, index) => (
        <button
          key={index}
          className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-sm transition-colors hover:bg-sidebar-accent/50"
          style={{ paddingLeft: `${(item.level - 1) * 12 + 8}px` }}
          onClick={() => onTocClick(item.line)}
        >
          <HashIcon className="size-3 shrink-0 text-muted-foreground" />
          <span className="truncate">{item.title}</span>
        </button>
      ))}
    </>
  );
}
