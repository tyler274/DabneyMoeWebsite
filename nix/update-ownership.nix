# Deterministic code-ownership metrics generator runnable via
# `nix run .#update-ownership -- [args...]`.
{ pkgs, script }:
pkgs.writeShellApplication {
  name = "update-ownership";
  runtimeInputs = [
    pkgs.git
    pkgs.python3
  ];
  text = ''
    exec python3 ${script} "$@"
  '';
}
