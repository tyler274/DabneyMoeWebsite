# System libraries Tauri's webview needs on Linux desktops, plus the
# pkg-config / linker search paths derived from their propagated closure.
{ pkgs }:
let
  tauriDeps = with pkgs; [
    webkitgtk_4_1
    gtk3
    libsoup_3
    cairo
    pango
    harfbuzz
    atk
    atkmm
    gdk-pixbuf
    glib
    librsvg
    openssl
    # `libdbus-sys` (pulled in transitively by Tauri on Linux) links
    # against the system D-Bus library.
    dbus
    # `Requires.private` of the gtk `.pc` files that aren't propagated.
    zlib
  ];

  # The gtk/webkit `.pc` files declare transitive `Requires:` (pango ->
  # harfbuzz, gdk -> zlib, ...). Close over propagated build inputs so
  # every required `.pc` is discoverable without hand-listing each one.
  tauriDepsClosure = pkgs.lib.closePropagation tauriDeps;
in
{
  inherit tauriDeps tauriDepsClosure;

  # Make pkg-config-discovered libs (webkit/gtk) visible to the linker.
  # Some `.pc` files live in share/pkgconfig (e.g. zlib) rather than
  # lib/pkgconfig, so search both.
  pkgConfigPath = pkgs.lib.concatStringsSep ":" [
    (pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" tauriDepsClosure)
    (pkgs.lib.makeSearchPathOutput "dev" "share/pkgconfig" tauriDepsClosure)
  ];
  ldLibraryPath = pkgs.lib.makeLibraryPath tauriDepsClosure;
}
