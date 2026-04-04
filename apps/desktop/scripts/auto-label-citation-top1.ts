/**
 * Auto-label unlabeled citation samples using top-1 candidate under confidence gates.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:auto-label -- --dataset ./dataset.json --out ./dataset.autolabeled.json
 *   pnpm --filter @claude-prism/desktop citation:auto-label -- --dataset ./dataset.json --out ./dataset.autolabeled.json --min-score 0.45 --min-gap 0.02
 */

import fs from "node:fs";
import path from "node:path";

interface Args {
  datasetPath: string;
  outPath: string;
  minScore: number;
  minGap: number;
  overwriteLabeled: boolean;
}

function parseArgs(argv: string[]): Args {
  let datasetPath = "";
  let outPath = "";
  let minScore = 0.65;
  let minGap = 0.05;
  let overwriteLabeled = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--dataset" && argv[i + 1]) {
      datasetPath = argv[++i];
      continue;
    }
    if (token === "--out" && argv[i + 1]) {
      outPath = argv[++i];
      continue;
    }
    if (token === "--min-score" && argv[i + 1]) {
      const parsed = Number.parseFloat(argv[++i]);
      if (Number.isFinite(parsed)) minScore = parsed;
      continue;
    }
    if (token === "--min-gap" && argv[i + 1]) {
      const parsed = Number.parseFloat(argv[++i]);
      if (Number.isFinite(parsed)) minGap = parsed;
      continue;
    }
    if (token === "--overwrite-labeled") {
      overwriteLabeled = true;
      continue;
    }
  }

  if (!datasetPath) {
    throw new Error("Missing --dataset path.");
  }
  if (!outPath) {
    throw new Error("Missing --out path.");
  }

  return { datasetPath, outPath, minScore, minGap, overwriteLabeled };
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

function toScore(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function normalizeDoi(doi: string): string {
  return doi
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\/doi\.org\//, "")
    .replace(/^doi:/, "");
}

function expectedFilled(record: Record<string, unknown>): boolean {
  const expected = asRecord(record.expected);
  const dois = asStringArray(expected.dois);
  const titles = asStringArray(expected.titles);
  return dois.length > 0 || titles.length > 0;
}

function pickTopCandidates(record: Record<string, unknown>): {
  top1: Record<string, unknown> | null;
  top2: Record<string, unknown> | null;
} {
  const list = Array.isArray(record.merged_results) ? record.merged_results : [];
  const top1 = list.length > 0 ? asRecord(list[0]) : null;
  const top2 = list.length > 1 ? asRecord(list[1]) : null;
  return { top1, top2 };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const datasetPath = path.resolve(args.datasetPath);
  const outPath = path.resolve(args.outPath);
  const raw = fs.readFileSync(datasetPath, "utf8");
  const rows = parseFlexibleJson(raw);

  let updated = 0;
  let skippedAlreadyLabeled = 0;
  let skippedNoCandidates = 0;
  let skippedNoMetadata = 0;
  let skippedLowConfidence = 0;

  const next = rows.map((row, index) => {
    const rec = asRecord(row);
    const idRaw = typeof rec.id === "string" ? rec.id.trim() : "";
    const id = idRaw || `sample_${String(index + 1).padStart(4, "0")}`;

    if (!args.overwriteLabeled && expectedFilled(rec)) {
      skippedAlreadyLabeled += 1;
      return rec;
    }

    const { top1, top2 } = pickTopCandidates(rec);
    if (!top1) {
      skippedNoCandidates += 1;
      return rec;
    }

    const title = typeof top1.title === "string" ? top1.title.trim() : "";
    const doiRaw = typeof top1.doi === "string" ? top1.doi.trim() : "";
    const doi = doiRaw ? normalizeDoi(doiRaw) : "";
    if (!title && !doi) {
      skippedNoMetadata += 1;
      return rec;
    }

    const s1 = toScore(top1.score);
    const s2 = top2 ? toScore(top2.score) : 0;
    const gap = s1 - s2;

    if (s1 < args.minScore || gap < args.minGap) {
      skippedLowConfidence += 1;
      return rec;
    }

    updated += 1;
    return {
      ...rec,
      id,
      expected: {
        dois: doi ? [doi] : [],
        titles: title ? [title] : [],
      },
      auto_label_meta: {
        method: "top1_confidence_gate",
        min_score: args.minScore,
        min_gap: args.minGap,
        top1_score: s1,
        top2_score: s2,
        score_gap: gap,
      },
    };
  });

  fs.writeFileSync(outPath, `${JSON.stringify(next, null, 2)}\n`, "utf8");

  console.log("Auto-label citation samples");
  console.log(`dataset: ${datasetPath}`);
  console.log(`out: ${outPath}`);
  console.log(`minScore=${args.minScore}, minGap=${args.minGap}`);
  console.log(`overwrite labeled: ${args.overwriteLabeled ? "yes" : "no"}`);
  console.log(`rows: ${rows.length}`);
  console.log(`updated: ${updated}`);
  console.log(`skipped(already labeled): ${skippedAlreadyLabeled}`);
  console.log(`skipped(no candidates): ${skippedNoCandidates}`);
  console.log(`skipped(no doi/title on top1): ${skippedNoMetadata}`);
  console.log(`skipped(low confidence): ${skippedLowConfidence}`);
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`auto-label-citation-top1 failed: ${message}`);
  process.exit(1);
}
