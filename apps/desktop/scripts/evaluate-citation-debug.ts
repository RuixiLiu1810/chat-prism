/**
 * Offline evaluator for citation debug runs.
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:evaluate -- --input ./runs.json
 *   pnpm --filter @claude-prism/desktop citation:evaluate -- --input ./runs.json --baseline ./runs_baseline.json --out ./eval-report.json
 *
 * Input formats:
 * - JSON array
 * - JSON Lines (one JSON object per line)
 *
 * Per record, evaluator tries these fields (flexible):
 * - expected DOI/title:
 *   expected.dois / expected_dois / gold_dois
 *   expected.titles / expected_titles / gold_titles
 * - merged results:
 *   merged_results / results / debug.merged_results
 * - latency:
 *   latency_ms / duration_ms / elapsed_ms
 */

import fs from "node:fs";
import path from "node:path";

interface ScoreExplain {
  contradiction_penalty?: number;
}

interface Candidate {
  title?: string;
  doi?: string;
  score?: number;
  score_explain?: ScoreExplain;
}

interface EvalCase {
  id: string;
  expectedDois: string[];
  expectedTitles: string[];
  expectedNoMatch: boolean;
  results: Candidate[];
  latencyMs?: number;
}

interface DatasetMetrics {
  total: number;
  labeled: number;
  withExpected: number;
  noMatchExpected: number;
  emptyRate: number;
  top1HitRate: number;
  top3HitRate: number;
  top5HitRate: number;
  mrr: number;
  latencyP50Ms?: number;
  latencyP95Ms?: number;
}

interface ThresholdEval {
  threshold: number;
  maxPenalty: number;
  precision: number;
  coverage: number;
  selected: number;
}

interface ThresholdRecommendation {
  auto: ThresholdEval;
  review: ThresholdEval;
}

interface EvalReport {
  summary: DatasetMetrics;
  thresholdRecommendation: ThresholdRecommendation;
  labeling: {
    unlabeledCount: number;
    unlabeledIds: string[];
  };
  baseline?: {
    summary: DatasetMetrics;
    delta: Record<string, number>;
  };
}

interface ParsedArgs {
  inputPath: string;
  baselinePath?: string;
  outPath?: string;
}

function parseArgs(argv: string[]): ParsedArgs {
  let inputPath = "";
  let baselinePath: string | undefined;
  let outPath: string | undefined;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--input" && argv[i + 1]) {
      inputPath = argv[++i];
      continue;
    }
    if (token === "--baseline" && argv[i + 1]) {
      baselinePath = argv[++i];
      continue;
    }
    if (token === "--out" && argv[i + 1]) {
      outPath = argv[++i];
      continue;
    }
  }

  if (!inputPath) {
    throw new Error(
      "Missing --input. Example: --input ./runs.json --baseline ./baseline.json --out ./report.json",
    );
  }
  return { inputPath, baselinePath, outPath };
}

