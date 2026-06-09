//! Static media entries for commission showcase pages.
//! Add images under `public/commissions/` and update the entries below.

#[derive(Clone, Copy)]
pub enum CommissionMedia {
    Image {
        src: &'static str,
        alt: &'static str,
        caption: Option<&'static str>,
    },
    Youtube {
        video_id: &'static str,
        title: &'static str,
    },
}

#[derive(Clone, Copy)]
pub struct GalleryItem {
    pub src: &'static str,
    pub alt: &'static str,
    pub caption: Option<&'static str>,
}

pub const LAPIDARY_INTRO: &str =
    "Custom-cut stones, cabochons, and lapidary work — from rough rock to finished pieces.";

pub const LAPIDARY_ITEMS: &[CommissionMedia] = &[
    CommissionMedia::Image {
        src: "/commissions/lapidary/cabochon-01.svg",
        alt: "Polished cabochon with banded agate",
        caption: Some("Banded agate cabochon"),
    },
    CommissionMedia::Youtube {
        video_id: "dQw4w9WgXcQ",
        title: "Lapidary process demo",
    },
    CommissionMedia::Image {
        src: "/commissions/lapidary/slab-01.svg",
        alt: "Polished stone slab on display stand",
        caption: Some("Display slab"),
    },
    CommissionMedia::Youtube {
        video_id: "9bZkp7q19f0",
        title: "Cutting and polishing walkthrough",
    },
];

pub const ARTWORK_INTRO: &str = "Original artwork across media — commissions and personal pieces.";

pub const ARTWORK_ITEMS: &[GalleryItem] = &[
    GalleryItem {
        src: "/commissions/artwork/piece-01.svg",
        alt: "Abstract composition in cool tones",
        caption: Some("Study in cyan"),
    },
    GalleryItem {
        src: "/commissions/artwork/piece-02.svg",
        alt: "Geometric landscape study",
        caption: Some("Horizon lines"),
    },
    GalleryItem {
        src: "/commissions/artwork/piece-03.svg",
        alt: "Portrait sketch with bold contrast",
        caption: Some("Portrait study"),
    },
];

pub const SONGS_INTRO: &str =
    "Original songs and recordings — listen below or reach out for custom work.";

pub struct Song {
    pub video_id: &'static str,
    pub title: &'static str,
    pub description: Option<&'static str>,
}

pub const SONGS: &[Song] = &[
    Song {
        video_id: "dQw4w9WgXcQ",
        title: "Song title (replace me)",
        description: Some("Short description of the track."),
    },
    Song {
        video_id: "9bZkp7q19f0",
        title: "Another track (replace me)",
        description: None,
    },
];
