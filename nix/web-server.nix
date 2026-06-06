# Web server container image.
#
# The dabney.moe web frontend is a Leptos SSR app: an Axum binary (`web`) plus
# the hashed asset bundle under `target/site`. The deploy unit is therefore a
# container, which we build reproducibly with Nix and ship to any cloud's
# registry (see ../terraform).
{ pkgs, crane, rustToolchain, rustToolchainMusl, src }:
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

  # Musl C toolchain. `mimalloc` (a C library) is compiled by the `cc` crate
  # against the target triple, so the static-musl build needs a musl-targeting
  # compiler + archiver, not the host glibc gcc.
  muslCC = pkgs.pkgsCross.musl64.stdenv.cc;
  muslCCBin = "${muslCC}/bin/${muslCC.targetPrefix}cc";
  muslAR = "${muslCC.bintools}/bin/${muslCC.targetPrefix}ar";

  # Builder for the SSR release derivation. With `staticMusl = true` the server
  # binary is compiled for `x86_64-unknown-linux-musl`, statically linked
  # (`+crt-static`) with the `mold` linker, dropping the glibc/gcc-lib runtime
  # refs. The wasm hydrate bundle always stays `wasm32-unknown-unknown`
  # regardless (cargo-leptos's bin-target-triple only affects the server bin).
  mkWebServer = { staticMusl ? false }:
    let
      toolchain = if staticMusl then rustToolchainMusl else rustToolchain;
      binPath =
        if staticMusl
        then "target/x86_64-unknown-linux-musl/release/web"
        else "target/release/web";
    in
    pkgs.stdenv.mkDerivation ({
      pname = if staticMusl then "dabney-web-static" else "dabney-web";
      version = "0.1.0";
      inherit src;

      nativeBuildInputs = [ toolchain pkgs.removeReferencesTo ]
        ++ leptosBuildTools
        ++ pkgs.lib.optionals staticMusl [ pkgs.mold muslCC ];

      configurePhase = ''
        runHook preConfigure
        export HOME="$TMPDIR"
        export CARGO_HOME="$TMPDIR/.cargo"
        mkdir -p "$CARGO_HOME"
        # crane's vendor dir ships a config.toml that redirects crates.io to
        # the vendored sources in the store; reuse it verbatim.
        cp ${cargoVendorDir}/config.toml "$CARGO_HOME/config.toml"
        export CARGO_NET_OFFLINE=true
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
      disallowedReferences = [ cargoVendorDir toolchain ];

      doCheck = false;
    } // pkgs.lib.optionalAttrs staticMusl {
      # cargo-leptos builds the bin (only) for this triple; the wasm lib
      # build is unaffected.
      LEPTOS_BIN_TARGET_TRIPLE = "x86_64-unknown-linux-musl";
      # `cc` (libmimalloc-sys) + rustc's link step both need the musl
      # toolchain. mold is selected via the cc driver's -fuse-ld.
      CC_x86_64_unknown_linux_musl = muslCCBin;
      AR_x86_64_unknown_linux_musl = muslAR;
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = muslCCBin;
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS =
        "-C target-feature=+crt-static -C link-arg=-fuse-ld=mold";
    });

  # The release SSR build: `$out/bin/web` (server binary) plus
  # `$out/share/site` (hashed assets + the wasm `pkg/` dir).
  webServerStatic = mkWebServer { staticMusl = true; }; # hardened default
  webServer = mkWebServer { }; # glibc fallback

  # OCI image factory: copies the server + assets in, sets the LEPTOS_* runtime
  # env, and listens on 0.0.0.0:8080 (the port every cloud module wires its
  # ingress to). Build with `nix build .#serverImage` -> result is a tarball
  # you `skopeo copy docker-archive:result docker://<reg>`.
  mkServerImage = server: pkgs.dockerTools.buildLayeredImage {
    name = "dabney-web";
    tag = "latest";
    contents = [ server pkgs.cacert ];
    config = {
      Cmd = [ "${server}/bin/web" ];
      Env = [
        "LEPTOS_OUTPUT_NAME=dabney"
        "LEPTOS_SITE_ROOT=${server}/share/site"
        "LEPTOS_SITE_PKG_DIR=pkg"
        "LEPTOS_SITE_ADDR=0.0.0.0:8080"
        "LEPTOS_ENV=PROD"
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      ];
      ExposedPorts = { "8080/tcp" = { }; };
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
