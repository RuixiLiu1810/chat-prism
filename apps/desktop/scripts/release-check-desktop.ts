import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

type Step = {
  id: string;
  label: string;
  cmd: string;
  args: string[];
  required: boolean;
  env?: NodeJS.ProcessEnv;
};

type StepResult = {
  step: Step;
  ok: boolean;
  code: number | null;
  durationMs: number;
  stdout: string;
  stderr: string;
};

type ToolchainMode = "pnpm" | "local-bin";

function runStep(step: Step): StepResult {
  const startedAt = Date.now();
  const proc = spawnSync(step.cmd, step.args, {
    encoding: "utf8",
    cwd: process.cwd(),
    env: step.env ? { ...process.env, ...step.env } : process.env,
  });
  const durationMs = Date.now() - startedAt;
  return {
    step,
    ok: proc.status === 0,
    code: proc.status,
    durationMs,
    stdout: proc.stdout ?? "",
    stderr: proc.stderr ?? "",
  };
}

function printStepResult(result: StepResult) {
  const status = result.ok ? "PASS" : "FAIL";
  const sec = (result.durationMs / 1000).toFixed(1);
  console.log(`[${status}] ${result.step.label} (${sec}s)`);

  if (result.ok) return;

  const combined = `${result.stdout}\n${result.stderr}`.trim();
  const snippet = combined.split("\n").slice(-24).join("\n");
  if (snippet) {
    console.log("---- failure tail ----");
    console.log(snippet);
    console.log("----------------------");
  }

  const lower = combined.toLowerCase();
  if (
    lower.includes("icu-uc") ||
    lower.includes("harfbuzz") ||
    lower.includes("pkg-config")
  ) {
    console.log(
      "hint: rust deps missing. try: brew install pkg-config icu4c harfbuzz",
    );
    console.log(
      "hint: then export PKG_CONFIG_PATH like /opt/homebrew/opt/icu4c/lib/pkgconfig",
    );
  }
}

function commandAvailable(cmd: string): boolean {
  const probe = spawnSync(cmd, ["--version"], {
    encoding: "utf8",
    cwd: process.cwd(),
    env: process.env,
  });
  return probe.status === 0;
}

function desktopRootDir(): string {
  const thisDir = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(thisDir, "..");
}

function localBinCommand(name: "tsc" | "vitest"): string | null {
  const desktopRoot = desktopRootDir();
  const cmd = path.resolve(desktopRoot, "../../node_modules/.bin", name);
  return fs.existsSync(cmd) ? cmd : null;
}

function buildSteps(withRust: boolean): { mode: ToolchainMode; steps: Step[] } {
  const hasPnpm = commandAvailable("pnpm");
  const localTsc = localBinCommand("tsc");
  const localVitest = localBinCommand("vitest");
  const canUseLocalBins = Boolean(localTsc && localVitest);

  let mode: ToolchainMode = "pnpm";
  const steps: Step[] = [];

  if (hasPnpm) {
    mode = "pnpm";
    steps.push(
      {
        id: "typecheck",
        label: "TypeScript type check",
        cmd: "pnpm",
        args: ["exec", "tsc", "--noEmit"],
        required: true,
      },
      {
        id: "unit",
        label: "Vitest full suite",
        cmd: "pnpm",
        args: ["test"],
        required: true,
      },
    );
  } else if (canUseLocalBins) {
    mode = "local-bin";
    steps.push(
      {
        id: "typecheck",
        label: "TypeScript type check",
        cmd: localTsc as string,
        args: ["--noEmit"],
        required: true,
      },
      {
        id: "unit",
        label: "Vitest full suite",
        cmd: localVitest as string,
        args: ["run"],
        required: true,
      },
    );
  } else {
    // Keep prior behavior if no fallback is available.
    steps.push(
      {
        id: "typecheck",
        label: "TypeScript type check",
        cmd: "pnpm",
        args: ["exec", "tsc", "--noEmit"],
        required: true,
      },
      {
        id: "unit",
        label: "Vitest full suite",
        cmd: "pnpm",
        args: ["test"],
        required: true,
      },
    );
  }

  if (withRust) {
    const rustEnv = buildRustEnvHints();
    steps.push({
      id: "rust",
      label: "Rust cargo check",
      cmd: "cargo",
      args: ["check", "--manifest-path", "src-tauri/Cargo.toml"],
      required: false,
      env: Object.keys(rustEnv).length > 0 ? rustEnv : undefined,
    });
  }

  return { mode, steps };
}

