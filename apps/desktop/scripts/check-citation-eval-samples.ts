/**
 * Validate citation evaluation dataset quality.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:check-samples -- --input ./dataset.json
 *   pnpm --filter @claude-prism/desktop citation:check-samples -- --input ./dataset.json --out-unlabeled ./unlabeled.json --strict
 *
 * Supported input:
 * - JSON array
 * - JSON Lines
 */

import fs from "node:fs";
import path from "node:path";

interface Args {
  inputPath: string;
  outUnlabeledPath?: string;
  strict: boolean;
}

interface SampleCheck {
  id: string;
  valid: boolean;
  labeled: boolean;
  issues: string[];
  raw: Record<string, unknown>;
}

function parseArgs(argv: string[]): Args {
  let inputPath = "";
  let outUnlabeledPath: string | undefined;
  let strict = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--input" && argv[i + 1]) {
      inputPath = argv[++i];
      continue;
    }
    if (token === "--out-unlabeled" && argv[i + 1]) {
      outUnlabeledPath = argv[++i];
      continue;
    }
    if (token === "--strict") {
      strict = true;
    }
  }

  if (!inputPath) {
    throw new Error("Missing --input dataset path.");
  }
  return { inputPath, outUnlabeledPath, strict };
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
  return value.filter((v): v is string => typeof v === "string").map((v) => v.trim());
}

function asBool(value: unknown): boolean {
  return value === true;
}

function validateSample(raw: unknown, index: number): SampleCheck {
  const record = asRecord(raw);
  const issues: string[] = [];
  const idRaw = record.id;
  const id =
    typeof idRaw === "string" && idRaw.trim()
      ? idRaw.trim()
      : `sample_${String(index + 1).padStart(4, "0")}`;

  const selectedText =
    typeof record.selected_text === "string" ? record.selected_text.trim() : "";
  if (!selectedText) {
    issues.push("selected_text is empty");
  }

  const expected = asRecord(record.expected);
  const dois = asStringArray(expected.dois).filter((v) => v.length > 0);
  const titles = asStringArray(expected.titles).filter((v) => v.length > 0);
  const noMatch =
    asBool(expected.no_match) ||
    asBool(record.expected_no_match) ||
    asBool(record.gold_no_match);
  const labeled = noMatch || dois.length > 0 || titles.length > 0;

  const mergedResults = record.merged_results;
  if (!Array.isArray(mergedResults)) {
    issues.push("merged_results is not an array");
  }

  const valid = issues.length === 0;
  return { id, valid, labeled, issues, raw: record };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const fullPath = path.resolve(args.inputPath);
  const raw = fs.readFileSync(fullPath, "utf-8");
  const parsed = parseFlexibleJson(raw);
  const checks = parsed.map((item, idx) => validateSample(item, idx));

  const total = checks.length;
  const validCount = checks.filter((c) => c.valid).length;
  const invalid = checks.filter((c) => !c.valid);
  const labeled = checks.filter((c) => c.labeled).length;
  const unlabeled = checks.filter((c) => c.valid && !c.labeled);

  console.log(`Total samples: ${total}`);
  console.log(`Valid samples: ${validCount}`);
  console.log(`Invalid samples: ${invalid.length}`);
  console.log(`Labeled samples: ${labeled}`);
  console.log(`Unlabeled samples: ${unlabeled.length}`);

  if (invalid.length > 0) {
    console.log("\nInvalid details:");
    for (const item of invalid.slice(0, 30)) {
      console.log(`- ${item.id}: ${item.issues.join("; ")}`);
    }
    if (invalid.length > 30) {
      console.log(`... and ${invalid.length - 30} more invalid samples`);
    }
  }

  if (unlabeled.length > 0) {
    console.log("\nUnlabeled sample IDs:");
    const ids = unlabeled.map((u) => u.id);
    for (const id of ids.slice(0, 80)) {
      console.log(`- ${id}`);
    }
    if (ids.length > 80) {
      console.log(`... and ${ids.length - 80} more`);
    }
  }

  if (args.outUnlabeledPath) {
    const outPath = path.resolve(args.outUnlabeledPath);
    const payload = unlabeled.map((u) => u.raw);
    fs.writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, "utf-8");
    console.log(`\nSaved unlabeled samples: ${outPath}`);
  }

  if (args.strict && (invalid.length > 0 || unlabeled.length > 0)) {
    process.exit(2);
  }
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`check-citation-eval-samples failed: ${message}`);
  process.exit(1);
}
