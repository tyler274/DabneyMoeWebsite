//! Static content for the site, sourced from Tyler Port's resume.
//! Keeping it here means the web (SSR) and Tauri (CSR) builds render
//! identical copy from a single source of truth.

pub struct Service {
    pub title: &'static str,
    pub blurb: &'static str,
    pub icon: &'static str,
}

pub struct Role {
    pub company: &'static str,
    pub location: &'static str,
    pub title: &'static str,
    pub period: &'static str,
    pub points: &'static [&'static str],
}

pub struct SkillGroup {
    pub label: &'static str,
    pub items: &'static [&'static str],
}

pub struct Expertise {
    pub title: &'static str,
    pub blurb: &'static str,
    pub image: &'static str,
    pub image_alt: &'static str,
}

pub struct Project {
    pub name: &'static str,
    pub blurb: &'static str,
    pub tags: &'static [&'static str],
    pub href: &'static str,
    pub demo_href: Option<&'static str>,
}

pub const PROFILE_PHOTO: &str = "/photos/tyler-dunes.jpg";
pub const ABOUT_PHOTO: &str = "/photos/tyler-travel.png";

pub const TAGLINE: &str = "Independent software engineer & freelance contractor";

pub const SUMMARY: &str = "Caltech-trained software engineer specializing in Rust, GPU \
computing, and high-performance systems. Previously worked for Coda Health as their \
Founding Senior System Software Engineer; recent work spans Dabney.moe (full-stack \
Rust clinical systems), Elysian Studio, ElyMalloc, and ElyLD in safe Rust. I take \
projects from first commit to production deployment. I'm available for freelance and \
contract work.";

pub const ABOUT: &str = "When I'm not shipping systems code, you'll find me behind a camera \
or exploring somewhere new. I bring the same curiosity I have for travel and culture to \
every engineering problem-whether that's parallelizing orbital mechanics at JPL, teaching \
GPU programming at Caltech, or building the next thing from scratch.";

pub const EXPERTISE: &[Expertise] = &[
    Expertise {
        title: "NVIDIA CUDA & GPU Programming",
        blurb:
            "From lecturing for CS179 at Caltech to CUDA C++ at JPL and Zeiss, I design and ship \
                parallel algorithms on GPUs-scientific simulation, ML inference, and \
                real-time image pipelines.",
        image: "/photos/gpu-compute.jpg",
        image_alt: "Isometric illustration of a parallel GPU compute die with glowing cyan and emerald streaming cores",
    },
    Expertise {
        title: "FPGAs & Hardware Acceleration",
        blurb: "At Carl Zeiss I deployed PyTorch models on AMD/Xilinx FPGAs for real-time \
                image signal processing. Comfortable from board bring-up through SYCL, \
                VHDL, and the full HPC stack.",
        image: "/photos/fpga-silicon.jpg",
        image_alt: "Overhead illustration of an FPGA silicon die showing programmable logic arrays and routing fabric",
    },
];

pub const SERVICES: &[Service] = &[
    Service {
        title: "Rust & Systems Engineering",
        blurb: "Memory-safe, high-throughput systems software, services, and tooling built \
                in Rust, C, and C++, from low-level runtimes to production backends.",
        icon: "⚙",
    },
    Service {
        title: "GPU & HPC Acceleration",
        blurb: "CUDA, SYCL, OpenCL, and HIP work that turns slow numerical pipelines into \
                parallel ones. Experience accelerating scientific and ML workloads on GPUs \
                and FPGAs.",
        icon: "▲",
    },
    Service {
        title: "Full-Stack & Product",
        blurb: "End-to-end delivery in TypeScript, React, Node.js, and Rust/Leptos, \
                including the infrastructure, CI, and ops needed to ship and keep it running.",
        icon: "◆",
    },
    Service {
        title: "DevOps & Infrastructure",
        blurb: "Docker, Linux, CI/CD, and monitoring for teams that need reliable \
                infrastructure without a dedicated platform org.",
        icon: "❖",
    },
];

