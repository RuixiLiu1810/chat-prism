/**
 * Generate a labeling template for citation eval samples.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:label-template -- --input ./dataset.json --out ./label-template.json
 *   pnpm --filter @claude-prism/desktop citation:label-template -- --input ./dataset.json --out ./label-template.json --top-k 5 --include-labeled
 *
 * Input supports:
 * - JSON array
 * - JSON Lines
 */

import fs from "node:fs";
import path from "node:path";

interface Args {
  inputPath: string;
  outPath: string;
  topK: number;
  includeLabeled: boolean;
}

interface CandidateSummary {
  rank: number;
  score: number | null;
  title: string | null;
  doi: string | null;
  year: number | null;
  venue: string | null;
}

interface LabelTemplateRow {
  id: string;
  selected_text: string;
  expected: {
    dois: string[];
    titles: string[];
    no_match: boolean;
  };
  proposed_expected: {
    dois: string[];
    titles: string[];
    no_match: boolean;
  };
  top_candidates: CandidateSummary[];
}

function parseArgs(argv: string[]): Args {
  let inputPath = "";
  let outPath = "";
  let topK = 3;
  let includeLabeled = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--input" && argv[i + 1]) {
      inputPath = argv[++i];
      continue;
    }
    if (token === "--out" && argv[i + 1]) {
      outPath = argv[++i];
      continue;
    }
    if (token === "--top-k" && argv[i + 1]) {
      const parsed = Number.parseInt(argv[++i], 10);
      if (Number.isFinite(parsed) && parsed > 0) {
        topK = parsed;
      }
      continue;
    }
    if (token === "--include-labeled") {
      includeLabeled = true;
    }
  }

  if (!inputPath) {
    throw new Error("Missing --input dataset path.");
  }
  if (!outPath) {
    throw new Error("Missing --out output path.");
  }

  return { inputPath, outPath, topK, includeLabeled };
}

function parseFlexibleJson(raw: string): unknown[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  if (trimmed.startsWith("[")) {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!Array.isArray(parsed)) {
      throw new Error("JSON mode requires top-level array.");
    }
    return parsed;
  }
  return trimmed
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as unknown);
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => (typeof item === "string" ? item.trim() : ""))
    .filter((item) => item.length > 0);
}

function normalizeDoi(doi: string): string {
  return doi
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/doi\.org\//, "")
    .replace(/^doi:/, "");
}

function normalizeTitle(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function toFiniteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function toFiniteInt(value: unknown): number | null {
  const num = toFiniteNumber(value);
  if (num === null) return null;
  const rounded = Math.round(num);
  return Number.isFinite(rounded) ? rounded : null;
}

function summarizeCandidates(sample: Record<string, unknown>, topK: number): CandidateSummary[] {
  const mergedResults = Array.isArray(sample.merged_results)
    ? sample.merged_results
    : [];
  return mergedResults.slice(0, topK).map((candidate, index) => {
    const rec = asRecord(candidate);
    const title = typeof rec.title === "string" ? rec.title.trim() : null;
    const doiRaw = typeof rec.doi === "string" ? rec.doi.trim() : "";
    const doi = doiRaw ? normalizeDoi(doiRaw) : null;
    const venue = typeof rec.venue === "string" ? rec.venue.trim() : null;
    return {
      rank: index + 1,
      score: toFiniteNumber(rec.score),
      title: title || null,
      doi,
      year: toFiniteInt(rec.year),
      venue: venue || null,
    };
  });
}

function uniqueKeepOrder(items: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of items) {
    if (!item) continue;
    if (seen.has(item)) continue;
    seen.add(item);
    out.push(item);
  }
  return out;
}

function buildTemplateRow(raw: unknown, index: number, topK: number): LabelTemplateRow {
  const sample = asRecord(raw);
  const expected = asRecord(sample.expected);
  const expectedDois = asStringArray(expected.dois).map(normalizeDoi);
  const expectedTitles = asStringArray(expected.titles).map((title) => title.trim());
  const expectedNoMatch =
    expected.no_match === true ||
    sample.expected_no_match === true ||
    sample.gold_no_match === true;
  const topCandidates = summarizeCandidates(sample, topK);

  const proposedDois = uniqueKeepOrder(
    topCandidates.map((candidate) => candidate.doi ?? ""),
  );
  const proposedTitles = uniqueKeepOrder(
    topCandidates
      .map((candidate) => candidate.title ?? "")
      .filter((title) => title.length > 0),
  );

  const idRaw = typeof sample.id === "string" ? sample.id.trim() : "";
  const selectedTextRaw =
    typeof sample.selected_text === "string" ? sample.selected_text.trim() : "";

  return {
    id: idRaw || `sample_${String(index + 1).padStart(4, "0")}`,
    selected_text: selectedTextRaw,
    expected: {
      dois: expectedDois,
      titles: expectedTitles,
      no_match: expectedNoMatch,
    },
    proposed_expected: {
      dois: proposedDois,
      titles: proposedTitles,
      no_match: topCandidates.length === 0,
    },
    top_candidates: topCandidates,
  };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const inputPath = path.resolve(args.inputPath);
  const outPath = path.resolve(args.outPath);
  const raw = fs.readFileSync(inputPath, "utf-8");
  const parsed = parseFlexibleJson(raw);

  const rows = parsed.map((item, index) =>
    buildTemplateRow(item, index, args.topK),
  );

  const selected = rows.filter((row) => {
    const labeled =
      row.expected.no_match ||
      row.expected.dois.length > 0 ||
      row.expected.titles.length > 0;
    return args.includeLabeled ? true : !labeled;
  });

  fs.writeFileSync(outPath, `${JSON.stringify(selected, null, 2)}\n`, "utf-8");

  console.log(`Input samples: ${rows.length}`);
  console.log(`Template rows: ${selected.length}`);
  console.log(
    `Mode: ${args.includeLabeled ? "include labeled + unlabeled" : "unlabeled only"}`,
  );
  console.log(`Top-K candidates per row: ${args.topK}`);
  console.log(`Saved template: ${outPath}`);
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`generate-citation-label-template failed: ${message}`);
  process.exit(1);
}
