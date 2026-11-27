// Starts the analysis worker for `src/worker_client.rs` and carries its
// messages. Messages are JSON strings in both directions; this file never
// reads them. A worker that fails to load or crashes is reported as an
// unaddressed failure, so pending requests fail instead of waiting forever.

// The worker module sits beside the page's index.html. The export script
// stamps this address with a cache-busting version.
const WORKER_URL = "./worker.js?v=102566b7e50d";

function unaddressed(reason) {
  return JSON.stringify({ kind: "unaddressed", reason: String(reason) });
}

export function start_worker(on_message) {
  const url = new URL(WORKER_URL, document.baseURI);
  const worker = new Worker(url, { type: "module", name: "ancestry-analysis" });
  worker.addEventListener("message", (event) => {
    on_message(typeof event.data === "string" ? event.data : unaddressed("the worker sent a non-text message"));
  });
  worker.addEventListener("error", (event) => {
    event.preventDefault();
    on_message(unaddressed(event.message || "the analysis worker failed to load"));
  });
  worker.addEventListener("messageerror", () => {
    on_message(unaddressed("a worker message could not be read"));
  });
  return worker;
}

export function post(worker, message) {
  worker.postMessage(message);
}
