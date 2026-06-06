# Roadmap / TODO

Planned features and improvements for the dabney.moe site, the Tauri app suite,
and the résumé. Items are also tracked as GitHub issues.

## Site content & features
- [ ] Projects / Portfolio section (Rummage + select freelance work, with tech tags and links)
- [ ] Testimonials / client logos for social proof
- [ ] Fuller About section with bio and headshot
- [ ] Crawlable `/resume` HTML route (router fallback already in place) in addition to the PDF
- [ ] Contact form backed by a Leptos server function (the site currently has none) wired to email/webhook
- [ ] Blog / writing section (`/blog`) on Rust / GPU / HPC topics
- [ ] Light/dark theme toggle (currently dark-only) honoring `prefers-color-scheme`
- [ ] Scroll-spy navbar that highlights the active section
- [ ] Mobile nav menu (links are `hidden md:flex` with no hamburger fallback)

## SEO / polish / infra
- [ ] Open Graph image at `public/og.png` (extend the `icon-gen` tool)
- [ ] `robots.txt` and `sitemap.xml` in `public/`
- [ ] JSON-LD structured data (`schema.org/Person`)
- [ ] Favicon set derived from `src-tauri/icons/source.png`
- [ ] Privacy-friendly analytics (e.g. Plausible)
- [x] CI: GitHub Actions running `cargo leptos build`, `cargo tauri build --no-bundle`, `clippy`, `fmt`, tests + Android APK build (`nix run .#ci` mirrors it locally)
- [x] Deployment: Nix `dockerTools` image (`nix build .#serverImage`) + modular Terraform/OpenTofu under `terraform/` (Cloud Run / App Runner / Container Apps); CI deploys the web image to GCP Cloud Run on green `main` once `GCP_SA_KEY`/`GCP_PROJECT` secrets + the remote state backend are configured
- [x] Harden the `serverImage` build: static `x86_64-unknown-linux-musl` target with the `mold` linker (+ fat LTO) and hardened Microsoft `mimalloc` (`-DMI_SECURE`) as the global allocator. `nix build .#serverImage` is now the hardened, glibc-free build (zero runtime store refs, ~18.7 MB image vs ~82 MB; `serverImageGlibc` kept as fallback); SSR + hydration verified in the container
- [x] HTTPS via Let's Encrypt for self-hosted/non-serverless deploys (the managed targets — Cloud Run / App Runner / Container Apps — already terminate TLS with their own certs). `selfhost/` runs a Caddy reverse proxy that auto-issues/renews certs for `dabney.moe`, redirects HTTP→HTTPS, keeps port 80 open for the `http-01` challenge, and follows the ACME `Link` headers (no hardcoded intermediate/ToS)
- [ ] Accessibility pass (focus states, contrast, aria labels, reduced motion)

## Tauri / app suite
- [x] Android SDK/NDK available declaratively via `nix develop .#android`; APK builds in CI + nightly emulator smoke test (run `cargo tauri ios init` once Xcode is available)
- [ ] Native open-in-browser / share actions via the Tauri opener plugin
- [ ] Explicit offline résumé view in the app
- [ ] App auto-update + full bundle targets (deb/rpm/AppImage need extra packaging tools)

## Résumé content (`Resume/resume.tex`)
- [ ] Add a one-line summary/objective framing freelance contracting
- [ ] Quantify impact with metrics rather than only responsibilities
- [ ] Add `dabney.moe` to the contact line
- [ ] Add a Projects section mirroring the site
- [ ] Trim/condense older roles to keep it to one page

## Résumé technical / build
- [ ] CI to compile the PDF from LaTeX (Nix/`latexmk`) so `resume.pdf` never drifts
- [ ] Single source of truth shared between `crates/ui/src/data.rs` and `resume.tex`
- [ ] Fix remaining LaTeX nits (stray trailing period, manual `\vspace` hacks)
 