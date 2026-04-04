import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  settingsExport,
  settingsGet,
  settingsImport,
  settingsReset,
  settingsSet,
  settingsTestProviderConnectivity,
} from "@/lib/settings-api";

vi.mock("@/lib/debug/logger", () => ({
  createLogger: () => ({
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

describe("settings-api", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls settings_get with projectRoot", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      effective: {},
      global: { version: 1, data: {} },
      project: { version: 1, data: {} },
      secretsMeta: {
        integrations: {
          agent: { apiKeyConfigured: false },
          semanticScholar: { apiKeyConfigured: false },
          llmQuery: { apiKeyConfigured: false },
        },
      },
      warnings: [],
    });

    await settingsGet("/tmp/project");

    expect(invoke).toHaveBeenCalledWith("settings_get", {
      projectRoot: "/tmp/project",
    });
  });

  it("calls settings_set with wrapped args", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ok: true,
      errors: [],
      warnings: [],
    });

    await settingsSet({
      scope: "global",
      patch: { advanced: { debugEnabled: true } },
    });

    expect(invoke).toHaveBeenCalledWith("settings_set", {
      args: {
        scope: "global",
        patch: { advanced: { debugEnabled: true } },
        projectRoot: null,
      },
    });
  });

  it("calls settings_reset with keys", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ok: true,
      errors: [],
      warnings: [],
    });

    await settingsReset({
      scope: "project",
      keys: ["citation.search.limit"],
      projectRoot: "/tmp/project",
    });

    expect(invoke).toHaveBeenCalledWith("settings_reset", {
      args: {
        scope: "project",
        keys: ["citation.search.limit"],
        projectRoot: "/tmp/project",
      },
    });
  });

  it("calls settings_export with null args when omitted", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      data: { version: 1, global: {} },
      warnings: [],
    });

    await settingsExport();

    expect(invoke).toHaveBeenCalledWith("settings_export", {
      args: null,
    });
  });

  it("calls settings_import with mode and payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ok: true,
      errors: [],
      warnings: [],
    });

    await settingsImport({
      json: { global: { advanced: { logLevel: "debug" } } },
      mode: "merge",
      projectRoot: "/tmp/project",
    });

    expect(invoke).toHaveBeenCalledWith("settings_import", {
      args: {
        json: { global: { advanced: { logLevel: "debug" } } },
        mode: "merge",
        projectRoot: "/tmp/project",
      },
    });
  });

  it("normalizes string invoke errors into Error", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("boom");

    await expect(settingsGet()).rejects.toThrow("boom");
  });

  it("calls settings_test_provider_connectivity with projectRoot", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        provider: "semanticScholar",
        label: "Semantic Scholar",
        endpoint: "https://api.semanticscholar.org/graph/v1/paper/search",
        ok: true,
        reachable: true,
        status: 200,
        latencyMs: 123,
        message: "Connected.",
      },
    ]);

    await settingsTestProviderConnectivity("/tmp/project");

    expect(invoke).toHaveBeenCalledWith("settings_test_provider_connectivity", {
      args: {
        projectRoot: "/tmp/project",
      },
    });
  });
});
