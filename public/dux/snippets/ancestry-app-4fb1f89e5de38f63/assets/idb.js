// IndexedDB storage for the ancestry app, called only from `src/storage.rs`.
// Metadata and text live in separate stores so listing never reads text.
// Every function returns a Promise that rejects with the browser's error.

const DB_NAME = "dabney-moe-ancestry";
const DB_VERSION = 1;
const SHEETS = "sheets";
const SHEET_TEXT = "sheet_text";
const WEIGHTS = "weights";
const WEIGHTS_KEY = "current";

let opening = null;

function open() {
  if (!opening) {
    opening = new Promise((resolve, reject) => {
      if (typeof indexedDB === "undefined") {
        reject(new Error("this browser offers no IndexedDB"));
        return;
      }
      const request = indexedDB.open(DB_NAME, DB_VERSION);
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains(SHEETS)) {
          db.createObjectStore(SHEETS, { keyPath: "id", autoIncrement: true });
        }
        if (!db.objectStoreNames.contains(SHEET_TEXT)) {
          db.createObjectStore(SHEET_TEXT);
        }
        if (!db.objectStoreNames.contains(WEIGHTS)) {
          db.createObjectStore(WEIGHTS);
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error("storage is held open by another tab"));
    });
    opening.catch(() => {
      opening = null;
    });
  }
  return opening;
}

function done(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error || new Error("storage transaction aborted"));
  });
}

function result(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function list_sheets() {
  const db = await open();
  const tx = db.transaction(SHEETS, "readonly");
  const all = await result(tx.objectStore(SHEETS).getAll());
  await done(tx);
  return JSON.stringify(all);
}

export async function add_sheet(meta, text) {
  const db = await open();
  const tx = db.transaction([SHEETS, SHEET_TEXT], "readwrite");
  const id = await result(tx.objectStore(SHEETS).add(JSON.parse(meta)));
  tx.objectStore(SHEET_TEXT).put(text, id);
  await done(tx);
  return id;
}

export async function sheet_text(id) {
  const db = await open();
  const tx = db.transaction(SHEET_TEXT, "readonly");
  const text = await result(tx.objectStore(SHEET_TEXT).get(id));
  await done(tx);
  return typeof text === "string" ? text : null;
}

export async function set_sheet_space(id, space) {
  const db = await open();
  const tx = db.transaction(SHEETS, "readwrite");
  const store = tx.objectStore(SHEETS);
  const meta = await result(store.get(id));
  if (!meta) {
    tx.abort();
    throw new Error("the sheet is no longer stored");
  }
  meta.space = JSON.parse(space);
  store.put(meta);
  await done(tx);
}

export async function delete_sheet(id) {
  const db = await open();
  const tx = db.transaction([SHEETS, SHEET_TEXT], "readwrite");
  tx.objectStore(SHEETS).delete(id);
  tx.objectStore(SHEET_TEXT).delete(id);
  await done(tx);
}

export async function weights_meta() {
  const db = await open();
  const tx = db.transaction(WEIGHTS, "readonly");
  const stored = await result(tx.objectStore(WEIGHTS).get(WEIGHTS_KEY));
  await done(tx);
  return stored ? stored.meta : null;
}

export async function weights_text() {
  const db = await open();
  const tx = db.transaction(WEIGHTS, "readonly");
  const stored = await result(tx.objectStore(WEIGHTS).get(WEIGHTS_KEY));
  await done(tx);
  return stored ? stored.text : null;
}

export async function put_weights(meta, text) {
  const db = await open();
  const tx = db.transaction(WEIGHTS, "readwrite");
  tx.objectStore(WEIGHTS).put({ meta, text }, WEIGHTS_KEY);
  await done(tx);
}

export async function delete_weights() {
  const db = await open();
  const tx = db.transaction(WEIGHTS, "readwrite");
  tx.objectStore(WEIGHTS).delete(WEIGHTS_KEY);
  await done(tx);
}

// Triggers a client-side file download from a base64-encoded binary payload.
export function download_base64(filename, base64) {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  const blob = new Blob([bytes], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename || "weights.ancw";
  link.style.display = "none";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// Triggers a client-side file download from UTF-8 text (JSON or Markdown report).
export function download_text(filename, content, mime) {
  const blob = new Blob([content], { type: mime || "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename || "report.txt";
  link.style.display = "none";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// Decodes UTF-8 bytes, transparently inflating gzip (magic 0x1f 0x8b) via the
// browser's native DecompressionStream without third-party dependencies.
export async function inflate_gzip_or_utf8(bytes) {
  if (bytes && bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b) {
    if (typeof DecompressionStream === "undefined") {
      throw new Error("this browser cannot decompress .gz files directly; please decompress the file first");
    }
    const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
    return await new Response(stream).text();
  }
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}
