import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";

const ZOTERO_BASE = "https://api.zotero.org";

export interface ZoteroCredentials {
  apiKey: string;
  userID: string;
  username: string;
}

export interface ZoteroCollection {
  key: string;
  name: string;
  parentKey: string | false;
  itemCount: number;
}

/** Result of importing a collection */
export interface CollectionImportResult {
  bibtex: string;
  libraryVersion: number;
  keyMap: Record<string, string>;
  totalItems: number;
}

/** Result of an incremental sync */
export interface CollectionSyncResult {
  updatedEntries: { key: string; citekey: string; bibtex: string }[];
  deletedKeys: string[];
  libraryVersion: number;
}

export interface CitationToZoteroInput {
  title: string;
  authors: string[];
  year?: number;
  venue?: string;
  doi?: string;
  url?: string;
}

export interface ZoteroUpsertOptions {
  collectionName?: string;
}

// ─── OAuth Flow (via Tauri Rust backend) ───

export async function startOAuth(): Promise<void> {
  const result = await invoke<{ authorize_url: string }>("zotero_start_oauth");
  await open(result.authorize_url);
}

export async function completeOAuth(): Promise<ZoteroCredentials> {
  const result = await invoke<{
    api_key: string;
    user_id: string;
    username: string;
  }>("zotero_complete_oauth");
  return {
    apiKey: result.api_key,
    userID: result.user_id,
    username: result.username,
  };
}

export async function cancelOAuth(): Promise<void> {
  await invoke("zotero_cancel_oauth");
}

// ─── Zotero Web API v3 ───

async function zoteroFetch(
  apiKey: string,
  path: string,
  headers?: Record<string, string>,
): Promise<Response> {
  const response = await fetch(`${ZOTERO_BASE}${path}`, {
    headers: {
      "Zotero-API-Key": apiKey,
      "Zotero-API-Version": "3",
      ...headers,
    },
  });
  if (!response.ok) {
    if (response.status === 304) return response;
    if (response.status === 403) throw new Error("Invalid or expired API key");
    throw new Error(`Zotero API error: ${response.status}`);
  }
  return response;
}

