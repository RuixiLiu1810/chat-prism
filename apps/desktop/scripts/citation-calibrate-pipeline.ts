/**
 * Citation calibration pipeline:
 * merge -> check -> evaluate -> label-template -> sync-thresholds
 *
 * Usage:
 *   pnpm --filter @claude-prism/desktop citation:calibrate -- --inputs ./a.json,./b.jsonl
 *   pnpm --filter @claude-prism/desktop citation:calibrate -- --dataset ./dataset.json --baseline ./baseline.json
 *   pnpm --filter @claude-prism/desktop citation:calibrate -- --dataset ./dataset.json --labels ./label-template-reviewed.json --labels-use-proposed
 *   pnpm --filter @claude-prism/desktop citation:calibrate -- --inputs ./a.json --write-defaults
 *   pnpm --filter @claude-prism/desktop citation:calibrate -- --inputs ./a.json --label-top-k 5
 */

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

interface Args {
  inputs: string[];
  datasetPath?: string;
  labelsPath?: string;
  baselinePath?: string;
  outDir: string;
  writeDefaults: boolean;
  allowUnlabeled: boolean;
  strictCheck: boolean;
  skipLabelTemplate: boolean;
  labelTopK: number;
  labelIncludeLabeled: boolean;
  labelsUseProposed: boolean;
  labelsOverwriteLabeled: boolean;
}

type RunnerMode = "pnpm" | "local-tsx";

type ScriptName =
  | "citation:merge-samples"
  | "citation:check-samples"
  | "citation:evaluate"
  | "citation:label-template"
  | "citation:sync-thresholds"
  | "citation:apply-labels";

const SCRIPT_PATH_MAP: Record<ScriptName, string> = {
  "citation:merge-samples": "scripts/merge-citation-eval-samples.ts",
  "citation:check-samples": "scripts/check-citation-eval-samples.ts",
  "citation:evaluate": "scripts/evaluate-citation-debug.ts",
  "citation:label-template": "scripts/generate-citation-label-template.ts",
  "citation:sync-thresholds": "scripts/sync-citation-thresholds.ts",
  "citation:apply-labels": "scripts/apply-citation-labels.ts",
};

function parseArgs(argv: string[]): Args {
  let inputsRaw = "";
  let datasetPath: string | undefined;
  let labelsPath: string | undefined;
  let baselinePath: string | undefined;
  let outDir = "./tmp/citation-calibration";
  let writeDefaults = false;
  let allowUnlabeled = false;
  let strictCheck = false;
  let skipLabelTemplate = false;
  let labelTopK = 3;
  let labelIncludeLabeled = false;
  let labelsUseProposed = false;
  let labelsOverwriteLabeled = false;

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--inputs" && argv[i + 1]) {
      inputsRaw = argv[++i];
      continue;
    }
    if (token === "--dataset" && argv[i + 1]) {
      datasetPath = argv[++i];
      continue;
    }
    if (token === "--labels" && argv[i + 1]) {
      labelsPath = argv[++i];
      continue;
    }
    if (token === "--baseline" && argv[i + 1]) {
      baselinePath = argv[++i];
      continue;
    }
    if (token === "--out-dir" && argv[i + 1]) {
      outDir = argv[++i];
      continue;
    }
    if (token === "--write-defaults") {
      writeDefaults = true;
      continue;
    }
    if (token === "--allow-unlabeled") {
      allowUnlabeled = true;
      continue;
    }
    if (token === "--strict-check") {
      strictCheck = true;
      continue;
    }
    if (token === "--skip-label-template") {
      skipLabelTemplate = true;
      continue;
    }
    if (token === "--label-top-k" && argv[i + 1]) {
      const parsed = Number.parseInt(argv[++i], 10);
      if (Number.isFinite(parsed) && parsed > 0) {
        labelTopK = parsed;
      }
      continue;
    }
    if (token === "--label-include-labeled") {
      labelIncludeLabeled = true;
      continue;
    }
    if (token === "--labels-use-proposed") {
      labelsUseProposed = true;
      continue;
    }
    if (token === "--labels-overwrite-labeled") {
      labelsOverwriteLabeled = true;
      continue;
    }
  }

  const inputs = inputsRaw
    .split(",")
    .map((x) => x.trim())
    .filter((x) => x.length > 0);

  if (inputs.length > 0 && datasetPath) {
    throw new Error("Use either --inputs or --dataset, not both.");
  }
  if (inputs.length === 0 && !datasetPath) {
    throw new Error("Missing input source. Use --inputs or --dataset.");
  }

  return {
    inputs,
    datasetPath,
    labelsPath,
    baselinePath,
    outDir,
    writeDefaults,
    allowUnlabeled,
    strictCheck,
    skipLabelTemplate,
    labelTopK,
    labelIncludeLabeled,
    labelsUseProposed,
    labelsOverwriteLabeled,
  };
}

function run(label: string, cmd: string, args: string[]) {
  const rendered = [cmd, ...args].join(" ");
  console.log(`\n> ${label}`);
  console.log(`$ ${rendered}`);
  const started = Date.now();
  const proc = spawnSync(cmd, args, {
    cwd: process.cwd(),
    env: process.env,
    stdio: "inherit",
  });
  const ms = Date.now() - started;
  if (proc.status !== 0) {
    throw new Error(`${label} failed with exit code ${proc.status ?? "unknown"}.`);
  }
  console.log(`[ok] ${label} (${(ms / 1000).toFixed(1)}s)`);
}

function commandAvailable(cmd: string): boolean {
  const probe = spawnSync(cmd, ["--version"], {
    cwd: process.cwd(),
    env: process.env,
    stdio: "ignore",
  });
  return probe.status === 0;
}

