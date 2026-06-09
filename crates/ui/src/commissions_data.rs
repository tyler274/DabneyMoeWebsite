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
    pub title: &'static str,
    pub artist: &'static str,
    pub artist_url: &'static str,
}

pub const LAPIDARY_INTRO: &str =
    "Custom-cut stones, cabochons, and lapidary work - from rough rock to finished pieces.";

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

pub const ARTWORK_INTRO: &str = "Artwork commissioned from independent artists. Copyright remains \
with the creators - these are pieces they made at my request.";

pub const ARTWORK_ITEMS: &[GalleryItem] = &[
    GalleryItem {
        src: "/commissions/artwork/Jack-O_Kiron_fightstick.jpg",
        alt: "Jack-O' Valentine fightstick artwork with in-game UI elements",
        title: "Jack-O' Valentine - fightstick art",
        artist: "Subakeye",
        artist_url: "https://x.com/Subakeye/status/1408890470827184128/photo/1",
    },
    GalleryItem {
        src: "/commissions/artwork/Angela.png",
        alt: "Angela character portrait in a clinical office setting",
        title: "Angela",
        artist: "Hydowa",
        artist_url: "https://vgen.co/Hydowa",
    },
];

pub const EILI_YOUTUBE: &str = "https://www.youtube.com/@EiliYT";

pub const SONGS_INTRO: &str =
    "Vocal covers performed by eili. Listen below. I commissioned these and did not work on them in any way.";

pub struct Song {
    pub video_id: &'static str,
    pub title: &'static str,
    pub description: Option<&'static str>,
}

pub const SONGS: &[Song] = &[
    Song {
        video_id: "HhpaUIl9ZJs",
        title: "Flares of the Blazing Sun (Female Version)",
        description: Some("Honkai: Star Rail"),
    },
    Song {
        video_id: "yPEGrkSUmzM",
        title: "Alicia",
        description: Some(
            "Lorien Testard, Sandfall Interactive - Clair Obscur: Expedition 33",
        ),
    },
    Song {
        video_id: "D7Tc1s1LptQ",
        title: "Lumière",
        description: Some(
            "Lorien Testard, Sandfall Interactive - Clair Obscur: Expedition 33",
        ),
    },
    Song {
        video_id: "himNXBaTWDc",
        title: "Sis Puella Magica!",
        description: Some("Yuki Kajiura - Madoka Magica"),
    },
];
