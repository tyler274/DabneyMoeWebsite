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

pub const PROFILE_PHOTO: &str = "/photos/tyler-dunes.jpg";
pub const ABOUT_PHOTO: &str = "/photos/tyler-travel.png";

pub const TAGLINE: &str = "Independent software engineer & freelance contractor";

pub const SUMMARY: &str = "Caltech-trained software engineer specializing in Rust, GPU \
computing, and high-performance systems. I take projects from first commit to production \
deployment. I'm available for freelance and contract work.";

pub const ABOUT: &str = "When I'm not shipping systems code, you'll find me behind a camera \
or exploring somewhere new. I bring the same curiosity I have for travel and culture to \
every engineering problem-whether that's parallelizing orbital mechanics at JPL, teaching \
GPU programming at Caltech, or building the next thing from scratch.";

pub const EXPERTISE: &[Expertise] = &[
    Expertise {
        title: "NVIDIA CUDA & GPU Programming",
        blurb: "From lecturing for CS179 at Caltech to CUDA C++ at JPL and Zeiss, I design and ship \
                parallel algorithms on GPUs-scientific simulation, ML inference, and \
                real-time image pipelines.",
        image: "/photos/cuda-logo.jpg",
        image_alt: "NVIDIA CUDA logo",
    },
    Expertise {
        title: "FPGAs & Hardware Acceleration",
        blurb: "At Carl Zeiss I deployed PyTorch models on AMD/Xilinx FPGAs for real-time \
                image signal processing. Comfortable from board bring-up through SYCL, \
                VHDL, and the full HPC stack.",
        image: "/photos/fpga-ultra96.png",
        image_alt: "Avnet Ultra96 FPGA development board",
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

pub const EXPERIENCE: &[Role] = &[
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
            "Built custom software for clients leveraging Rust, Python, C++, and GPU \
             acceleration.",
            "Designed and implemented core systems for the independent game Rummage.",
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
            "HPC environments",
            "FPGAs (Intel, AMD)",
            "Docker",
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
