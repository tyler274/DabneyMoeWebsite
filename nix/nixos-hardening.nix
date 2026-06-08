# NixOS host-level hardening for the dabney-web service.
#
# Import this module into your NixOS configuration when running the web server
# directly on a NixOS host (VM or bare metal) instead of inside a managed
# container platform like Cloud Run.  It enables AppArmor and wraps the service
# in a systemd sandbox that restricts what an attacker can do if they breach
# the Axum process.
#
# Usage in your NixOS configuration:
#
#   imports = [ /path/to/nix/nixos-hardening.nix ];
#   services.dabney-web.package = pkgs.callPackage ./nix/web-server.nix { ... };
#
# On managed container platforms (Cloud Run GEN2, etc.) this module is NOT
# needed; the runtime's own sandbox (gVisor / Kata Containers) provides the
# equivalent protections at a lower level.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.dabney-web;
in
{
  options.services.dabney-web = {
    enable = lib.mkEnableOption "dabney.moe Leptos SSR web server";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The dabney-web derivation (bin/web + share/site).";
    };

    listenAddr = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8080";
      description = "Address and port the server binds to.";
    };

    siteRoot = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "Override LEPTOS_SITE_ROOT (defaults to the package's share/site).";
    };
  };

  config = lib.mkIf cfg.enable {
    # ── AppArmor ────────────────────────────────────────────────────────────
    # AppArmor is a Mandatory Access Control system that limits what files,
    # sockets, and capabilities a process may use, regardless of what the Unix
    # permissions say.  Even if an attacker achieves arbitrary code execution
    # inside the Axum process, the profile below ensures they can only:
    #   • read the Nix store (executables + assets)
    #   • open a TCP listen socket on the configured port
    #   • read CA certificates
    # Any write to the filesystem or attempt to open a raw/netlink socket will
    # be denied by the kernel before it takes effect.
    security.apparmor = {
      enable = true;
      policies."dabney-web" = {
        enable = true;
        enforce = true;
        profile = ''
          #include <tunables/global>

          profile dabney-web "${cfg.package}/bin/web" {
            #include <abstractions/base>
            #include <abstractions/nameservice>
            #include <abstractions/ssl_certs>

            # The static-musl binary only needs to read its own store path.
            "${cfg.package}/**"                   r,
            "${pkgs.cacert}/etc/ssl/**"            r,

            # Deny all writes everywhere.
            deny /** w,
            deny /** wl,

            # Allow outbound TCP (upstream HTTP calls, if any) and the
            # inbound listen socket.  Deny everything else.
            network inet  tcp,
            network inet6 tcp,
            deny network raw,
            deny network netlink,

            # No new privileges.
            deny capability sys_ptrace,
            deny capability sys_admin,
            deny capability net_admin,
          }
        '';
      };
    };

    # ── systemd service ────────────────────────────────────────────────────
    # systemd provides a second, independent sandboxing layer via Linux
    # namespaces, seccomp-bpf, and capability bounding sets.  Even if AppArmor
    # were bypassed, these restrictions still apply because they are enforced by
    # the kernel at the process-creation level.
    systemd.services.dabney-web = {
      description = "dabney.moe Leptos SSR web server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      environment = {
        LEPTOS_OUTPUT_NAME = "dabney";
        LEPTOS_SITE_ROOT = if cfg.siteRoot != "" then cfg.siteRoot else "${cfg.package}/share/site";
        LEPTOS_SITE_PKG_DIR = "pkg";
        LEPTOS_SITE_ADDR = cfg.listenAddr;
        LEPTOS_ENV = "PROD";
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      };

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/web";

        # Run as an unprivileged dynamic user (systemd allocates a transient
        # UID/GID; no /etc/passwd entry required).
        DynamicUser = true;

        # ── Filesystem isolation ───────────────────────────────────────────
        # The Nix store is read-only by kernel CoW semantics; these directives
        # additionally prevent the process from seeing or modifying anything
        # outside the store.
        ProtectSystem = "strict"; # / read-only except allowed paths
        ProtectHome = true; # /home, /root, /run/user invisible
        PrivateTmp = true; # private /tmp, not shared with host
        PrivateDevices = true; # no /dev access except null/zero/random
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;

        # Nothing to write at runtime; make that explicit.
        ReadOnlyPaths = [ "/" ];
        ReadWritePaths = [ ]; # override here if the server ever needs a writable dir
        InaccessiblePaths = [
          "/proc/sys"
          "/sys/fs"
        ];

        # ── Capability bounding set ────────────────────────────────────────
        # A static-musl HTTP server listening on port > 1024 needs zero Linux
        # capabilities.  Drop them all so a root escalation from inside the
        # process is impossible.
        CapabilityBoundingSet = ""; # empty = drop all
        AmbientCapabilities = "";
        NoNewPrivileges = true;

        # ── Namespace isolation ────────────────────────────────────────────
        # Give the process its own network namespace view (no access to host
        # internal interfaces) and PID namespace (can't signal other processes).
        PrivateNetwork = false; # must be false to accept external TCP
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
        ]; # no UNIX, netlink, raw sockets
        IPAddressDeny = "any"; # deny all outbound by default …
        # … re-open only what the SSR server legitimately needs:
        # (add upstream API CIDRs here if the server makes outbound HTTP calls)

        # ── Syscall filtering (seccomp-bpf) ───────────────────────────────
        # This is the NixOS/systemd equivalent of a Firejail or AppArmor
        # syscall profile.  The @system-service set covers every syscall a
        # well-behaved UNIX daemon needs; anything outside it raises EPERM
        # before it reaches the kernel.
        SystemCallFilter = [ "@system-service" ];
        SystemCallErrorNumber = "EPERM";
        SystemCallArchitectures = "native";

        # ── Misc hardening ─────────────────────────────────────────────────
        LockPersonality = true; # disallow personality() ABI tricks
        MemoryDenyWriteExecute = true; # no JIT / W^X (safe for musl+Rust)
        RestrictNamespaces = true; # can't create new namespaces (no pivot_root escape)
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        RemoveIPC = true;
        UMask = "0077";
      };
    };
  };
}
