# Web server container image.
#
# The dabney.moe web frontend is a Leptos SSR app: an Axum binary (`web`) plus
# the hashed asset bundle under `target/site`. The deploy unit is therefore a
# container, which we build reproducibly with Nix and ship to any cloud's
# registry (see ../terraform).
{ pkgs
, crane
, rustToolchain
, rustToolchainMusl
, src
,
}:
let
  craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

  # Vendor the whole Cargo.lock closure into the store so `cargo leptos build`
  # runs without network access inside the sandbox.
  cargoVendorDir = craneLib.vendorCargoDeps { inherit src; };

  # Tools cargo-leptos shells out to during a release build.
  leptosBuildTools = with pkgs; [
    cargo-leptos
    tailwindcss
    dart-sass
    wasm-bindgen-cli
    binaryen
    pkg-config
  ];

  # Filter the source tree for Rust/WASM/Tailwind compilation so changes to
  # static assets under `public/` (other than `public/recovery/index.json`,
  # which `crates/ui` embeds via `include_str!`), docs, infra, or CI scripts
  # never invalidate the compiled server derivation.
  rustSrc = pkgs.lib.cleanSourceWith {
    inherit src;
    filter =
      path: type:
      let
        rel = pkgs.lib.removePrefix (toString src + "/") (toString path);
        isTopExcluded =
          builtins.elem rel [
            "docs"
            "terraform"
            "selfhost"
            "anubis"
            "rawPics"
            "scripts"
            ".github"
            ".devcontainer"
            ".githooks"
          ]
          || pkgs.lib.hasSuffix ".md" rel;
        isPublicExcluded =
          (rel == "public" && false)
          || (
            pkgs.lib.hasPrefix "public/" rel
            && rel != "public/recovery"
            && rel != "public/recovery/index.json"
          );
      in
      !isTopExcluded && !isPublicExcluded && pkgs.lib.cleanSourceFilter path type;
  };

  # Full static asset directory layered onto `$out/share/site` after the
  # Rust/WASM build finishes, so static-only commits (e.g. `/dux/`, `/coda/`,
  # `resume.pdf`, photos) reuse the cached server binary without recompiling.
  publicAssets = pkgs.lib.cleanSourceWith {
    src = src + "/public";
    filter = pkgs.lib.cleanSourceFilter;
  };

  # Musl C toolchain. `mimalloc` (a C library) is compiled by the `cc` crate
  # against the target triple, so the static-musl build needs a musl-targeting
  # compiler + archiver, not the host glibc gcc.
  muslCC = pkgs.pkgsCross.musl64.stdenv.cc;
  muslCCBin = "${muslCC}/bin/${muslCC.targetPrefix}cc";
  muslAR = "${muslCC.bintools}/bin/${muslCC.targetPrefix}ar";

  # Shared musl cross-compilation environment for both dependency pre-builds
  # and the final `cargo leptos build --release` derivation.
  muslEnv = {
    # cargo-leptos builds the bin (only) for this triple; the wasm lib
    # build is unaffected.
    LEPTOS_BIN_TARGET_TRIPLE = "x86_64-unknown-linux-musl";
    # `cc` (libmimalloc-sys) + rustc's link step both need the musl
    # toolchain. mold is selected via the cc driver's -fuse-ld.
    CC_x86_64_unknown_linux_musl = muslCCBin;
    AR_x86_64_unknown_linux_musl = muslAR;
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = muslCCBin;
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-fuse-ld=mold";
  };

  # Pre-compile third-party Rust dependencies for both the WASM hydrate target
  # (`target/front`) and the SSR server binary (`target/`) using crane's dummy
  # sources. Because this derivation only depends on `Cargo.toml`/`Cargo.lock`,
  # Magic Nix Cache reuses it across every code change until dependencies bump.
  mkWebServerDeps =
    { staticMusl ? false
    ,
    }:
    let
      toolchain = if staticMusl then rustToolchainMusl else rustToolchain;
      binTargetArgs = pkgs.lib.optionalString staticMusl "--target x86_64-unknown-linux-musl";
    in
    pkgs.stdenv.mkDerivation (
      {
        pname = if staticMusl then "dabney-web-static-deps" else "dabney-web-deps";
        version = "0.1.0";
        src = craneLib.mkDummySrc { src = rustSrc; };

        nativeBuildInputs = [
          toolchain
          pkgs.pkg-config
          pkgs.zstd
        ]
        ++ pkgs.lib.optionals staticMusl [
          pkgs.mold
          muslCC
        ];

        configurePhase = ''
          runHook preConfigure
          export HOME="$TMPDIR"
          export CARGO_HOME="$TMPDIR/.cargo"
          mkdir -p "$CARGO_HOME"
          cp ${cargoVendorDir}/config.toml "$CARGO_HOME/config.toml"
          export CARGO_NET_OFFLINE=true
          runHook postConfigure
        '';

        buildPhase = ''
          runHook preBuild
          # Match the exact cargo invocations `cargo leptos build --release`
          # executes for the WASM hydrate cdylib and the SSR server binary.
          cargo build \
            --package=web --lib \
            --target-dir=target/front \
            --target=wasm32-unknown-unknown \
            --no-default-features --features=hydrate \
            --profile=wasm-release

          cargo build \
            --package=web --bin=web \
            ${binTargetArgs} \
            --no-default-features --features=ssr \
            --release
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          # Strip dummy workspace crate artifacts so the real build always
          # compiles them cleanly and the cached archive stays compact.
          for dir in target target/front; do
            find "$dir" -maxdepth 4 -type d -name deps | while IFS= read -r deps; do
              profile="$(dirname "$deps")"
              find "$profile" -maxdepth 1 -type f -delete
              rm -rf "$profile/incremental"
              for pkg in web ui clinical_workspace clinical-workspace recovery_index recovery-index; do
                rm -rf "$deps/$pkg"* "$deps/lib$pkg"* "$profile/.fingerprint/$pkg-"*
              done
            done
          done

          mkdir -p "$out"
          tar -cf - target | zstd -T0 -3 -o "$out/target.tar.zst"
          runHook postInstall
        '';

        doCheck = false;
      }
      // pkgs.lib.optionalAttrs staticMusl muslEnv
    );

  # Builder for the SSR release derivation. With `staticMusl = true` the server
  # binary is compiled for `x86_64-unknown-linux-musl`, statically linked
  # (`+crt-static`) with the `mold` linker, dropping the glibc/gcc-lib runtime
  # refs. The wasm hydrate bundle always stays `wasm32-unknown-unknown`
  # regardless (cargo-leptos's bin-target-triple only affects the server bin).
  mkWebServer =
    { staticMusl ? false
    ,
    }:
    let
      toolchain = if staticMusl then rustToolchainMusl else rustToolchain;
      deps = mkWebServerDeps { inherit staticMusl; };
      binPath =
        if staticMusl then "target/x86_64-unknown-linux-musl/release/web" else "target/release/web";
      compiled = pkgs.stdenv.mkDerivation (
        {
          pname = if staticMusl then "dabney-web-static-bin" else "dabney-web-bin";
          version = "0.1.0";
          src = rustSrc;

          nativeBuildInputs = [
            toolchain
            pkgs.removeReferencesTo
            pkgs.zstd
          ]
          ++ leptosBuildTools
          ++ pkgs.lib.optionals staticMusl [
            pkgs.mold
            muslCC
          ];

          configurePhase = ''
            runHook preConfigure
            export HOME="$TMPDIR"
            export CARGO_HOME="$TMPDIR/.cargo"
            mkdir -p "$CARGO_HOME"
            # crane's vendor dir ships a config.toml that redirects crates.io to
            # the vendored sources in the store; reuse it verbatim.
            cp ${cargoVendorDir}/config.toml "$CARGO_HOME/config.toml"
            export CARGO_NET_OFFLINE=true
            # Restore pre-compiled third-party dependencies.
            zstd -dc ${deps}/target.tar.zst | tar -xf -
            # Use the Nix-provided tailwind/sass binaries rather than letting
            # cargo-leptos download its own (the sandbox has no network).
            export LEPTOS_TAILWIND_VERSION="$(tailwindcss --help 2>/dev/null | head -n1 | awk '{print $NF}')"
            runHook postConfigure
          '';

          buildPhase = ''
            runHook preBuild
            # Offline is enforced via CARGO_NET_OFFLINE; cargo-leptos doesn't
            # accept cargo's --frozen passthrough.
            cargo leptos build --release
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            install -Dm755 ${binPath} "$out/bin/web"
            # Leptos resolves its asset manifest relative to current_exe().
            install -Dm644 ${builtins.dirOf binPath}/hash.txt "$out/bin/hash.txt"
            mkdir -p "$out/share"
            cp -r target/site "$out/share/site"
            runHook postInstall
          '';

          # The release binary embeds source-path strings (panic locations)
          # pointing at the vendored crates and the toolchain's std sources.
          # They're never read at runtime, so scrub them to keep the runtime
          # closure (and thus the image) from dragging in the whole toolchain.
          postFixup = ''
            find "$out" -type f \
              -exec remove-references-to -t ${cargoVendorDir} -t ${toolchain} {} +
          '';
          disallowedReferences = [
            cargoVendorDir
            toolchain
            deps
          ];

          doCheck = false;
        }
        // pkgs.lib.optionalAttrs staticMusl muslEnv
      );
    in
    pkgs.runCommand (if staticMusl then "dabney-web-static" else "dabney-web") { } ''
      mkdir -p "$out/bin" "$out/share/site"
      ln -s ${compiled}/bin/web "$out/bin/web"
      ln -s ${compiled}/bin/hash.txt "$out/bin/hash.txt"
      cp -r ${compiled}/share/site/. "$out/share/site/"
      chmod -R u+w "$out/share/site"
      cp -r ${publicAssets}/. "$out/share/site/"
    '';

  # The release SSR build: `$out/bin/web` (server binary) plus
  # `$out/share/site` (hashed assets + the wasm `pkg/` dir).
  webServerStatic = mkWebServer { staticMusl = true; }; # hardened default
  webServer = mkWebServer { }; # glibc fallback

  # OCI image factory: copies the server + assets in, sets the LEPTOS_* runtime
  # env, and listens on 0.0.0.0:8080 (the port every cloud module and self-host
  # target wires its ingress to). Build with `nix build .#serverImage` -> result
  # is an ultra-lean ~25 MB static-musl OCI tarball deployable anywhere via
  # `./scripts/deploy.sh` or `skopeo copy docker-archive:result docker://<reg>`.
  mkServerImage =
    server:
    pkgs.dockerTools.buildLayeredImage {
      name = "dabney-web";
      # Embed the Nix store hash of the server derivation in the tag so every
      # distinct build gets a unique, content-addressed name.  This avoids the
      # "renaming old image" conflict when loading a new build alongside an
      # existing one.  The store path is /nix/store/<32-char-hash>-<name>;
      # builtins.substring extracts those 32 chars, and unsafeDiscardStringContext
      # strips the Nix string context so it is usable as a plain attribute value.
      tag = builtins.unsafeDiscardStringContext (builtins.substring 11 32 (builtins.toString server));
      # fakeNss provides /etc/passwd + /etc/group with a "nobody" entry at
      # UID/GID 65534, which the runtime needs to resolve the User field below.
      contents = [
        server
        pkgs.cacert
        pkgs.dockerTools.fakeNss
      ];
      config = {
        Cmd = [ "${server}/bin/web" ];
        # Run as nobody:nobody; the process never has root privileges even when
        # the container runtime doesn't enforce non-root on its own.
        User = "65534:65534";
        Env = [
          "LEPTOS_OUTPUT_NAME=dabney"
          "LEPTOS_HASH_FILES=true"
          "LEPTOS_SITE_ROOT=${server}/share/site"
          "LEPTOS_SITE_PKG_DIR=pkg"
          "LEPTOS_SITE_ADDR=0.0.0.0:8080"
          "LEPTOS_ENV=PROD"
          "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        ];
        ExposedPorts = {
          "8080/tcp" = { };
        };
      };
    };
in
{
  inherit webServer webServerStatic;

  # Default deploy image: the hardened static-musl build (mold + LTO +
  # mimalloc -DMI_SECURE). The glibc image stays available as a fallback.
  serverImage = mkServerImage webServerStatic;
  serverImageGlibc = mkServerImage webServer;
}