function splitPathList(raw: string | undefined): string[] {
  if (!raw) return [];
  return raw
    .split(":")
    .map((x) => x.trim())
    .filter((x) => x.length > 0);
}

function mergePathList(existing: string | undefined, extra: string[]): string | undefined {
  const merged = [...splitPathList(existing), ...extra];
  const seen = new Set<string>();
  const deduped: string[] = [];
  for (const item of merged) {
    if (seen.has(item)) continue;
    seen.add(item);
    deduped.push(item);
  }
  return deduped.length > 0 ? deduped.join(":") : undefined;
}

function mergeFlagList(existing: string | undefined, extra: string[]): string | undefined {
  const merged = [
    ...(existing ?? "")
      .split(/\s+/)
      .map((x) => x.trim())
      .filter((x) => x.length > 0),
    ...extra,
  ];
  const seen = new Set<string>();
  const deduped: string[] = [];
  for (const item of merged) {
    if (seen.has(item)) continue;
    seen.add(item);
    deduped.push(item);
  }
  return deduped.length > 0 ? deduped.join(" ") : undefined;
}

function buildRustEnvHints(): NodeJS.ProcessEnv {
  if (process.platform !== "darwin") {
    return {};
  }

  const env: NodeJS.ProcessEnv = {};

  const pkgConfigDirs = [
    "/opt/homebrew/opt/icu4c/lib/pkgconfig",
    "/opt/homebrew/opt/harfbuzz/lib/pkgconfig",
    "/opt/homebrew/opt/graphite2/lib/pkgconfig",
    "/opt/homebrew/opt/freetype/lib/pkgconfig",
    "/opt/homebrew/opt/glib/lib/pkgconfig",
    "/opt/homebrew/opt/libpng/lib/pkgconfig",
  ].filter((dir) => fs.existsSync(dir));
  const mergedPkgConfigPath = mergePathList(process.env.PKG_CONFIG_PATH, pkgConfigDirs);
  if (mergedPkgConfigPath) {
    env.PKG_CONFIG_PATH = mergedPkgConfigPath;
  }

  const harfbuzzInclude = "/opt/homebrew/opt/harfbuzz/include";
  if (fs.existsSync(path.join(harfbuzzInclude, "harfbuzz", "hb.h"))) {
    const mergedCflags = mergeFlagList(process.env.CFLAGS, [`-I${harfbuzzInclude}`]);
    if (mergedCflags) {
      env.CFLAGS = mergedCflags;
    }
    const mergedCxxflags = mergeFlagList(process.env.CXXFLAGS, [
      "-std=c++17",
      `-I${harfbuzzInclude}`,
    ]);
    if (mergedCxxflags) {
      env.CXXFLAGS = mergedCxxflags;
    }
  }

  return env;
}

function main() {
  const args = new Set(process.argv.slice(2));
  const withRust = args.has("--with-rust");
  const { mode, steps } = buildSteps(withRust);

  console.log("Desktop release check");
  console.log(`cwd: ${process.cwd()}`);
  console.log(`withRust: ${withRust ? "yes" : "no"}`);
  console.log(`toolchain: ${mode}`);

  const results: StepResult[] = [];
  for (const step of steps) {
    const rendered = [step.cmd, ...step.args].join(" ");
    console.log(`\n> ${step.label}`);
    console.log(`$ ${rendered}`);
    const result = runStep(step);
    results.push(result);
    printStepResult(result);
  }

  const requiredFailed = results.filter((r) => r.step.required && !r.ok);
  const optionalFailed = results.filter((r) => !r.step.required && !r.ok);

  console.log("\nSummary");
  for (const r of results) {
    console.log(
      `- ${r.ok ? "PASS" : "FAIL"} ${r.step.id} (${(r.durationMs / 1000).toFixed(1)}s)`,
    );
  }

  if (requiredFailed.length === 0 && optionalFailed.length === 0) {
    console.log("\nRelease check passed.");
    process.exit(0);
  }

  if (requiredFailed.length === 0 && optionalFailed.length > 0) {
    console.log("\nRelease check passed with optional failures.");
    process.exit(0);
  }

  console.log("\nRelease check failed.");
  process.exit(1);
}

main();
