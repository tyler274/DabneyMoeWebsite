// Starts the ancestry app. A file rather than an inline script so the page's
// policy can forbid inline scripts. The export script stamps the import with
// a cache-busting version.
import init from "./ancestry-app.js?v=102566b7e50d";

await init();
