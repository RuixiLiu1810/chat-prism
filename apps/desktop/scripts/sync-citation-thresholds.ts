/**
 * Sync citation default thresholds from evaluator report.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:sync-thresholds -- --report /tmp/citation_eval_report.json
 *   pnpm --filter @claude-prism/desktop citation:sync-thresholds -- --report /tmp/citation_eval_report.json --write
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

interface ThresholdEval {
  threshold?: number;
  maxPenalty?: number;
}

interface EvalReport {
  thresholdRecommendation?: {
    auto?: ThresholdEval;
    review?: ThresholdEval;
  };
  labeling?: {
    unlabeledCount?: number;
  };
}

interface ParsedArgs {
  reportPath: string;
  write: boolean;
  allowUnlabeled: boolean;
}

function parseArgs(argv: string[]): ParsedArgs {
  let reportPath = "";
  let write = false;
  let allowUnlabeled = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--report" && argv[i + 1]) {
      reportPath = argv[++i];
      continue;
    }
    if (token === "--write") {
      write = true;
      continue;
    }
    if (token === "--allow-unlabeled") {
      allowUnlabeled = true;
      continue;
    }
  }

  if (!reportPath) {
    throw new Error("Missing --report <path>.");
  }

  return { reportPath, write, allowUnlabeled };
}

function asNum(v: unknown): number | undefined {
  return typeof v === "number" && Number.isFinite(v) ? v : undefined;
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

function fmt(n: number): string {
  const rounded = Math.round(n * 10000) / 10000;
  let s = rounded.toFixed(4).replace(/\.?0+$/, "");
  if (!s.includes(".")) s = `${s}.0`;
  return s;
}

function replaceOnce(
  content: string,
  regex: RegExp,
  replacer: (...args: string[]) => string,
  label: string,
): string {
  let matched = false;
  const next = content.replace(regex, (...args) => {
    matched = true;
    return replacer(...(args as string[]));
  });
  if (!matched) {
    throw new Error(`pattern not found: ${label}`);
  }
  return next;
}

function patchSettingsSchema(
  source: string,
  autoThreshold: string,
  reviewThreshold: string,
): string {
  let next = source;
  next = replaceOnce(
    next,
    /(citation:\s*\{\s*stylePolicy:\s*"auto",\s*autoApplyThreshold:\s*)([0-9.]+)(,\s*reviewThreshold:\s*)([0-9.]+)/,
    (_m, p1, _oldAuto, p3) => `${p1}${autoThreshold}${p3}${reviewThreshold}`,
    "settings-schema default citation thresholds",
  );
  next = replaceOnce(
    next,
    /(pickNumberInRange\(autoApplyThreshold,\s*)([0-9.]+)(,\s*0,\s*1\))/,
    (_m, p1, _old, p3) => `${p1}${autoThreshold}${p3}`,
    "settings-schema auto fallback threshold",
  );
  next = replaceOnce(
    next,
    /(pickNumberInRange\(reviewThreshold,\s*)([0-9.]+)(,\s*0,\s*1\))/,
    (_m, p1, _old, p3) => `${p1}${reviewThreshold}${p3}`,
    "settings-schema review fallback threshold",
  );
  return next;
}

function patchCitationStore(
  source: string,
  autoThreshold: string,
  reviewThreshold: string,
  autoPenalty: string,
  reviewPenalty: string,
): string {
  let next = source;
  next = replaceOnce(
    next,
    /(const DEFAULT_AUTO_APPLY_THRESHOLD = )([0-9.]+)(;)/,
    (_m, p1, _old, p3) => `${p1}${autoThreshold}${p3}`,
    "citation-store auto threshold",
  );
  next = replaceOnce(
    next,
    /(const DEFAULT_REVIEW_THRESHOLD = )([0-9.]+)(;)/,
    (_m, p1, _old, p3) => `${p1}${reviewThreshold}${p3}`,
    "citation-store review threshold",
  );
  next = replaceOnce(
    next,
    /(const AUTO_APPLY_MAX_CONTRADICTION_PENALTY = )([0-9.]+)(;)/,
    (_m, p1, _old, p3) => `${p1}${autoPenalty}${p3}`,
    "citation-store auto penalty",
  );
  next = replaceOnce(
    next,
    /(const REVIEW_MAX_CONTRADICTION_PENALTY = )([0-9.]+)(;)/,
    (_m, p1, _old, p3) => `${p1}${reviewPenalty}${p3}`,
    "citation-store review penalty",
  );
  return next;
}

function patchRustSettings(
  source: string,
  autoThreshold: string,
  reviewThreshold: string,
): string {
  let next = source;
  next = replaceOnce(
    next,
    /("autoApplyThreshold":\s*)([0-9.]+)(,\s*"reviewThreshold":\s*)([0-9.]+)/,
    (_m, p1, _oldAuto, p3) => `${p1}${autoThreshold}${p3}${reviewThreshold}`,
    "settings.rs default global citation thresholds",
  );
  next = replaceOnce(
    next,
    /(get_in\(input, &\["citation", "autoApplyThreshold"\]\), 0\.0, 1\.0\)\s*\.unwrap_or\()([0-9.]+)(\);)/m,
    (_m, p1, _old, p3) => `${p1}${autoThreshold}${p3}`,
    "settings.rs sanitize auto fallback",
  );
  next = replaceOnce(
    next,
    /(get_in\(input, &\["citation", "reviewThreshold"\]\), 0\.0, 1\.0\)\s*\.unwrap_or\()([0-9.]+)(\);)/m,
    (_m, p1, _old, p3) => `${p1}${reviewThreshold}${p3}`,
    "settings.rs sanitize review fallback",
  );
  next = replaceOnce(
    next,
    /(get_in\(&global, &\["citation", "autoApplyThreshold"\]\)\s*\.and_then\(Value::as_f64\)\s*\.unwrap_or\()([0-9.]+)(\))/m,
    (_m, p1, _old, p3) => `${p1}${autoThreshold}${p3}`,
    "settings.rs effective auto fallback",
  );
  next = replaceOnce(
    next,
    /(get_in\(&global, &\["citation", "reviewThreshold"\]\)\s*\.and_then\(Value::as_f64\)\s*\.unwrap_or\()([0-9.]+)(\))/m,
    (_m, p1, _old, p3) => `${p1}${reviewThreshold}${p3}`,
    "settings.rs effective review fallback",
  );
  return next;
}

function maybeWrite(filePath: string, content: string, write: boolean) {
  if (!write) return;
  fs.writeFileSync(filePath, content, "utf8");
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const fullReportPath = path.resolve(args.reportPath);
  const raw = fs.readFileSync(fullReportPath, "utf8");
  const report = JSON.parse(raw) as EvalReport;

  const auto = asNum(report.thresholdRecommendation?.auto?.threshold);
  const review = asNum(report.thresholdRecommendation?.review?.threshold);
  const autoPenalty = asNum(report.thresholdRecommendation?.auto?.maxPenalty);
  const reviewPenalty = asNum(report.thresholdRecommendation?.review?.maxPenalty);

  if (
    auto == null ||
    review == null ||
    autoPenalty == null ||
    reviewPenalty == null
  ) {
    throw new Error(
      "Report missing thresholdRecommendation.auto/review threshold/maxPenalty.",
    );
  }

  const normalizedAuto = clamp(auto, 0, 1);
  const normalizedReview = clamp(Math.min(review, normalizedAuto), 0, 1);
  const normalizedAutoPenalty = clamp(autoPenalty, 0, 1);
  const normalizedReviewPenalty = clamp(
    Math.max(reviewPenalty, normalizedAutoPenalty),
    0,
    1,
  );

  const autoStr = fmt(normalizedAuto);
  const reviewStr = fmt(normalizedReview);
  const autoPenaltyStr = fmt(normalizedAutoPenalty);
  const reviewPenaltyStr = fmt(normalizedReviewPenalty);

  const thisDir = path.dirname(fileURLToPath(import.meta.url));
  const root = path.resolve(thisDir, "..");
  const schemaPath = path.resolve(root, "src/lib/settings-schema.ts");
  const storePath = path.resolve(root, "src/stores/citation-store.ts");
  const rustPath = path.resolve(root, "src-tauri/src/settings.rs");

  const schemaCurrent = fs.readFileSync(schemaPath, "utf8");
  const storeCurrent = fs.readFileSync(storePath, "utf8");
  const rustCurrent = fs.readFileSync(rustPath, "utf8");

  const schemaNext = patchSettingsSchema(schemaCurrent, autoStr, reviewStr);
  const storeNext = patchCitationStore(
    storeCurrent,
    autoStr,
    reviewStr,
    autoPenaltyStr,
    reviewPenaltyStr,
  );
  const rustNext = patchRustSettings(rustCurrent, autoStr, reviewStr);

  console.log("Citation threshold sync");
  console.log(`report: ${fullReportPath}`);
  console.log(`write: ${args.write ? "yes" : "no (dry-run)"}`);
  console.log(
    `target -> auto=${autoStr}, review=${reviewStr}, autoPenalty=${autoPenaltyStr}, reviewPenalty=${reviewPenaltyStr}`,
  );

  const unlabeled = asNum(report.labeling?.unlabeledCount) ?? 0;
  if (unlabeled > 0) {
    console.log(
      `warning: unlabeledCount=${unlabeled}; threshold recommendation may be unstable.`,
    );
  }
  if (args.write && unlabeled > 0 && !args.allowUnlabeled) {
    throw new Error(
      "Refusing to write defaults because unlabeledCount > 0. Re-run with --allow-unlabeled if intentional.",
    );
  }

  maybeWrite(schemaPath, schemaNext, args.write);
  maybeWrite(storePath, storeNext, args.write);
  maybeWrite(rustPath, rustNext, args.write);

  if (args.write) {
    console.log("updated files:");
    console.log(`- ${schemaPath}`);
    console.log(`- ${storePath}`);
    console.log(`- ${rustPath}`);
  } else {
    console.log("dry-run complete. add --write to apply changes.");
  }
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`sync-citation-thresholds failed: ${message}`);
  process.exit(1);
}
