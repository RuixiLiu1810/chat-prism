/**
 * Apply reviewed label-template back into citation eval dataset.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:apply-labels -- --dataset ./dataset.json --labels ./label-template-reviewed.json --out ./dataset.labeled.json
 *   pnpm --filter @claude-prism/desktop citation:apply-labels -- --dataset ./dataset.json --labels ./labels.json --out ./dataset.labeled.json --use-proposed
 *
 * Input supports:
 * - JSON array
 * - JSON Lines
 */

import fs from "node:fs";
import path from "node:path";

interface Args {
  datasetPath: string;
  labelsPath: string;
  outPath: string;
  useProposed: boolean;
  overwriteLabeled: boolean;
}

interface ExpectedLike {
  dois: string[];
  titles: string[];
  noMatch: boolean;
}

interface LabelRecord {
  id: string;
  expected: ExpectedLike;
  proposed_expected: ExpectedLike;
}

function parseArgs(argv: string[]): Args {
  let datasetPath = "";
  let labelsPath = "";
  let outPath = "";
  let useProposed = false;
  let overwriteLabeled = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--dataset" && argv[i + 1]) {
      datasetPath = argv[++i];
      continue;
    }
    if (token === "--labels" && argv[i + 1]) {
      labelsPath = argv[++i];
      continue;
    }
    if (token === "--out" && argv[i + 1]) {
      outPath = argv[++i];
      continue;
    }
    if (token === "--use-proposed") {
      useProposed = true;
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
  if (!labelsPath) {
    throw new Error("Missing --labels path.");
  }
  if (!outPath) {
    throw new Error("Missing --out path.");
  }

  return { datasetPath, labelsPath, outPath, useProposed, overwriteLabeled };
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

function uniqueKeepOrder(items: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of items) {
    const normalized = item.trim();
    if (!normalized) continue;
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

function parseExpected(raw: unknown): ExpectedLike {
  const rec = asRecord(raw);
  return {
    dois: uniqueKeepOrder(asStringArray(rec.dois).map(normalizeDoi)),
    titles: uniqueKeepOrder(asStringArray(rec.titles)),
    noMatch: rec.no_match === true,
  };
}

function isExpectedFilled(expected: ExpectedLike): boolean {
  return expected.noMatch || expected.dois.length > 0 || expected.titles.length > 0;
}

function chooseExpected(
  label: LabelRecord,
  useProposed: boolean,
): ExpectedLike | null {
  if (isExpectedFilled(label.expected)) {
    return label.expected;
  }
  if (useProposed && isExpectedFilled(label.proposed_expected)) {
    return label.proposed_expected;
  }
  return null;
}

function toLabelRecord(raw: unknown, index: number): LabelRecord {
  const rec = asRecord(raw);
  const idRaw = typeof rec.id === "string" ? rec.id.trim() : "";
  return {
    id: idRaw || `row_${String(index + 1).padStart(4, "0")}`,
    expected: parseExpected(rec.expected),
    proposed_expected: parseExpected(rec.proposed_expected),
  };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const datasetPath = path.resolve(args.datasetPath);
  const labelsPath = path.resolve(args.labelsPath);
  const outPath = path.resolve(args.outPath);

  const datasetRaw = fs.readFileSync(datasetPath, "utf8");
  const labelsRaw = fs.readFileSync(labelsPath, "utf8");
  const datasetList = parseFlexibleJson(datasetRaw);
  const labelList = parseFlexibleJson(labelsRaw);

  const labels = labelList.map((item, index) => toLabelRecord(item, index));
  const labelById = new Map(labels.map((item) => [item.id, item]));

  let updated = 0;
  let skippedNoLabel = 0;
  let skippedNoExpected = 0;
  let skippedAlreadyLabeled = 0;

  const patched = datasetList.map((item, index) => {
    const rec = asRecord(item);
    const idRaw = typeof rec.id === "string" ? rec.id.trim() : "";
    const id = idRaw || `sample_${String(index + 1).padStart(4, "0")}`;
    const expectedCurrent = parseExpected(rec.expected);
    const label = labelById.get(id);

    if (!label) {
      skippedNoLabel += 1;
      return rec;
    }

    if (!args.overwriteLabeled && isExpectedFilled(expectedCurrent)) {
      skippedAlreadyLabeled += 1;
      return rec;
    }

    const chosen = chooseExpected(label, args.useProposed);
    if (!chosen) {
      skippedNoExpected += 1;
      return rec;
    }

    updated += 1;
    return {
      ...rec,
      id,
      expected: {
        dois: chosen.dois,
        titles: chosen.titles,
        no_match: chosen.noMatch,
      },
    };
  });

  fs.writeFileSync(outPath, `${JSON.stringify(patched, null, 2)}\n`, "utf8");

  console.log("Apply citation labels");
  console.log(`dataset: ${datasetPath}`);
  console.log(`labels: ${labelsPath}`);
  console.log(`out: ${outPath}`);
  console.log(
    `mode: ${args.useProposed ? "fallback to proposed_expected enabled" : "expected only"}`,
  );
  console.log(
    `overwrite labeled: ${args.overwriteLabeled ? "yes" : "no (default)"}`,
  );
  console.log(`rows: dataset=${datasetList.length}, labels=${labels.length}`);
  console.log(`updated: ${updated}`);
  console.log(`skipped(no label): ${skippedNoLabel}`);
  console.log(`skipped(no expected in label): ${skippedNoExpected}`);
  console.log(`skipped(already labeled): ${skippedAlreadyLabeled}`);
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`apply-citation-labels failed: ${message}`);
  process.exit(1);
}
