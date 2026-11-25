# Google Play Store Listing Copy — Dabney.moe Clinical Demo

Ready-to-paste metadata for `moe.dabney.demo`.

## Core Identity

- **Application ID (package name)**: `moe.dabney.demo`
- **App name** *(24 / 30 chars)*: `Dabney.moe Clinical Demo`
- **Default language**: English (United States)
- **Category**: Medical *(or Health & Fitness — see [data-safety-and-health-declaration.md](data-safety-and-health-declaration.md))*
- **Tags**: Health, Demo, Open Source
- **Free / Paid**: Free (no ads, no in-app purchases, no account required)
- **Website**: `https://dabney.moe/clinical`
- **Privacy policy URL**: `https://dabney.moe/privacy`
- **Support email**: `tp@dabney.moe`

## Short Description *(72 / 80 chars)*

```text
Zero-PHI synthetic patient & clinician workflow demo built in Rust/WASM.
```

## Full Description *(~1,950 / 4,000 chars)*

```text
SYNTHETIC DEMO ONLY — ZERO PHI — NOT MEDICAL ADVICE
All names, lab values, glucose summaries, medications, and visit notes in this app are 100% fictional sample data. Do not enter real personal health information. Everything runs locally on your device with zero server uploads, zero accounts, and zero tracking.

Dabney.moe Clinical Demo lets patients and clinicians preview the Dabney.moe clinical software suite offline on Android—built from the same pure Rust and WebAssembly codebase powering the live in-browser demos at https://dabney.moe/clinical.

WHAT YOU CAN EXPLORE

1. Patient Experience Demo ("Your Health" & "For My Doctor")
• Switch between four fictional sample patient profiles (type 2 diabetes & CGM, blood pressure & renal monitoring, mood & glycemic stability, and atrial fibrillation).
• Inspect sample medications, lab results, CGM summaries, progress notes, and imaging records.
• Toggle exactly which synthetic records to include in your "For My Doctor" visit summary while keeping unselected records private.
• Keep device-only private notes that are structurally excluded from doctor packets.
• Test patient-controlled sharing states (Connected to Demo Clinic, Pause Sharing, or Standalone) and preview the exact visit packet a clinician would receive—simulated locally with zero network transmission.
• Ask deterministic, record-cited questions scoped strictly to the selected fictional profile (unmatched questions return zero citations instead of guessing).

2. Physician Workspace Demo
• Preview today's synthetic clinic queue, reviewable patient reply drafts (clinician approval required), and verbatim record-cited chart Q&A.

3. Mental Health & Commercial CGM Analytics
• Explore local-first mood, anxiety, sleep cadence, and commercial CGM/BGM analytics (ADA 5-band Time in Range, GMI, GRI, 24-hour AGP) running entirely on-device in WebAssembly.

PRIVACY & SAFETY BY DESIGN
• 100% synthetic sample data—zero Protected Health Information (PHI).
• Zero user data collected, stored on servers, or shared with third parties.
• Zero ads, zero analytics SDKs, zero Health Connect permissions, and zero login required.
• Educational and software demonstration purposes only; not a medical device, not medical advice, and not for diagnosis, treatment, or emergency use.

Try the identical in-browser demos anytime at https://dabney.moe/clinical.
```

## Store Graphic Assets Checklist

| Asset | Required Size & Format | Source |
| --- | --- | --- |
| App icon | 512 × 512 px, 32-bit PNG | `cargo run -p icon-gen -- --play-assets target/android-release/play-store-assets` → `icon-512.png` |
| Feature graphic | 1024 × 500 px, PNG or JPEG | `cargo run -p icon-gen -- --play-assets target/android-release/play-store-assets` → `feature-graphic-1024x500.png` |
| Phone screenshots | Min 2 screenshots (16:9 or 9:16, 320–3840 px) | Captured from verified Android emulator (`Patient`, `For My Doctor`, `Physician`, `Mood & CGM`) |
