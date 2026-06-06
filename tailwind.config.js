/** @type {import('tailwindcss').Config} */
module.exports = {
  // Scan every Leptos crate plus the Trunk entry HTML so utility classes
  // referenced from Rust `view!` macros are retained.
  content: [
    "./crates/**/*.rs",
    "./crates/tauri-ui/index.html",
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "Inter",
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "Segoe UI",
          "Roboto",
          "Helvetica Neue",
          "Arial",
          "sans-serif",
        ],
      },
    },
  },
  plugins: [],
};