function localBinCommand(name: "tsx"): string | null {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const desktopRoot = path.resolve(scriptDir, "..");
  const cmd = path.resolve(desktopRoot, "../../node_modules/.bin", name);
  return fs.existsSync(cmd) ? cmd : null;
}

function detectRunnerMode(): RunnerMode {
  if (commandAvailable("pnpm")) return "pnpm";
  if (localBinCommand("tsx")) return "local-tsx";
  throw new Error(
    "Neither pnpm nor local tsx is available. Install pnpm or ensure ../../node_modules/.bin/tsx exists.",
  );
}

function runScriptTask(
  runnerMode: RunnerMode,
  label: string,
  scriptName: ScriptName,
  scriptArgs: string[],
) {
  if (runnerMode === "pnpm") {
    run(label, "pnpm", [scriptName, "--", ...scriptArgs]);
    return;
  }

  const tsx = localBinCommand("tsx");
  if (!tsx) {
    throw new Error("local tsx binary not found.");
  }
  const scriptPath = SCRIPT_PATH_MAP[scriptName];
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const desktopRoot = path.resolve(scriptDir, "..");
  run(label, tsx, [path.resolve(desktopRoot, scriptPath), ...scriptArgs]);
}

function readUnlabeledCount(filePath: string): number {
  if (!fs.existsSync(filePath)) return 0;
  const raw = fs.readFileSync(filePath, "utf8").trim();
  if (!raw) return 0;
  try {
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? parsed.length : 0;
  } catch {
    return 0;
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const runnerMode = detectRunnerMode();
  const outDir = path.resolve(args.outDir);
  fs.mkdirSync(outDir, { recursive: true });

  const datasetPath = args.datasetPath
    ? path.resolve(args.datasetPath)
    : path.resolve(outDir, "dataset.json");
  const labelsPath = args.labelsPath ? path.resolve(args.labelsPath) : undefined;
  const labeledDatasetPath = path.resolve(outDir, "dataset.labeled.json");
  const unlabeledPath = path.resolve(outDir, "unlabeled.json");
  const reportPath = path.resolve(outDir, "eval-report.json");
  const labelTemplatePath = path.resolve(outDir, "label-template.json");
  const workingDatasetPath = labelsPath ? labeledDatasetPath : datasetPath;

  console.log("Citation calibration pipeline");
  console.log(`cwd: ${process.cwd()}`);
  console.log(`outDir: ${outDir}`);
  console.log(`dataset: ${datasetPath}`);
  if (labelsPath) {
    console.log(`labels: ${labelsPath}`);
  }
  console.log(`workingDataset: ${workingDatasetPath}`);
  console.log(`writeDefaults: ${args.writeDefaults ? "yes" : "no"}`);
  console.log(`labelTemplate: ${args.skipLabelTemplate ? "skip" : "yes"}`);
  console.log(`runner: ${runnerMode}`);

  if (args.inputs.length > 0) {
    runScriptTask(runnerMode, "merge samples", "citation:merge-samples", [
      "--inputs",
      args.inputs.join(","),
      "--out",
      datasetPath,
    ]);
  }

  if (labelsPath) {
    const applyArgs = [
      "--dataset",
      datasetPath,
      "--labels",
      labelsPath,
      "--out",
      labeledDatasetPath,
    ];
    if (args.labelsUseProposed) {
      applyArgs.push("--use-proposed");
    }
    if (args.labelsOverwriteLabeled) {
      applyArgs.push("--overwrite-labeled");
    }
    runScriptTask(runnerMode, "apply labels", "citation:apply-labels", applyArgs);
  }

  const checkArgs = [
    "--input",
    workingDatasetPath,
    "--out-unlabeled",
    unlabeledPath,
  ];
  if (args.strictCheck) checkArgs.push("--strict");
  runScriptTask(runnerMode, "check samples", "citation:check-samples", checkArgs);

  const evalArgs = [
    "--input",
    workingDatasetPath,
    "--out",
    reportPath,
  ];
  if (args.baselinePath) {
    evalArgs.push("--baseline", path.resolve(args.baselinePath));
  }
  runScriptTask(runnerMode, "evaluate", "citation:evaluate", evalArgs);

  if (!args.skipLabelTemplate) {
    const labelArgs = [
      "--input",
      workingDatasetPath,
      "--out",
      labelTemplatePath,
      "--top-k",
      String(args.labelTopK),
    ];
    if (args.labelIncludeLabeled) {
      labelArgs.push("--include-labeled");
    }
    runScriptTask(
      runnerMode,
      "generate label template",
      "citation:label-template",
      labelArgs,
    );
  }

  const syncArgs = ["--report", reportPath];
  if (args.writeDefaults) syncArgs.push("--write");
  if (args.allowUnlabeled) syncArgs.push("--allow-unlabeled");
  runScriptTask(
    runnerMode,
    "sync thresholds",
    "citation:sync-thresholds",
    syncArgs,
  );

  const unlabeledCount = readUnlabeledCount(unlabeledPath);
  console.log("\nArtifacts");
  console.log(`- dataset: ${datasetPath}`);
  if (labelsPath) {
    console.log(`- dataset.labeled: ${workingDatasetPath}`);
  }
  console.log(`- unlabeled: ${unlabeledPath} (count=${unlabeledCount})`);
  if (!args.skipLabelTemplate) {
    console.log(`- label-template: ${labelTemplatePath}`);
  }
  console.log(`- report: ${reportPath}`);
  console.log("\nPipeline finished.");
}

try {
  main();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  console.error(`citation-calibrate-pipeline failed: ${message}`);
  process.exit(1);
}
