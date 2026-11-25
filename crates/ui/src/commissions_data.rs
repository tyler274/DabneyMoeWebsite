//! Static media entries for commission showcase pages.
//! Add images under `public/commissions/` and update the entries below.

#[derive(Clone, Copy)]
pub struct LapidaryItem {
    pub src: &'static str,
    pub alt: &'static str,
    pub crystal: &'static str,
    pub design: &'static str,
    pub designer: &'static str,
    pub lighting: Option<&'static str>,
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
    "Custom-cut stones, cabochons, and lapidary work - from rough rock to finished pieces. \
     Registered member of the U.S. Faceters Guild.";

pub const LAPIDARY_ITEMS: &[LapidaryItem] = &[
    LapidaryItem {
        src: "/commissions/lapidary/GGAG_Whirlpool_Arya-Akhavan_blue-light.jpg",
        alt: "GGAG Whirlpool cut gemstone fluorescing yellow-green under blue light",
        crystal: "GGAG",
        design: "Whirlpool",
        designer: "Arya Akhavan",
        lighting: Some("blue light"),
    },
    LapidaryItem {
        src: "/commissions/lapidary/Ce-YAG-Turtles-Special_Treforze_Arya-Akhavan_white-light.jpg",
        alt: "Ce-YAG Turtles Special Treforze cut gemstone held in hand under white light",
        crystal: "Ce-YAG Turtles Special",
        design: "Treforze",
        designer: "Arya Akhavan",
        lighting: Some("white light"),
    },
    LapidaryItem {
        src: "/commissions/lapidary/Ce-YAG-Turtles-Special_Secret-Dungeon_Arya-Akhavan_blue-light.jpg",
        alt: "Ce-YAG Turtles Special Definitely Final Dungeon cut gemstone fluorescing under blue light",
        crystal: "Ce-YAG Turtles Special",
        design: "Definitely Final Dungeon",
        designer: "Arya Akhavan",
        lighting: Some("blue light"),
    },
];

pub const ARTWORK_INTRO: &str = "Original vector illustrations and visual studies created by \
Tyler Port for dabney.moe.";

pub const ARTWORK_ITEMS: &[GalleryItem] = &[
    GalleryItem {
        src: "/dabney-elephant.svg",
        alt: "Dabney.moe original emerald-green elephant vector emblem",
        title: "Dabney.moe Elephant Emblem",
        artist: "Tyler Port",
        artist_url: "https://dabney.moe",
    },
    GalleryItem {
        src: "/commissions/artwork/piece-01.svg",
        alt: "Original geometric study with cyan circle, rectangle, and flowing wave on deep slate gradient",
        title: "Geometric Study I — Cyan Wave",
        artist: "Tyler Port",
        artist_url: "https://dabney.moe",
    },
    GalleryItem {
        src: "/commissions/artwork/piece-02.svg",
        alt: "Original geometric study of layered cyan triangular peaks over a slate horizon",
        title: "Geometric Study II — Slate Peaks",
        artist: "Tyler Port",
        artist_url: "https://dabney.moe",
    },
    GalleryItem {
        src: "/commissions/artwork/piece-03.svg",
        alt: "Original stylized minimalist mask portrait on a radial teal-to-slate backdrop",
        title: "Geometric Study III — Minimalist Mask",
        artist: "Tyler Port",
        artist_url: "https://dabney.moe",
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
        description: Some("Lorien Testard, Sandfall Interactive - Clair Obscur: Expedition 33"),
    },
    Song {
        video_id: "D7Tc1s1LptQ",
        title: "Lumière",
        description: Some("Lorien Testard, Sandfall Interactive - Clair Obscur: Expedition 33"),
    },
    Song {
        video_id: "himNXBaTWDc",
        title: "Sis Puella Magica!",
        description: Some("Yuki Kajiura - Madoka Magica"),
    },
];
