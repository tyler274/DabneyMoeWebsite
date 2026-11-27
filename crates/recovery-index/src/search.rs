//! Pure, deterministic filtering, distance sorting, facet discovery, and pagination
//! over a [`StateCatalog`].

use crate::schema::{
    Amenity, CareType, Coordinates, DietaryOption, ElectronicsPolicy, Facility, FoodSituation,
    Payment, Setting, SmokingPolicy, StaffLicense, StateCatalog, VapingPolicy,
};

/// Number of facility cards shown per page.
pub const PAGE_SIZE: usize = 12;

/// Minimum and maximum search radii in kilometres.
pub const MIN_RADIUS_KM: u16 = 1;
pub const MAX_RADIUS_KM: u16 = 500;
pub const DEFAULT_RADIUS_KM: u16 = 80; // ~50 miles

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecoverySort {
    #[default]
    Name,
    Distance,
}

/// How multi-select license filters combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StaffMatchMode {
    /// Facility has at least one of the chosen licenses (e.g. MD Psychiatrist OR DO Psychiatrist).
    #[default]
    Any,
    /// Facility has every chosen license (e.g. MD Psychiatrist AND LCSW).
    All,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecoveryQuery {
    pub text: String,
    pub care: Vec<CareType>,
    pub settings: Vec<Setting>,
    pub payments: Vec<Payment>,
    /// Non-care amenities required (AND semantics: facility must have all selected amenities).
    pub amenities: Vec<Amenity>,
    /// Accepted food situations (OR semantics: facility matches any selected).
    pub food: Vec<FoodSituation>,
    /// Required dietary accommodations (AND semantics: facility must offer all selected).
    pub dietary: Vec<DietaryOption>,
    /// Accepted electronic device policies (OR semantics: facility matches any selected).
    pub electronics: Vec<ElectronicsPolicy>,
    /// Accepted smoking policies (OR semantics).
    pub smoking: Vec<SmokingPolicy>,
    /// Accepted vaping policies (OR semantics).
    pub vaping: Vec<VapingPolicy>,
    /// Desired clinical staff license types (strictly distinct: MD != DO, Psychiatrist != NP).
    pub staff_licenses: Vec<StaffLicense>,
    /// Whether `staff_licenses` requires any match or all matches.
    pub staff_match_mode: StaffMatchMode,
    /// When true, restrict results to facilities with enriched amenities or staff license data.
    pub only_enriched: bool,
    pub origin: Option<Coordinates>,
    pub radius_km: u16,
    pub sort: RecoverySort,
    pub page: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryHit {
    pub facility: Facility,
    pub distance_km: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryResults {
    pub hits: Vec<RecoveryHit>,
    pub total: usize,
    pub page: usize,
    pub page_count: usize,
}

/// Distinct filter facets actually present in a loaded [`StateCatalog`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfferedFacets {
    pub settings: Vec<Setting>,
    pub payments: Vec<Payment>,
    pub amenities: Vec<Amenity>,
    pub food: Vec<FoodSituation>,
    pub dietary: Vec<DietaryOption>,
    pub electronics: Vec<ElectronicsPolicy>,
    pub smoking: Vec<SmokingPolicy>,
    pub vaping: Vec<VapingPolicy>,
    pub staff_licenses: Vec<StaffLicense>,
    pub enriched_count: usize,
}

impl OfferedFacets {
    pub fn from_catalog(catalog: &StateCatalog) -> Self {
        let mut out = Self::default();
        for f in &catalog.facilities {
            if f.is_enriched() {
                out.enriched_count += 1;
            }
            for &s in &f.settings {
                insert_sorted_unique(&mut out.settings, s);
            }
            for &p in &f.payments {
                insert_sorted_unique(&mut out.payments, p);
            }
            for &a in &f.lifestyle.amenities {
                insert_sorted_unique(&mut out.amenities, a);
            }
            if let Some(food) = f.lifestyle.food {
                insert_sorted_unique(&mut out.food, food);
            }
            for &d in &f.lifestyle.dietary {
                insert_sorted_unique(&mut out.dietary, d);
            }
            if let Some(elec) = f.lifestyle.electronics {
                insert_sorted_unique(&mut out.electronics, elec);
            }
            if let Some(smoking) = f.lifestyle.smoking {
                insert_sorted_unique(&mut out.smoking, smoking);
            }
            if let Some(vaping) = f.lifestyle.vaping {
                insert_sorted_unique(&mut out.vaping, vaping);
            }
            for &lic in &f.staff_licenses {
                insert_sorted_unique(&mut out.staff_licenses, lic);
            }
        }
        out
    }
}

fn insert_sorted_unique<T: Ord>(list: &mut Vec<T>, val: T) {
    if let Err(pos) = list.binary_search(&val) {
        list.insert(pos, val);
    }
}

/// Parses `"lat, lng"` or `"lat lng"` typed by a user.
pub fn parse_coordinates(input: &str) -> Result<Coordinates, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter latitude and longitude, e.g. 39.7392, -104.9903".to_string());
    }
    let parts: Vec<&str> = trimmed
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != 2 {
        return Err("Use two numbers separated by a comma, e.g. 39.7392, -104.9903".to_string());
    }
    let lat: f64 = parts[0]
        .parse()
        .map_err(|_| "Latitude must be a number between -90 and 90".to_string())?;
    let lng: f64 = parts[1]
        .parse()
        .map_err(|_| "Longitude must be a number between -180 and 180".to_string())?;
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err("Latitude must be between -90 and 90".to_string());
    }
    if !lng.is_finite() || !(-180.0..=180.0).contains(&lng) {
        return Err("Longitude must be between -180 and 180".to_string());
    }
    Ok(Coordinates { lat, lng })
}

