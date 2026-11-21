// Browser SQLite bridge backed by SHA-256-verified sql.js + IndexedDB persistence.
const IDB_NAME = "coda_physician_demo_sqlite", IDB_STORE = "snapshots";
let sqlModule = null, db = null, seedBytes = null, snapshotKey = null, initPromise = null;
let persistQueue = Promise.resolve();
let persistTimer = null;
let lifecycleHooked = false;

function persistSnapshotNow() {
  if (!db || !snapshotKey) return persistQueue;
  const bytes = db.export(), key = snapshotKey;
  persistQueue = persistQueue.then(() => idbOp("readwrite", (s) => s.put(bytes, key)));
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

function loadSqlJs() {
  if (typeof window.initSqlJs === "function") return Promise.resolve();
  return new Promise((resolve, reject) => {
    const cur = document.querySelector('script[data-coda-sqljs="1"]');
    const el = cur || Object.assign(document.createElement("script"), { src: assetUrl("./sql-wasm.js?v=eda696d6406e") });
    el.dataset.codaSqljs = "1";
    el.addEventListener("load", () => resolve(), { once: true });
    el.addEventListener("error", () => reject(new Error("failed to load sql-wasm.js")), { once: true });
    if (!cur) document.head.appendChild(el);
  });
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

function schedulePersist() {
  if (!db || !snapshotKey) return;
  if (persistTimer !== null) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    void persistSnapshotNow();
  }, 200);
}

export async function initDemoDb(dbPath) {
  ensureLifecycleFlush();
  if (db) return;
  if (!initPromise) {
    initPromise = (async () => {
      await loadSqlJs();
      sqlModule = await window.initSqlJs({ locateFile: (f) => assetUrl(`./${f}?v=eda696d6406e`) });
      const res = await fetch(assetUrl(dbPath), dbPath.includes("?") ? undefined : { cache: "no-cache" });
      if (!res.ok) throw new Error(`failed to fetch ${dbPath}: HTTP ${res.status}`);
      seedBytes = new Uint8Array(await res.arrayBuffer());
      snapshotKey = await digestKey(seedBytes);
      const saved = await idbOp("readonly", (s) => s.get(snapshotKey));
      db = new sqlModule.Database(saved || seedBytes.slice());
    })();
  }
  await initPromise;
}

function runStmt(sql, paramsJson, collect) {
  if (!db) throw new Error("demo SQLite database is not initialized");
  const stmt = db.prepare(sql);
  try {
    stmt.bind(JSON.parse(paramsJson));
    const rows = [];
    while (stmt.step()) if (collect) rows.push(stmt.getAsObject());
    return rows;
  } finally { stmt.free(); }
}

export function querySql(sql, paramsJson) { return JSON.stringify(runStmt(sql, paramsJson, true)); }
export function execSql(sql, paramsJson) { runStmt(sql, paramsJson, false); schedulePersist(); }

export async function resetDemoDb() {
  if (!sqlModule || !seedBytes) throw new Error("demo SQLite database is not initialized");
  if (persistTimer !== null) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  await persistQueue;
  if (snapshotKey) await idbOp("readwrite", (s) => s.delete(snapshotKey));
  if (db) db.close();
  db = new sqlModule.Database(seedBytes.slice());
}
