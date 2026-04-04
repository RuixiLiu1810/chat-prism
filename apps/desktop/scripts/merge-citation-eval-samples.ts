/**
 * Merge citation eval samples from multiple files.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:merge-samples -- --inputs ./a.json,./b.jsonl --out ./dataset.json
 *
 * Supports each input file as:
 * - JSON array
 * - JSON Lines (one sample object per line)
 */

import fs from "node:fs";
import path from "node:path";

interface Args {
  inputs: string[];
  outPath: string;
}

function parseArgs(argv: string[]): Args {
  let inputsRaw = "";
  let outPath = "";

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--inputs" && argv[i + 1]) {
      inputsRaw = argv[++i];
      continue;
    }
    if (token === "--out" && argv[i + 1]) {
      outPath = argv[++i];
      continue;
    }
  }

  const inputs = inputsRaw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  if (inputs.length === 0) {
    throw new Error("Missing --inputs, expected comma-separated file paths.");
  }
  if (!outPath) {
    throw new Error("Missing --out output path.");
  }
  return { inputs, outPath };
}

function parseFlexibleJson(raw: string): unknown[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  if (trimmed.startsWith("[")) {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!Array.isArray(parsed)) {
      throw new Error("JSON mode requires an array.");
    }
    return parsed;
  }
  return trimmed
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as unknown);
}

function asRecord(v: unknown): Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : {};
}

function normalizeSample(raw: unknown, fallbackIndex: number) {
  const record = asRecord(raw);
  const id = typeof record.id === "string" && record.id.trim()
    ? record.id.trim()
    : `sample_${String(fallbackIndex + 1).padStart(4, "0")}`;

  const selectedText =
    typeof record.selected_text === "string" ? record.selected_text : "";
  const expected = asRecord(record.expected);
  const expectedDois = Array.isArray(expected.dois) ? expected.dois : [];
  const expectedTitles = Array.isArray(expected.titles) ? expected.titles : [];
  const mergedResults = Array.isArray(record.merged_results)
    ? record.merged_results
    : [];

  return {
    ...record,
    id,
    selected_text: selectedText,
    expected: {
      ...expected,
      dois: expectedDois,
      titles: expectedTitles,
    },
    merged_results: mergedResults,
  };
}

function loadSamplesFromFile(filePath: string): unknown[] {
  const fullPath = path.resolve(filePath);
  const raw = fs.readFileSync(fullPath, "utf-8");
  return parseFlexibleJson(raw);
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const dedup = new Map<string, Record<string, unknown>>();
  let totalRead = 0;

  for (const input of args.inputs) {
    const list = loadSamplesFromFile(input);
    totalRead += list.length;
    list.forEach((item, index) => {
      const normalized = normalizeSample(item, index);
      dedup.set(normalized.id, normalized);
    });
  }

  const merged = Array.from(dedup.values());
  merged.sort((a, b) => {
    const aId = typeof a.id === "string" ? a.id : "";
    const bId = typeof b.id === "string" ? b.id : "";
    return aId.localeCompare(bId);
  });

  const outPath = path.resolve(args.outPath);
  fs.writeFileSync(outPath, `${JSON.stringify(merged, null, 2)}\n`, "utf-8");

  console.log(`Read samples: ${totalRead}`);
  console.log(`Merged unique samples: ${merged.length}`);
  console.log(`Saved dataset: ${outPath}`);
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`merge-citation-eval-samples failed: ${message}`);
  process.exit(1);
}