function extractCitekey(bibtex: string): string {
  const match = bibtex.match(/@\w+\{([^,\s]+)/);
  return match ? match[1] : "";
}

export async function validateApiKey(
  apiKey: string,
): Promise<ZoteroCredentials> {
  const response = await zoteroFetch(apiKey, "/keys/current");
  const data = await response.json();
  return {
    apiKey,
    userID: String(data.userID),
    username: data.username ?? "",
  };
}

// ─── Collections ───

export async function fetchCollections(
  apiKey: string,
  userID: string,
): Promise<ZoteroCollection[]> {
  const response = await zoteroFetch(
    apiKey,
    `/users/${userID}/collections?format=json`,
  );
  const data = (await response.json()) as {
    key: string;
    data: { key: string; name: string; parentCollection: string | false };
    meta: { numItems: number };
  }[];
  return data.map((c) => ({
    key: c.key,
    name: c.data.name,
    parentKey: c.data.parentCollection,
    itemCount: c.meta.numItems,
  }));
}

// ─── Collection Import (full download) ───

/**
 * Import all items from a specific collection.
 * Pass collectionKey = null to import the entire "My Library" (all top-level items).
 */
export async function importCollection(
  apiKey: string,
  userID: string,
  collectionKey: string | null,
  onProgress?: (loaded: number, total: number) => void,
): Promise<CollectionImportResult> {
  const basePath = collectionKey
    ? `/users/${userID}/collections/${collectionKey}/items/top`
    : `/users/${userID}/items/top`;

  let allBibtex = "";
  const keyMap: Record<string, string> = {};
  let start = 0;
  const limit = 100;
  let total = 0;
  let libraryVersion = 0;

  while (true) {
    const params = new URLSearchParams({
      format: "json",
      include: "bibtex",
      limit: String(limit),
      start: String(start),
    });
    const response = await zoteroFetch(apiKey, `${basePath}?${params}`);

    if (start === 0) {
      total = Number(response.headers.get("Total-Results") ?? 0);
      libraryVersion = Number(
        response.headers.get("Last-Modified-Version") ?? 0,
      );
    }

    const items = (await response.json()) as { key: string; bibtex?: string }[];
    if (items.length === 0) break;

    for (const item of items) {
      const bibtex = item.bibtex ?? "";
      if (!bibtex.trim()) continue;
      const citekey = extractCitekey(bibtex);
      if (citekey) keyMap[item.key] = citekey;
      allBibtex += (allBibtex ? "\n\n" : "") + bibtex;
    }

    start += limit;
    onProgress?.(Math.min(start, total), total);
    if (start >= total) break;
  }

  return { bibtex: allBibtex, libraryVersion, keyMap, totalItems: total };
}

// ─── Incremental Sync ───

/**
 * Sync changes for a specific collection since lastVersion.
 * collectionKey = null syncs the entire library.
 *
 * Note: Zotero's `since` param works at the library level (not per-collection),
 * so for collection sync we re-fetch all collection items and diff locally.
 */
export async function syncCollection(
  apiKey: string,
  userID: string,
  collectionKey: string | null,
  lastVersion: number,
  onProgress?: (loaded: number, total: number) => void,
): Promise<CollectionSyncResult> {
  // For "My Library" (all items), we can use the `since` param
  if (!collectionKey) {
    return syncFullLibrary(apiKey, userID, lastVersion, onProgress);
  }

  // For a specific collection, re-fetch all items and diff against keyMap
  // (Zotero API doesn't support `since` scoped to a collection)
  const result = await importCollection(
    apiKey,
    userID,
    collectionKey,
    onProgress,
  );

  return {
    updatedEntries: Object.entries(result.keyMap).map(([key, citekey]) => {
      // Extract the bibtex for this citekey from the full bibtex string
      const bibtexEntries = result.bibtex.split(/\n(?=@)/);
      const entry =
        bibtexEntries.find((e) => extractCitekey(e) === citekey) ?? "";
      return { key, citekey, bibtex: entry };
    }),
    deletedKeys: [],
    libraryVersion: result.libraryVersion,
  };
}

async function syncFullLibrary(
  apiKey: string,
  userID: string,
  lastVersion: number,
  onProgress?: (loaded: number, total: number) => void,
): Promise<CollectionSyncResult> {
  const updatedEntries: CollectionSyncResult["updatedEntries"] = [];
  let start = 0;
  const limit = 100;
  let total = 0;
  let newVersion = lastVersion;

  while (true) {
    const params = new URLSearchParams({
      since: String(lastVersion),
      format: "json",
      include: "bibtex",
      limit: String(limit),
      start: String(start),
    });
    const response = await zoteroFetch(
      apiKey,
      `/users/${userID}/items/top?${params}`,
    );

    if (start === 0) {
      total = Number(response.headers.get("Total-Results") ?? 0);
      newVersion = Number(
        response.headers.get("Last-Modified-Version") ?? lastVersion,
      );
    }

    const items = (await response.json()) as { key: string; bibtex?: string }[];
    if (items.length === 0) break;

    for (const item of items) {
      const bibtex = item.bibtex ?? "";
      if (!bibtex.trim()) continue;
      const citekey = extractCitekey(bibtex);
      updatedEntries.push({ key: item.key, citekey, bibtex });
    }

    start += limit;
    onProgress?.(Math.min(start, total), total);
    if (start >= total) break;
  }

  // Fetch deleted items
  const deletedResponse = await zoteroFetch(
    apiKey,
    `/users/${userID}/deleted?since=${lastVersion}`,
  );
  const deleted = (await deletedResponse.json()) as { items?: string[] };
  const deletedKeys = deleted.items ?? [];

  if (!newVersion || newVersion === lastVersion) {
    newVersion = Number(
      deletedResponse.headers.get("Last-Modified-Version") ?? lastVersion,
    );
  }

  return { updatedEntries, deletedKeys, libraryVersion: newVersion };
}

function mapCreators(authors: string[]) {
  return authors
    .map((name) => name.trim())
    .filter(Boolean)
    .map((name) => {
      const parts = name.split(/\s+/);
      if (parts.length <= 1) {
        return { creatorType: "author", name };
      }
      return {
        creatorType: "author",
        firstName: parts.slice(0, -1).join(" "),
        lastName: parts[parts.length - 1],
      };
    });
}

async function findItemKeyByDoi(
  apiKey: string,
  userID: string,
  doi: string,
): Promise<string | null> {
  const normalizeDoi = (value: string) =>
    value.trim().toLowerCase().replace(/^https?:\/\/doi\.org\//, "");
  const response = await zoteroFetch(
    apiKey,
    `/users/${userID}/items?q=${encodeURIComponent(doi)}&qmode=everything&format=json&limit=20`,
  );
  const data = (await response.json()) as {
    key: string;
    data?: { DOI?: string };
  }[];
  const norm = normalizeDoi(doi);
  const hit = data.find(
    (item) => {
      const value = item.data?.DOI;
      return value ? normalizeDoi(value) === norm : false;
    },
  );
  return hit?.key ?? null;
}

async function ensureItemInCollection(
  apiKey: string,
  userID: string,
  itemKey: string,
  collectionKey: string,
): Promise<void> {
  const itemResp = await zoteroFetch(
    apiKey,
    `/users/${userID}/items/${itemKey}?format=json`,
  );
  const item = (await itemResp.json()) as {
    key?: string;
    version?: number;
    data?: Record<string, unknown> & { collections?: string[] };
  };
  const currentCollections = Array.isArray(item.data?.collections)
    ? item.data.collections
    : [];
  if (currentCollections.includes(collectionKey)) return;

  const nextCollections = Array.from(
    new Set([...currentCollections, collectionKey]),
  );

  if (item.data) {
    const putResponse = await fetch(`${ZOTERO_BASE}/users/${userID}/items/${itemKey}`, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        "Zotero-API-Key": apiKey,
        "Zotero-API-Version": "3",
        ...(typeof item.version === "number"
          ? { "If-Unmodified-Since-Version": String(item.version) }
          : {}),
      },
      body: JSON.stringify({
        ...item.data,
        collections: nextCollections,
      }),
    });
    if (putResponse.ok) return;
  }

  const tryPayloads = [JSON.stringify([itemKey]), JSON.stringify([{ key: itemKey }])];
  for (const body of tryPayloads) {
    const response = await fetch(
      `${ZOTERO_BASE}/users/${userID}/collections/${collectionKey}/items`,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Zotero-API-Key": apiKey,
          "Zotero-API-Version": "3",
        },
        body,
      },
    );
    if (response.ok) return;
  }

  throw new Error(
    `Zotero ensure-collection failed for item ${itemKey} -> ${collectionKey}`,
  );
}

