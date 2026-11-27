// Connects the worker's wasm request handler to postMessage, answering the
// messages `worker.js` held while the wasm loaded first. Requests execute
// strictly one at a time in FIFO order even when a handler returns a Promise.

export function serve(handle) {
  let chain = Promise.resolve();
  const answer = (message) => {
    const text = typeof message === "string" ? message : "";
    chain = chain
      .then(() => handle(text))
      .then((reply) => {
        if (typeof reply === "string" && reply.length > 0) {
          self.postMessage(reply);
        }
      })
      .catch((error) => {
        self.postMessage(
          JSON.stringify({
            kind: "unaddressed",
            reason:
              error && error.message ? error.message : String(error),
          }),
        );
      });
  };
  const held = Array.isArray(self.ancestryInbox)
    ? self.ancestryInbox.splice(0)
    : [];
  self.onmessage = (event) => answer(event.data);
  for (const message of held) {
    answer(message);
  }
}

export function post_progress(message) {
  self.postMessage(message);
}

export async function fetch_text(url) {
  const response = await fetch(url, { credentials: "same-origin" });
  if (response.status === 404) {
    throw new Error("NOT_FOUND:" + url);
  }
  if (!response.ok) {
    throw new Error("HTTP " + response.status + " fetching " + url);
  }
  return await response.text();
}

export async function fetch_bytes(url) {
  const response = await fetch(url, { credentials: "same-origin" });
  if (response.status === 404) {
    throw new Error("NOT_FOUND:" + url);
  }
  if (!response.ok) {
    throw new Error("HTTP " + response.status + " fetching " + url);
  }
  return new Uint8Array(await response.arrayBuffer());
}
