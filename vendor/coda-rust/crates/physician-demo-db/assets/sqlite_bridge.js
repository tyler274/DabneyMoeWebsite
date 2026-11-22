// Browser snapshot fetch + SHA-256-keyed IndexedDB persistence for pure-Rust `turso_core`.
const IDB_NAME = "coda_physician_demo_sqlite", IDB_STORE = "snapshots";
const PERSIST_DEBOUNCE_MS = 200;

let seedBytes = null, snapshotKey = null, seedPromise = null, exportBytesFn = null;
let persistQueue = Promise.resolve();
let persistTimer = null;
let lifecycleHooked = false;

function persistSnapshotNow() {
  if (!exportBytesFn || !snapshotKey) return persistQueue;
  const bytes = exportBytesFn(), key = snapshotKey;
  persistQueue = persistQueue
    .then(() => idbOp("readwrite", (s) => s.put(bytes, key)))
    .catch(() => false);
  return persistQueue;
}

function flushPendingPersist() {
  if (persistTimer === null) return persistQueue;
  clearTimeout(persistTimer);
  persistTimer = null;
  return persistSnapshotNow();
}

function ensureLifecycleFlush() {
  if (lifecycleHooked) return;
  lifecycleHooked = true;
  if (typeof window !== "undefined") {
    window.addEventListener("pagehide", () => { void flushPendingPersist(); });
  }
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") void flushPendingPersist();
    });
  }
}

function assetUrl(path) {
  const u = new URL(window.location.href);
  u.search = ""; u.hash = "";
  if (!u.pathname.endsWith("/")) u.pathname += "/";
  return new URL(path, u.href).href;
}

async function digestKey(bytes) {
  if (!window.crypto?.subtle) return `seed-len-${bytes.byteLength}`;
  const hash = await window.crypto.subtle.digest("SHA-256", bytes);
  return "seed-" + Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("").slice(0, 16);
}

async function idbOp(mode, run) {
  if (!window.indexedDB) return null;
  try {
    const idb = await new Promise((resolve, reject) => {
      const req = window.indexedDB.open(IDB_NAME, 1);
      req.onupgradeneeded = () => { if (!req.result.objectStoreNames.contains(IDB_STORE)) req.result.createObjectStore(IDB_STORE); };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
    return await new Promise((resolve) => {
      const tx = idb.transaction(IDB_STORE, mode);
      const req = run(tx.objectStore(IDB_STORE));
      if (mode === "readonly") {
        req.onsuccess = () => resolve(req.result instanceof Uint8Array ? req.result : null);
        req.onerror = () => resolve(null);
      } else {
        tx.oncomplete = () => resolve(true);
        tx.onerror = () => resolve(false);
      }
    });
  } catch { return null; }
}

export function schedulePersist() {
  if (!exportBytesFn || !snapshotKey) return;
  if (persistTimer !== null) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    void persistSnapshotNow();
  }, PERSIST_DEBOUNCE_MS);
}

export async function initDemoDb(dbPath, exportFn) {
  ensureLifecycleFlush();
  if (!seedPromise) {
    seedPromise = (async () => {
      const res = await fetch(assetUrl(dbPath), dbPath.includes("?") ? undefined : { cache: "no-cache" });
      if (!res.ok) throw new Error(`failed to fetch ${dbPath}: HTTP ${res.status}`);
      seedBytes = new Uint8Array(await res.arrayBuffer());
      snapshotKey = await digestKey(seedBytes);
    })().catch((err) => {
      seedPromise = null;
      throw err;
    });
  }
  await seedPromise;
  await flushPendingPersist();
  await persistQueue;
  exportBytesFn = exportFn;
  const saved = await idbOp("readonly", (s) => s.get(snapshotKey));
  return (saved && saved.byteLength > 0 ? saved : seedBytes).slice();
}

export async function resetDemoDb() {
  if (!seedBytes) throw new Error("demo SQLite database is not initialized");
  if (persistTimer !== null) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  await persistQueue;
  if (snapshotKey) await idbOp("readwrite", (s) => s.delete(snapshotKey));
  return seedBytes.slice();
}