async function findOrCreateTopLevelCollection(
  apiKey: string,
  userID: string,
  name: string,
): Promise<string> {
  const existing = (await fetchCollections(apiKey, userID)).find(
    (c) => c.parentKey === false && c.name === name,
  );
  if (existing) return existing.key;

  const response = await fetch(`${ZOTERO_BASE}/users/${userID}/collections`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Zotero-API-Key": apiKey,
      "Zotero-API-Version": "3",
    },
    body: JSON.stringify([{ name }]),
  });
  if (!response.ok) {
    throw new Error(`Zotero create collection failed: ${response.status}`);
  }
  const result = (await response.json()) as {
    success?: Record<string, string>;
  };
  const key = result.success?.["0"];
  if (!key) {
    throw new Error("Zotero create collection failed: empty collection key");
  }
  return key;
}

export async function upsertZoteroItemFromCitation(
  apiKey: string,
  userID: string,
  input: CitationToZoteroInput,
  options?: ZoteroUpsertOptions,
): Promise<{ key: string | null; created: boolean }> {
  let collectionKey: string | null = null;
  const collectionName = options?.collectionName?.trim();
  if (collectionName) {
    collectionKey = await findOrCreateTopLevelCollection(
      apiKey,
      userID,
      collectionName,
    );
  }

  if (input.doi?.trim()) {
    const existing = await findItemKeyByDoi(apiKey, userID, input.doi);
    if (existing) {
      if (collectionKey) {
        await ensureItemInCollection(apiKey, userID, existing, collectionKey);
      }
      return { key: existing, created: false };
    }
  }

  const payload = [
    {
      itemType: "journalArticle",
      title: input.title,
      creators: mapCreators(input.authors),
      date: input.year ? String(input.year) : undefined,
      publicationTitle: input.venue,
      DOI: input.doi,
      url: input.url,
      tags: [{ tag: "source:semantic-scholar" }],
      collections: collectionKey ? [collectionKey] : undefined,
    },
  ];

  const response = await fetch(`${ZOTERO_BASE}/users/${userID}/items`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Zotero-API-Key": apiKey,
      "Zotero-API-Version": "3",
    },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(`Zotero create item failed: ${response.status}`);
  }
  const result = (await response.json()) as {
    success?: Record<string, string>;
  };
  const key = result.success?.["0"] ?? null;
  if (key && collectionKey) {
    await ensureItemInCollection(apiKey, userID, key, collectionKey);
  }
  return { key, created: true };
}
