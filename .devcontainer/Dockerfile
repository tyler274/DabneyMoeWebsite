# Dev container for contributors on any OS (Windows/macOS/Linux) who have
# Docker + VS Code/Cursor but not Nix. The flake provides the entire pinned
# toolchain; this image just bootstraps Nix + direnv so the editor and its
# terminal automatically land in the project's `nix develop` shell.
FROM nixos/nix:latest

# Enable the modern Nix CLI + flakes for every invocation, and trust the
# repo's flake config (e.g. binary caches).
RUN mkdir -p /etc/nix \
    && printf '%s\n' \
        "experimental-features = nix-command flakes" \
        "accept-flake-config = true" \
        "max-jobs = auto" \
        >> /etc/nix/nix.conf

# git is needed for Nix to evaluate the local (git tree) flake; direnv +
# nix-direnv auto-load the dev shell when a terminal opens in the workspace.
RUN nix profile install \
        nixpkgs#git \
        nixpkgs#direnv \
        nixpkgs#nix-direnv \
    && mkdir -p /root/.config/direnv \
    && echo 'source $HOME/.nix-profile/share/nix-direnv/direnvrc' \
        > /root/.config/direnv/direnvrc \
    && echo 'eval "$(direnv hook bash)"' >> /root/.bashrc

WORKDIR /workspaces