/// Formats distance cleanly in miles and kilometres.
pub fn format_distance(km: f64) -> String {
    let miles = km * 0.621_371;
    if miles < 1.0 {
        format!("< 1 mi ({km:.1} km)")
    } else if miles < 10.0 {
        format!("{miles:.1} mi ({km:.1} km)")
    } else {
        format!("{miles:.0} mi ({km:.0} km)")
    }
}

/// Pure, deterministic filter + optional Haversine distance + sort + pagination over one state.
pub fn search_catalog(catalog: &StateCatalog, query: &RecoveryQuery) -> RecoveryResults {
    let words: Vec<String> = query
        .text
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let radius = f64::from(query.radius_km.clamp(MIN_RADIUS_KM, MAX_RADIUS_KM));

    let mut matched: Vec<(&Facility, Option<f64>)> = catalog
        .facilities
        .iter()
        .filter(|f| !query.only_enriched || f.is_enriched())
        .filter(|f| matches_words(f, &words))
        .filter(|f| query.care.is_empty() || query.care.iter().any(|c| f.care.contains(c)))
        .filter(|f| {
            query.settings.is_empty() || query.settings.iter().any(|s| f.settings.contains(s))
        })
        .filter(|f| {
            query.payments.is_empty() || query.payments.iter().any(|p| f.payments.contains(p))
        })
        .filter(|f| {
            query
                .amenities
                .iter()
                .all(|a| f.lifestyle.amenities.contains(a))
        })
        .filter(|f| {
            query.food.is_empty()
                || f.lifestyle
                    .food
                    .is_some_and(|food| query.food.contains(&food))
        })
        .filter(|f| {
            query
                .dietary
                .iter()
                .all(|d| f.lifestyle.dietary.contains(d))
        })
        .filter(|f| {
            query.electronics.is_empty()
                || f.lifestyle
                    .electronics
                    .is_some_and(|pol| query.electronics.contains(&pol))
        })
        .filter(|f| {
            query.smoking.is_empty()
                || f.lifestyle
                    .smoking
                    .is_some_and(|pol| query.smoking.contains(&pol))
        })
        .filter(|f| {
            query.vaping.is_empty()
                || f.lifestyle
                    .vaping
                    .is_some_and(|pol| query.vaping.contains(&pol))
        })
        .filter(|f| matches_staff_licenses(f, &query.staff_licenses, query.staff_match_mode))
        .filter_map(|facility| match query.origin {
            None => Some((facility, None)),
            Some(origin) => {
                let d = facility.location?.distance_km(origin);
                (d <= radius).then_some((facility, Some(d)))
            }
        })
        .collect();

    let by_name = |a: &(&Facility, Option<f64>), b: &(&Facility, Option<f64>)| {
        a.0.name
            .to_lowercase()
            .cmp(&b.0.name.to_lowercase())
            .then_with(|| {
                a.0.site_name
                    .as_deref()
                    .map(str::to_lowercase)
                    .cmp(&b.0.site_name.as_deref().map(str::to_lowercase))
            })
    };

    match query.sort {
        RecoverySort::Distance if query.origin.is_some() => {
            matched.sort_by(|a, b| match (a.1, b.1) {
                (Some(da), Some(db)) => da.total_cmp(&db).then_with(|| by_name(a, b)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => by_name(a, b),
            });
        }
        _ => matched.sort_by(by_name),
    }

    let total = matched.len();
    let page_count = total.div_ceil(PAGE_SIZE);
    let page = if page_count == 0 {
        0
    } else {
        query.page.min(page_count.saturating_sub(1))
    };
    let start = page.saturating_mul(PAGE_SIZE);
    let hits = matched
        .into_iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|(facility, distance_km)| RecoveryHit {
            facility: facility.clone(),
            distance_km,
        })
        .collect();

    RecoveryResults {
        hits,
        total,
        page,
        page_count,
    }
}

fn matches_staff_licenses(
    facility: &Facility,
    wanted: &[StaffLicense],
    mode: StaffMatchMode,
) -> bool {
    if wanted.is_empty() {
        return true;
    }
    match mode {
        StaffMatchMode::Any => wanted
            .iter()
            .any(|lic| facility.staff_licenses.contains(lic)),
        StaffMatchMode::All => wanted
            .iter()
            .all(|lic| facility.staff_licenses.contains(lic)),
    }
}

fn matches_words(facility: &Facility, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    let mut fields = vec![
        facility.name.to_lowercase(),
        facility
            .site_name
            .as_deref()
            .unwrap_or_default()
            .to_lowercase(),
        facility.address.city.to_lowercase(),
        facility.address.zip.to_lowercase(),
    ];
    if let Some(notes) = &facility.lifestyle.notes {
        fields.push(notes.to_lowercase());
    }
    for amenity in &facility.lifestyle.amenities {
        fields.push(amenity.label().to_lowercase());
    }
    if let Some(food) = facility.lifestyle.food {
        fields.push(food.label().to_lowercase());
    }
    for lic in &facility.staff_licenses {
        fields.push(lic.label().to_lowercase());
    }
    words
        .iter()
        .all(|word| fields.iter().any(|field| field.contains(word.as_str())))
}