pub const PROJECTS: &[Project] = &[
    Project {
        name: "Dabney.moe Clinical Workspace",
        blurb:
            "Clean-room local-first Rust & WebAssembly clinical workspace hosted on dabney.moe — \
                interactive synthetic zero-PHI physician chart review with verbatim record-cited Q&A, \
                embedded Axum clinical API with optional same-origin streaming reverse proxy, \
                comprehensive commercial CGM & mental health analytics, Nix OCI images, and \
                provider-agnostic infrastructure.",
        tags: &["Rust", "Leptos", "Axum", "WASM", "Nix", "Terraform"],
        href: "https://dabney.moe/clinical",
        demo_href: Some("/clinical/physician"),
    },
    Project {
        name: "Elysian Studio",
        blurb: "Safe-Rust rewrite of Bambu Studio. Deterministic CPU slicing (walls, infill, \
                supports), wgpu Vulkan compute contours, an iced UI, and LAN MQTT/FTPS \
                printer control. Uses ElyMalloc in-process and links with ElyLD.",
        tags: &["Rust", "wgpu", "Vulkan", "iced", "3D printing"],
        href: "https://github.com/tyler274/elysian-studio",
        demo_href: None,
    },
    Project {
        name: "ElyMalloc",
        blurb: "Pure-Rust drop-in for Microsoft mimalloc v3.5.2. Ships the C ABI and SONAME \
                (libmimalloc.so.3, mi_*), always-on MI_SECURE mitigations, WASM GlobalAlloc, \
                Kani proofs, and a first-class NixOS malloc provider.",
        tags: &["Rust", "allocators", "mimalloc", "NixOS", "WASM"],
        href: "https://github.com/tyler274/ElyMalloc",
        demo_href: None,
    },
    Project {
        name: "ElyLD",
        blurb: "GNU-ld-compatible incremental ELF linker, forked from Wild. --incremental \
                patches an existing output in place. Links Linux kernel vmlinux and glibc \
                DSOs, and runs as a NixOS stdenv linker.",
        tags: &["Rust", "ELF", "linkers", "Linux kernel", "NixOS"],
        href: "https://github.com/tyler274/ElyLD",
        demo_href: None,
    },
];

