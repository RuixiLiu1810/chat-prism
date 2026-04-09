import * as fs from "node:fs";
import * as path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

type Check = {
  id: string;
  label: string;
  run: () => void;
};

function desktopRoot(): string {
  const cwd = process.cwd();
  if (fs.existsSync(path.resolve(cwd, "src-tauri/Cargo.toml"))) {
    return cwd;
  }
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(scriptDir, "..");
}

function runCommand(label: string, cmd: string, args: string[]) {
  const proc = spawnSync(cmd, args, {
    cwd: desktopRoot(),
    encoding: "utf8",
    env: process.env,
  });
  if (proc.status !== 0) {
    const tail = `${proc.stdout || ""}\n${proc.stderr || ""}`
      .trim()
      .split("\n")
      .slice(-30)
      .join("\n");
    throw new Error(`${label} failed\n${tail}`);
  }
}

function checkTemplateSchema(filePath: string) {
  const raw = fs.readFileSync(filePath, "utf8");
  const parsed = JSON.parse(raw) as {
    id?: string;
    name?: string;
    sections?: Array<{ id?: string; label?: string }>;
  };
  if (!parsed.id || !parsed.name) {
    throw new Error(`${path.basename(filePath)} missing id/name`);
  }
  if (!Array.isArray(parsed.sections) || parsed.sections.length === 0) {
    throw new Error(`${path.basename(filePath)} missing sections[]`);
  }
  for (const [idx, section] of parsed.sections.entries()) {
    if (!section.id || !section.label) {
      throw new Error(
        `${path.basename(filePath)} section[${idx}] missing id/label`,
      );
    }
  }
}

function templateFiles(): string[] {
  const root = desktopRoot();
  const dir = path.resolve(root, "src-tauri/src/agent/templates");
  return [
    "imrad_standard.json",
    "review_article.json",
    "case_report.json",
    "methods_paper.json",
  ].map((name) => path.join(dir, name));
}

const checks: Check[] = [
  {
    id: "templates",
    label: "Validate academic writing templates",
    run: () => {
      for (const filePath of templateFiles()) {
        if (!fs.existsSync(filePath)) {
          throw new Error(`template file missing: ${filePath}`);
        }
        checkTemplateSchema(filePath);
      }
    },
  },
  {
    id: "typecheck",
    label: "TypeScript typecheck",
    run: () => runCommand("TypeScript typecheck", "npx", ["tsc", "--noEmit"]),
  },
  {
    id: "rust_check",
    label: "Rust cargo check",
    run: () =>
      runCommand("cargo check", "cargo", [
        "check",
        "--manifest-path",
        "src-tauri/Cargo.toml",
      ]),
  },
  {
    id: "workflow_tests",
    label: "Workflow/session regression tests",
    run: () => {
      runCommand(
        "session workflow snapshot test",
        "cargo",
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "workflow_snapshot_syncs_into_session_work_state",
        ],
      );
      runCommand(
        "session review memory test",
        "cargo",
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "review_and_revision_tool_results_are_persisted_in_work_state",
        ],
      );
      runCommand(
        "provider compatibility test",
        "cargo",
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "provider_transport_matrix_includes_deepseek",
        ],
      );
    },
  },
];

function main() {
  console.log("Agent workflow smoke check");
  for (const check of checks) {
    const started = Date.now();
    try {
      check.run();
      const sec = ((Date.now() - started) / 1000).toFixed(1);
      console.log(`[PASS] ${check.label} (${sec}s)`);
    } catch (error) {
      const sec = ((Date.now() - started) / 1000).toFixed(1);
      console.error(`[FAIL] ${check.label} (${sec}s)`);
      console.error(error instanceof Error ? error.message : String(error));
      process.exit(1);
    }
  }
  console.log("All checks passed.");
}

main();