function parseFlexibleJson(raw: string): unknown[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];

  if (trimmed.startsWith("[")) {
    const parsed = JSON.parse(trimmed) as unknown;
    if (Array.isArray(parsed)) return parsed;
    throw new Error("Input JSON must be an array when using JSON mode.");
  }

  const out: unknown[] = [];
  const lines = trimmed.split(/\r?\n/);
  for (const line of lines) {
    const s = line.trim();
    if (!s) continue;
    out.push(JSON.parse(s) as unknown);
  }
  return out;
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((v) => (typeof v === "string" ? v.trim() : ""))
    .filter((v) => v.length > 0);
}

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asBool(value: unknown): boolean {
  return value === true;
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

function toEvalCase(raw: unknown, index: number): EvalCase {
  const root = asRecord(raw);
  const expected = asRecord(root.expected);
  const debug = asRecord(root.debug);

  const expectedDois = [
    ...asStringArray(expected.dois),
    ...asStringArray(root.expected_dois),
    ...asStringArray(root.gold_dois),
  ].map(normalizeDoi);

  const expectedTitles = [
    ...asStringArray(expected.titles),
    ...asStringArray(root.expected_titles),
    ...asStringArray(root.gold_titles),
  ].map(normalizeTitle);
  const expectedNoMatch =
    asBool(expected.no_match) ||
    asBool(root.expected_no_match) ||
    asBool(root.gold_no_match);

  const mergedResultsRaw =
    (Array.isArray(root.merged_results) ? root.merged_results : undefined) ??
    (Array.isArray(root.results) ? root.results : undefined) ??
    (Array.isArray(debug.merged_results) ? debug.merged_results : undefined) ??
    [];
  const results = mergedResultsRaw
    .map((item) => asRecord(item))
    .map((item) => ({
      title: typeof item.title === "string" ? item.title : undefined,
      doi: typeof item.doi === "string" ? item.doi : undefined,
      score: asNumber(item.score),
      score_explain: asRecord(item.score_explain) as ScoreExplain,
    }));

  const latencyMs =
    asNumber(root.latency_ms) ??
    asNumber(root.duration_ms) ??
    asNumber(root.elapsed_ms) ??
    asNumber(debug.latency_ms) ??
    asNumber(debug.duration_ms) ??
    asNumber(debug.elapsed_ms);

  return {
    id:
      (typeof root.id === "string" && root.id.trim()) ||
      `case_${String(index + 1).padStart(3, "0")}`,
    expectedDois,
    expectedTitles,
    expectedNoMatch,
    results,
    latencyMs,
  };
}

function isExpectedMatch(
  candidate: Candidate,
  expectedDois: string[],
  expectedTitles: string[],
): boolean {
  const candDoi = candidate.doi ? normalizeDoi(candidate.doi) : "";
  if (candDoi && expectedDois.includes(candDoi)) return true;

  const candTitle = candidate.title ? normalizeTitle(candidate.title) : "";
  if (candTitle && expectedTitles.includes(candTitle)) return true;

  return false;
}

function hasPositiveExpected(sample: EvalCase): boolean {
  return sample.expectedDois.length > 0 || sample.expectedTitles.length > 0;
}

function isLabeledSample(sample: EvalCase): boolean {
  return sample.expectedNoMatch || hasPositiveExpected(sample);
}

function firstMatchRank(sample: EvalCase): number {
  for (let i = 0; i < sample.results.length; i += 1) {
    if (
      isExpectedMatch(sample.results[i], sample.expectedDois, sample.expectedTitles)
    ) {
      return i + 1;
    }
  }
  return -1;
}

function percentile(values: number[], p: number): number | undefined {
  if (values.length === 0) return undefined;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(
    sorted.length - 1,
    Math.max(0, Math.round((sorted.length - 1) * p)),
  );
  return sorted[idx];
}

function safeRate(numerator: number, denominator: number): number {
  return denominator > 0 ? numerator / denominator : 0;
}

function round4(value: number): number {
  return Math.round(value * 10000) / 10000;
}

function evaluateDataset(samples: EvalCase[]): DatasetMetrics {
  const total = samples.length;
  const withExpectedSamples = samples.filter(hasPositiveExpected);
  const withExpected = withExpectedSamples.length;
  const labeled = samples.filter(isLabeledSample).length;
  const noMatchExpected = samples.filter((s) => s.expectedNoMatch).length;
  const emptyCount = samples.filter((s) => s.results.length === 0).length;

  let top1Hits = 0;
  let top3Hits = 0;
  let top5Hits = 0;
  let mrrTotal = 0;

  for (const sample of withExpectedSamples) {
    const rank = firstMatchRank(sample);
    if (rank === 1) top1Hits += 1;
    if (rank > 0 && rank <= 3) top3Hits += 1;
    if (rank > 0 && rank <= 5) top5Hits += 1;
    if (rank > 0) mrrTotal += 1 / rank;
  }

  const latencies = samples
    .map((s) => s.latencyMs)
    .filter((v): v is number => typeof v === "number");

  return {
    total,
    labeled,
    withExpected,
    noMatchExpected,
    emptyRate: round4(safeRate(emptyCount, total)),
    top1HitRate: round4(safeRate(top1Hits, withExpected)),
    top3HitRate: round4(safeRate(top3Hits, withExpected)),
    top5HitRate: round4(safeRate(top5Hits, withExpected)),
    mrr: round4(safeRate(mrrTotal, withExpected)),
    latencyP50Ms: percentile(latencies, 0.5),
    latencyP95Ms: percentile(latencies, 0.95),
  };
}

function evalThreshold(
  samples: EvalCase[],
  threshold: number,
  maxPenalty: number,
): ThresholdEval {
  const withExpectedSamples = samples.filter(hasPositiveExpected);
  const evaluatedTotal = withExpectedSamples.length;
  let selected = 0;
  let correct = 0;

  for (const sample of withExpectedSamples) {
    const top = sample.results[0];
    if (!top) continue;
    const score = top.score ?? 0;
    const penalty = top.score_explain?.contradiction_penalty ?? 0;
    if (score < threshold || penalty > maxPenalty) continue;
    selected += 1;
    if (isExpectedMatch(top, sample.expectedDois, sample.expectedTitles)) {
      correct += 1;
    }
  }

  return {
    threshold: round4(threshold),
    maxPenalty: round4(maxPenalty),
    precision: round4(safeRate(correct, selected)),
    coverage: round4(safeRate(selected, evaluatedTotal)),
    selected,
  };
}

function chooseAutoThreshold(samples: EvalCase[]): ThresholdEval {
  let best: ThresholdEval | null = null;
  for (let threshold = 0.6; threshold <= 0.95; threshold += 0.01) {
    for (let penalty = 0.04; penalty <= 0.16; penalty += 0.01) {
      const candidate = evalThreshold(samples, threshold, penalty);
      if (candidate.selected === 0) continue;
      const passesPrecision = candidate.precision >= 0.9;
      if (!passesPrecision) continue;
      if (!best) {
        best = candidate;
        continue;
      }
      const betterCoverage = candidate.coverage > best.coverage;
      const sameCoverageBetterPrecision =
        candidate.coverage === best.coverage &&
        candidate.precision > best.precision;
      if (betterCoverage || sameCoverageBetterPrecision) {
        best = candidate;
      }
    }
  }
  return best ?? evalThreshold(samples, 0.78, 0.08);
}

function chooseReviewThreshold(samples: EvalCase[]): ThresholdEval {
  let best: ThresholdEval | null = null;
  let bestF1 = -1;
  for (let threshold = 0.5; threshold <= 0.85; threshold += 0.01) {
    for (let penalty = 0.12; penalty <= 0.30; penalty += 0.01) {
      const candidate = evalThreshold(samples, threshold, penalty);
      if (candidate.selected === 0) continue;
      const passesPrecision = candidate.precision >= 0.6;
      if (!passesPrecision) continue;
      const fScore = 2 * candidate.precision * candidate.coverage;
      const denom = candidate.precision + candidate.coverage;
      const f1 = denom > 0 ? fScore / denom : 0;

      if (!best) {
        best = candidate;
        bestF1 = f1;
        continue;
      }

      if (
        f1 > bestF1 ||
        (f1 === bestF1 && candidate.precision > best.precision)
      ) {
        best = candidate;
        bestF1 = f1;
      }
    }
  }
  if (!best) return evalThreshold(samples, 0.62, 0.24);
  return best;
}

function recommendThresholds(samples: EvalCase[]): ThresholdRecommendation {
  const auto = chooseAutoThreshold(samples);
  const reviewRaw = chooseReviewThreshold(samples);
  const reviewThreshold = Math.min(reviewRaw.threshold, auto.threshold);
  const reviewPenalty = Math.max(reviewRaw.maxPenalty, auto.maxPenalty);
  const review = evalThreshold(samples, reviewThreshold, reviewPenalty);
  return { auto, review };
}

function deltaSummary(
  current: DatasetMetrics,
  baseline: DatasetMetrics,
): Record<string, number> {
  return {
    top1HitRate: round4(current.top1HitRate - baseline.top1HitRate),
    top3HitRate: round4(current.top3HitRate - baseline.top3HitRate),
    top5HitRate: round4(current.top5HitRate - baseline.top5HitRate),
    emptyRate: round4(current.emptyRate - baseline.emptyRate),
    mrr: round4(current.mrr - baseline.mrr),
    latencyP50Ms: round4(
      (current.latencyP50Ms ?? 0) - (baseline.latencyP50Ms ?? 0),
    ),
    latencyP95Ms: round4(
      (current.latencyP95Ms ?? 0) - (baseline.latencyP95Ms ?? 0),
    ),
  };
}

function printSummary(label: string, summary: DatasetMetrics): void {
  console.log(`\n== ${label} ==`);
  console.log(
    `samples=${summary.total}, labeled=${summary.labeled}, positive=${summary.withExpected}, no_match=${summary.noMatchExpected}, empty_rate=${summary.emptyRate}`,
  );
  console.log(
    `top1=${summary.top1HitRate}, top3=${summary.top3HitRate}, top5=${summary.top5HitRate}, mrr=${summary.mrr}`,
  );
  if (summary.latencyP50Ms != null || summary.latencyP95Ms != null) {
    console.log(
      `latency_p50_ms=${summary.latencyP50Ms ?? "n/a"}, latency_p95_ms=${summary.latencyP95Ms ?? "n/a"}`,
    );
  }
}

function loadCasesFromFile(filePath: string): EvalCase[] {
  const fullPath = path.resolve(filePath);
  const raw = fs.readFileSync(fullPath, "utf-8");
  const list = parseFlexibleJson(raw);
  return list.map((item, idx) => toEvalCase(item, idx));
}

function main(): void {
  const args = parseArgs(process.argv.slice(2));
  const currentCases = loadCasesFromFile(args.inputPath);
  const summary = evaluateDataset(currentCases);
  const thresholdRecommendation = recommendThresholds(currentCases);
  const unlabeledIds = currentCases
    .filter((sample) => !isLabeledSample(sample))
    .map((sample) => sample.id);

  const report: EvalReport = {
    summary,
    thresholdRecommendation,
    labeling: {
      unlabeledCount: unlabeledIds.length,
      unlabeledIds,
    },
  };

  printSummary("Current", summary);
  console.log("\n== Threshold Recommendation ==");
  console.log(
    `auto: threshold=${thresholdRecommendation.auto.threshold}, max_penalty=${thresholdRecommendation.auto.maxPenalty}, precision=${thresholdRecommendation.auto.precision}, coverage=${thresholdRecommendation.auto.coverage}, selected=${thresholdRecommendation.auto.selected}`,
  );
  console.log(
    `review: threshold=${thresholdRecommendation.review.threshold}, max_penalty=${thresholdRecommendation.review.maxPenalty}, precision=${thresholdRecommendation.review.precision}, coverage=${thresholdRecommendation.review.coverage}, selected=${thresholdRecommendation.review.selected}`,
  );
  if (unlabeledIds.length > 0) {
    console.log(
      `\nUnlabeled samples: ${unlabeledIds.length} (fill expected.dois/titles or set expected.no_match=true before trusting threshold output).`,
    );
  }

  if (args.baselinePath) {
    const baselineCases = loadCasesFromFile(args.baselinePath);
    const baselineSummary = evaluateDataset(baselineCases);
    const delta = deltaSummary(summary, baselineSummary);
    report.baseline = { summary: baselineSummary, delta };
    printSummary("Baseline", baselineSummary);
    console.log("\n== Delta (current - baseline) ==");
    console.log(JSON.stringify(delta, null, 2));
  }

  if (args.outPath) {
    const outPath = path.resolve(args.outPath);
    fs.writeFileSync(outPath, `${JSON.stringify(report, null, 2)}\n`, "utf-8");
    console.log(`\nSaved report: ${outPath}`);
  }
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`citation evaluator failed: ${message}`);
  process.exit(1);
}