pub const EXPERIENCE: &[Role] = &[
    Role {
        company: "Dabney.moe",
        location: "Remote",
        title: "Senior System Software Engineer",
        period: "Oct 2026 – Nov 2026",
        points: &[
            "Previously worked for Coda Health as their Founding Senior System Software Engineer; \
             engineered full-stack Rust clinical systems (WASM web interfaces, Axum services, \
             PostgreSQL with pgvector RLS, Valkey SSE fan-out, zero-PHI push, and Swift \
             HealthKit / Kotlin Health Connect bridges) powering clinician and patient workflows.",
            "Built a bounded LLM tool-execution harness, offline BGE semantic encoder, \
             Poppler/Tesseract OCR, PKCE FHIR R4 & EHR adapters, and synchronized care workflows \
             backed by TLA+ model checks, PostgreSQL crash-recovery suites, headless Android \
             emulator tests, and Chromium E2E qualification.",
            "Automated Nix distroless OCI images, OpenTofu/Terraform cloud infrastructure, \
             OpenTelemetry observability, sandboxed CI, and clean-room interactive WASM clinical \
             workspace hosting at dabney.moe/clinical.",
        ],
    },
    Role {
        company: "Tivara",
        location: "New York City",
        title: "Founding Software Engineer",
        period: "May 2025 – Aug 2025",
        points: &[
            "Owned end-to-end development and launched the product pilots of AI voice agents \
             for patient intake and scheduling across two U.S. clinical networks.",
            "Full-stack development plus operations and infrastructure management in \
             TypeScript, React, and Node.js.",
        ],
    },
    Role {
        company: "Freelance Engineer & Independent Game Developer",
        location: "Remote",
        title: "Independent Contractor",
        period: "Oct 2024 – Present",
        points: &[
            "Building the Elysian systems stack in safe Rust: Elysian Studio (AGPL rewrite \
             of Bambu Studio with wgpu Vulkan slicing and open LAN printer I/O), ElyMalloc \
             (pure-Rust mimalloc v3 drop-in with always-on MI_SECURE mitigations and a \
             NixOS malloc provider), and ElyLD (GNU-ld-compatible incremental ELF linker \
             for Linux vmlinux, glibc DSOs, and NixOS stdenv).",
            "Leading a from-scratch Rust rewrite of gssproxy, the GSSAPI/Kerberos proxy \
             daemon and its mechglue interposer: a memory-safe, drop-in replacement across \
             a multi-crate workspace, with FFI to MIT krb5/GSSAPI and Kani formal \
             verification of the security-critical XDR/gssx wire codec.",
            "Designed and built this site (dabney.moe) as a Leptos (SSR + WebAssembly) and \
             Tauri v2 multiplatform app sharing one Rust UI crate, with a Nix-driven, \
             multi-cloud (GCP/AWS/Azure) Terraform CI/CD pipeline producing hardened, \
             statically-linked container images.",
            "Built custom software for clients leveraging Rust, Python, C++, and GPU \
             acceleration, and designed the core systems for the independent game Rummage.",
        ],
    },
    Role {
        company: "Carl Zeiss AG",
        location: "Germany",
        title: "System Software Engineer",
        period: "Mar 2023 – Mar 2024",
        points: &[
            "Led systems engineering for high-performance computing on GPUs and FPGAs, \
             specializing in CUDA C++, SYCL, and Python to accelerate scientific and AI/ML \
             workloads.",
            "Managed master's students and interns on GPU/FPGA programming and system \
             optimization projects.",
            "Built and deployed FPGA implementations of PyTorch models for real-time image \
             signal processing pipelines.",
        ],
    },
    Role {
        company: "Caltech",
        location: "Pasadena, CA",
        title: "Lecturer & Teaching Assistant, Computer Science",
        period: "Apr 2018 – Jun 2022",
        points: &[
            "Taught and developed course material for CS179 (GPU Programming), covering \
             NVIDIA CUDA for parallel algorithms.",
            "Taught Rust for the CS11 workshop and ported CS24 (Operating Systems) \
             assignments from C to Rust.",
            "Taught relational database theory and SQL application development in CS121.",
        ],
    },
    Role {
        company: "NASA Jet Propulsion Laboratory",
        location: "Pasadena, CA",
        title: "Software Engineering Intern",
        period: "Jan 2018 – Jun 2018",
        points: &[
            "Accelerated Europa Lander orbital calculations by reimplementing MATLAB in \
             CUDA C++ for large-scale parallel simulation and visualization.",
        ],
    },
    Role {
        company: "Tinder Inc.",
        location: "Los Angeles, CA",
        title: "Software Engineering Intern",
        period: "Jun 2018 – Aug 2018",
        points: &[
            "Built a Prometheus/Grafana extension (Python, Puppet) to trace microservice \
             dependencies and pinpoint root causes of service degradation.",
        ],
    },
    Role {
        company: "Caltech IMSS / UGCS",
        location: "Pasadena, CA",
        title: "IMSS Representative & Systems Administrator",
        period: "Jan 2016 – Jun 2022",
        points: &[
            "Administered a 150 TB storage array and compute resources, keeping them highly \
             available for thousands of users in a Linux environment.",
        ],
    },
];

