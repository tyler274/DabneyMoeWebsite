// The analysis worker's module. It holds messages that arrive while the wasm
// loads, then `src/bin/ancestry-worker.rs` takes them over through
// `worker_host.js`. A failed load is posted back so the page sees it.

const inbox = [];
self.ancestryInbox = inbox;
self.onmessage = (event) => {
  inbox.push(event.data);
};

// The export script stamps this import with a cache-busting version.
import("./ancestry-worker.js?v=102566b7e50d")
  .then((glue) => glue.default())
  .catch((error) => {
    self.postMessage(
      JSON.stringify({
        kind: "unaddressed",
        reason: "the analysis worker failed to start: " + (error && error.message ? error.message : String(error)),
      }),
    );
  });