pub const SKILLS: &[SkillGroup] = &[
    SkillGroup {
        label: "Languages",
        items: &[
            "Rust",
            "Python",
            "C / C++",
            "CUDA / SYCL / OpenCL / HIP",
            "SQL",
            "Swift",
            "C#",
            "Java / Kotlin",
            "TypeScript / JavaScript",
            "Haskell",
            "OCaml",
            "VHDL",
        ],
    },
    SkillGroup {
        label: "Platforms & Tech",
        items: &[
            "Linux (Gentoo, Arch, RHEL, Debian)",
            "Dioxus",
            "PostgreSQL / pgvector",
            "Valkey / Redis",
            "HPC environments",
            "FPGAs (Intel, AMD)",
            "Docker",
            "Nix",
            "Terraform / OpenTofu",
            "CI/CD (GitHub Actions)",
            "Cloud (GCP, AWS, Azure)",
            "Git",
            "Windows",
            "macOS",
            "BSD",
        ],
    },
    SkillGroup {
        label: "Specialties",
        items: &[
            "GPU computing & parallelization",
            "Systems engineering & administration",
            "DevOps & infrastructure",
            "High-performance computing",
            "Cross-platform apps (Dioxus, Tauri, Leptos/WASM)",
            "LLM agent harnesses",
            "Formal verification (TLA+, Kani)",
            "OpenTelemetry / Datadog",
            "Memory allocators",
            "ELF linkers",
            "AI/ML (PyTorch)",
            "Low-level programming",
            "Relational database design",
            "Computer graphics",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taglines_are_present() {
        assert!(!TAGLINE.trim().is_empty(), "TAGLINE must not be empty");
        assert!(!SUMMARY.trim().is_empty(), "SUMMARY must not be empty");
    }

    #[test]
    fn services_are_well_formed() {
        assert!(!SERVICES.is_empty(), "expected at least one service");
        for s in SERVICES {
            assert!(
                !s.title.trim().is_empty(),
                "service title must not be empty"
            );
            assert!(
                !s.blurb.trim().is_empty(),
                "service blurb must not be empty: {}",
                s.title
            );
            assert!(
                !s.icon.trim().is_empty(),
                "service icon must not be empty: {}",
                s.title
            );
        }
    }

    #[test]
    fn projects_are_well_formed() {
        assert!(!PROJECTS.is_empty(), "expected at least one project");
        for p in PROJECTS {
            assert!(!p.name.trim().is_empty(), "project name must not be empty");
            assert!(
                !p.blurb.trim().is_empty(),
                "project blurb must not be empty: {}",
                p.name
            );
            assert!(
                p.href.starts_with("https://"),
                "project href must be https: {}",
                p.name
            );
            if let Some(demo) = p.demo_href {
                assert!(
                    demo.starts_with('/') || demo.starts_with("https://"),
                    "project demo_href must be root-relative or https: {}",
                    p.name
                );
            }
            assert!(!p.tags.is_empty(), "project must have tags: {}", p.name);
            for tag in p.tags {
                assert!(
                    !tag.trim().is_empty(),
                    "project tag must not be empty: {}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn experience_is_well_formed() {
        assert!(!EXPERIENCE.is_empty(), "expected at least one role");
        for r in EXPERIENCE {
            assert!(
                !r.company.trim().is_empty(),
                "role company must not be empty"
            );
            assert!(
                !r.location.trim().is_empty(),
                "role location must not be empty: {}",
                r.company
            );
            assert!(
                !r.title.trim().is_empty(),
                "role title must not be empty: {}",
                r.company
            );
            assert!(
                !r.period.trim().is_empty(),
                "role period must not be empty: {}",
                r.company
            );
            assert!(
                !r.points.is_empty(),
                "role must have at least one bullet point: {}",
                r.company
            );
            for p in r.points {
                assert!(
                    !p.trim().is_empty(),
                    "role bullet must not be empty: {}",
                    r.company
                );
            }
        }
    }

    #[test]
    fn expertise_is_well_formed() {
        assert!(
            !EXPERTISE.is_empty(),
            "expected at least one expertise area"
        );
        for e in EXPERTISE {
            assert!(
                !e.title.trim().is_empty(),
                "expertise title must not be empty"
            );
            assert!(
                !e.blurb.trim().is_empty(),
                "expertise blurb must not be empty: {}",
                e.title
            );
            assert!(
                !e.image.trim().is_empty(),
                "expertise image must not be empty: {}",
                e.title
            );
            assert!(
                !e.image_alt.trim().is_empty(),
                "expertise image alt must not be empty: {}",
                e.title
            );
        }
    }

    #[test]
    fn about_copy_is_present() {
        assert!(!ABOUT.trim().is_empty(), "ABOUT must not be empty");
    }

    #[test]
    fn skills_are_well_formed() {
        assert!(!SKILLS.is_empty(), "expected at least one skill group");
        for g in SKILLS {
            assert!(
                !g.label.trim().is_empty(),
                "skill group label must not be empty"
            );
            assert!(
                !g.items.is_empty(),
                "skill group must have items: {}",
                g.label
            );
            for item in g.items {
                assert!(
                    !item.trim().is_empty(),
                    "skill item must not be empty: {}",
                    g.label
                );
            }
        }
    }
}
