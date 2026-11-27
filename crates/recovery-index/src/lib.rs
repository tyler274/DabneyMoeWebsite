//! Shared recovery & mental-health facility index schema, deterministic search engine,
//! and (under the `cli` feature) 4-layer cached in-repo updater tool.

pub mod schema;
pub mod search;

#[cfg(feature = "cli")]
pub mod updater;

pub use schema::*;
pub use search::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_facility(
        name: &str,
        amenities: Vec<Amenity>,
        food: Option<FoodSituation>,
        electronics: Option<ElectronicsPolicy>,
        staff_licenses: Vec<StaffLicense>,
    ) -> Facility {
        Facility {
            id: None,
            name: name.to_string(),
            site_name: None,
            address: Address {
                street: Some("100 Mountain Way".to_string()),
                street2: None,
                city: "Boulder".to_string(),
                state: "CO".to_string(),
                zip: "80302".to_string(),
            },
            phone: Some("303-555-0100".to_string()),
            website: Some("https://example.org".to_string()),
            location: Some(Coordinates {
                lat: 40.015,
                lng: -105.2705,
            }),
            care: vec![CareType::MentalHealth, CareType::SubstanceUse],
            settings: vec![Setting::Residential],
            payments: vec![Payment::PrivateInsurance],
            groups: vec![Group::CoOccurring],
            lifestyle: FacilityLifestyle {
                amenities,
                food,
                dietary: vec![DietaryOption::VeganVegetarian],
                electronics,
                smoking: Some(SmokingPolicy::DesignatedArea),
                vaping: Some(VapingPolicy::NotPermitted),
                languages: vec!["Spanish".to_string()],
                notes: Some("Heated pool and mountain views".to_string()),
            },
            staff_licenses,
            enrichment: None,
        }
    }

    #[test]
    fn staff_license_filter_never_conflates_md_do_psychiatrist_or_np() {
        let catalog = StateCatalog {
            state: "CO".to_string(),
            retrieved: "2026-11-27".to_string(),
            facilities: vec![
                sample_facility(
                    "MD Psych Center",
                    vec![Amenity::Pool, Amenity::HotTub],
                    Some(FoodSituation::ChefPrepared),
                    Some(ElectronicsPolicy::Scheduled),
                    vec![StaffLicense::MdPsychiatrist, StaffLicense::Lcsw],
                ),
                sample_facility(
                    "DO Psych Center",
                    vec![Amenity::Pool],
                    Some(FoodSituation::DietitianCatered),
                    Some(ElectronicsPolicy::WorkApproved),
                    vec![StaffLicense::DoPsychiatrist, StaffLicense::LpcLmhc],
                ),
                sample_facility(
                    "NP Clinic",
                    vec![Amenity::Gym],
                    Some(FoodSituation::Cafeteria),
                    Some(ElectronicsPolicy::Prohibited),
                    vec![StaffLicense::Pmhnp, StaffLicense::Np, StaffLicense::Pa],
                ),
            ],
        };

        // MD Psychiatrist must NOT match DO Psychiatrist or PMHNP.
        let md_only = search_catalog(
            &catalog,
            &RecoveryQuery {
                staff_licenses: vec![StaffLicense::MdPsychiatrist],
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(md_only.total, 1);
        assert_eq!(md_only.hits[0].facility.name, "MD Psych Center");

        // DO Psychiatrist must NOT match MD Psychiatrist.
        let do_only = search_catalog(
            &catalog,
            &RecoveryQuery {
                staff_licenses: vec![StaffLicense::DoPsychiatrist],
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(do_only.total, 1);
        assert_eq!(do_only.hits[0].facility.name, "DO Psych Center");

        // PMHNP must NOT match MD or DO Psychiatrist.
        let pmhnp_only = search_catalog(
            &catalog,
            &RecoveryQuery {
                staff_licenses: vec![StaffLicense::Pmhnp],
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(pmhnp_only.total, 1);
        assert_eq!(pmhnp_only.hits[0].facility.name, "NP Clinic");

        // StaffMatchMode::Any matches either MD or DO Psychiatrist.
        let either_psych = search_catalog(
            &catalog,
            &RecoveryQuery {
                staff_licenses: vec![StaffLicense::MdPsychiatrist, StaffLicense::DoPsychiatrist],
                staff_match_mode: StaffMatchMode::Any,
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(either_psych.total, 2);

        // StaffMatchMode::All requires both MD Psychiatrist AND LCSW.
        let md_and_lcsw = search_catalog(
            &catalog,
            &RecoveryQuery {
                staff_licenses: vec![StaffLicense::MdPsychiatrist, StaffLicense::Lcsw],
                staff_match_mode: StaffMatchMode::All,
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(md_and_lcsw.total, 1);
        assert_eq!(md_and_lcsw.hits[0].facility.name, "MD Psych Center");
    }

    #[test]
    fn amenity_food_and_electronics_filters_compose_cleanly() {
        let catalog = StateCatalog {
            state: "CO".to_string(),
            retrieved: "2026-11-27".to_string(),
            facilities: vec![
                sample_facility(
                    "Resort Retreat",
                    vec![Amenity::Pool, Amenity::HotTub, Amenity::PrivateRooms],
                    Some(FoodSituation::ChefPrepared),
                    Some(ElectronicsPolicy::WorkApproved),
                    vec![StaffLicense::MdPsychiatrist],
                ),
                sample_facility(
                    "Pool Only Lodge",
                    vec![Amenity::Pool],
                    Some(FoodSituation::Cafeteria),
                    Some(ElectronicsPolicy::Prohibited),
                    vec![StaffLicense::Pmhnp],
                ),
            ],
        };

        // Amenities use AND semantics: Pool + HotTub matches only Resort Retreat.
        let both = search_catalog(
            &catalog,
            &RecoveryQuery {
                amenities: vec![Amenity::Pool, Amenity::HotTub],
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(both.total, 1);
        assert_eq!(both.hits[0].facility.name, "Resort Retreat");

        // Food + Electronics filter.
        let work_chef = search_catalog(
            &catalog,
            &RecoveryQuery {
                food: vec![FoodSituation::ChefPrepared],
                electronics: vec![ElectronicsPolicy::WorkApproved],
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(work_chef.total, 1);
        assert_eq!(work_chef.hits[0].facility.name, "Resort Retreat");
    }

    #[cfg(feature = "cli")]
    #[test]
    fn nppes_classifier_strictly_distinguishes_credentials() {
        use crate::updater::nppes::classify_nppes_provider;

        assert_eq!(
            classify_nppes_provider("M.D.", &["2084P0800X".into()], &["Psychiatry".into()]),
            Some(StaffLicense::MdPsychiatrist)
        );
        assert_eq!(
            classify_nppes_provider("D.O.", &["2084P0800X".into()], &["Psychiatry".into()]),
            Some(StaffLicense::DoPsychiatrist)
        );
        // Never guess MD vs DO when credential is blank!
        assert_eq!(
            classify_nppes_provider("", &["2084P0800X".into()], &["Psychiatry".into()]),
            None
        );
        assert_eq!(
            classify_nppes_provider(
                "PMHNP-BC",
                &["363LP0808X".into()],
                &["Psych/Mental Health".into()]
            ),
            Some(StaffLicense::Pmhnp)
        );
        assert_eq!(
            classify_nppes_provider("FNP-C", &["363LF0000X".into()], &["Family".into()]),
            Some(StaffLicense::Np)
        );
        assert_eq!(
            classify_nppes_provider(
                "PA-C",
                &["363A00000X".into()],
                &["Physician Assistant".into()]
            ),
            Some(StaffLicense::Pa)
        );
        assert_eq!(
            classify_nppes_provider("Psy.D.", &["103TC0700X".into()], &["Clinical".into()]),
            Some(StaffLicense::PsydPsychologist)
        );
        assert_eq!(
            classify_nppes_provider("Ph.D.", &["103TC0700X".into()], &["Clinical".into()]),
            Some(StaffLicense::PhdPsychologist)
        );
    }

    #[cfg(feature = "cli")]
    #[test]
    fn website_extractor_detects_amenities_electronics_and_avoids_false_positives() {
        use crate::updater::website::extract_signals_from_text;

        let sample = "Welcome to Alpine Recovery in Baltimore, MD 21201. WHAT WE DO: \
            Our residential campus features a heated swimming pool, outdoor hot tub, private rooms, \
            and gourmet meals prepared by our private chef with vegan and gluten-free options. \
            Cell phones are allowed during designated evening hours after a 7-day blackout period. \
            Medical Director Dr. Elena Ruiz, D.O. (board-certified psychiatrist) leads care alongside \
            Marcus Chen, M.D. (addiction medicine specialist) and Sarah Patel, PMHNP-BC, and Alex Kim, LCSW.";

        let sig = extract_signals_from_text(sample);
        assert!(sig.amenities.contains(&Amenity::Pool));
        assert!(sig.amenities.contains(&Amenity::HotTub));
        assert!(sig.amenities.contains(&Amenity::PrivateRooms));
        assert_eq!(sig.food, Some(FoodSituation::ChefPrepared));
        assert!(sig.dietary.contains(&DietaryOption::VeganVegetarian));
        assert!(sig.dietary.contains(&DietaryOption::GlutenFree));
        assert_eq!(sig.electronics, Some(ElectronicsPolicy::Scheduled));
        assert!(sig.staff_licenses.contains(&StaffLicense::DoPsychiatrist));
        assert!(sig.staff_licenses.contains(&StaffLicense::MdAddiction));
        assert!(sig.staff_licenses.contains(&StaffLicense::Pmhnp));
        assert!(sig.staff_licenses.contains(&StaffLicense::Lcsw));
        // Must NOT falsely detect generic NP from PMHNP-BC or MdPhysician from "Baltimore, MD 21201"
        assert!(!sig.staff_licenses.contains(&StaffLicense::Np));
        assert!(!sig.staff_licenses.contains(&StaffLicense::MdPhysician));
        assert!(!sig.staff_licenses.contains(&StaffLicense::MdPsychiatrist));
    }

    #[cfg(feature = "cli")]
    #[test]
    fn website_extractor_respects_no_phone_and_no_pet_negations() {
        use crate::updater::website::extract_signals_from_text;

        let strict =
            "We maintain a phone-free environment; cell phones are not permitted on campus. \
            Pets are not allowed at our facility.";
        let sig = extract_signals_from_text(strict);
        assert_eq!(sig.electronics, Some(ElectronicsPolicy::Prohibited));
        assert!(!sig.amenities.contains(&Amenity::PetFriendly));
    }

    #[cfg(feature = "cli")]
    #[test]
    fn override_application_wins_and_can_remove_false_positives() {
        use crate::updater::overrides::FacilityOverride;

        let mut fac = sample_facility(
            "Cedar Lodge",
            vec![Amenity::Pool, Amenity::Wifi],
            Some(FoodSituation::Cafeteria),
            Some(ElectronicsPolicy::Prohibited),
            vec![StaffLicense::Np],
        );

        let ov = FacilityOverride {
            id: fac.stable_id(),
            name: Some(fac.name.clone()),
            amenities: vec![Amenity::HotTub, Amenity::PrivateRooms],
            remove_amenities: vec![Amenity::Wifi],
            food: Some(FoodSituation::ChefPrepared),
            electronics: Some(ElectronicsPolicy::Scheduled),
            staff_licenses: vec![StaffLicense::DoPsychiatrist, StaffLicense::Pmhnp],
            remove_staff_licenses: vec![StaffLicense::Np],
            verified_at: Some("2026-11-27".to_string()),
            ..FacilityOverride::default()
        };
        ov.apply_to(&mut fac);

        assert!(fac.lifestyle.amenities.contains(&Amenity::Pool));
        assert!(fac.lifestyle.amenities.contains(&Amenity::HotTub));
        assert!(fac.lifestyle.amenities.contains(&Amenity::PrivateRooms));
        assert!(!fac.lifestyle.amenities.contains(&Amenity::Wifi));
        assert_eq!(fac.lifestyle.food, Some(FoodSituation::ChefPrepared));
        assert_eq!(
            fac.lifestyle.electronics,
            Some(ElectronicsPolicy::Scheduled)
        );
        assert_eq!(
            fac.staff_licenses,
            vec![StaffLicense::DoPsychiatrist, StaffLicense::Pmhnp]
        );
        assert!(fac
            .enrichment
            .as_ref()
            .is_some_and(|m| m.sources.contains(&DataSource::Verified)));
    }

    #[cfg(feature = "cli")]
    #[test]
    fn llm_json_parser_handles_direct_fenced_and_claude_wrapped_outputs() {
        use crate::updater::llm::{extraction_json_schema, parse_llm_json};

        // Schema has every required property
        let schema = extraction_json_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["amenities"].is_object());
        assert!(schema["properties"]["staff_licenses"].is_object());

        // Direct JSON with markdown fences & unknown code safely dropped
        let fenced = "```json\n{\
            \"amenities\": [\"pool\", \"hot-tub\", \"made-up-code\"],\
            \"food\": \"chef-prepared\",\
            \"dietary\": [\"vegan-vegetarian\", \"gluten-free\"],\
            \"electronics\": \"scheduled\",\
            \"smoking\": \"designated-area\",\
            \"vaping\": null,\
            \"staff_licenses\": [\"do-psychiatrist\", \"pmhnp\"],\
            \"notes\": \"Heated outdoor pool & jacuzzi; cell phones allowed evenings after 5 days.\"\
        }\n```";
        let ext = parse_llm_json(fenced).expect("fenced JSON parses");
        assert_eq!(ext.amenities, vec![Amenity::Pool, Amenity::HotTub]);
        assert_eq!(ext.food, Some(FoodSituation::ChefPrepared));
        assert_eq!(ext.electronics, Some(ElectronicsPolicy::Scheduled));
        assert_eq!(
            ext.staff_licenses,
            vec![StaffLicense::DoPsychiatrist, StaffLicense::Pmhnp]
        );
        assert!(ext
            .notes
            .as_deref()
            .unwrap()
            .contains("Heated outdoor pool"));

        // Claude CLI `--output-format json` wrapper (`structured_output`)
        let claude_wrapped = r#"{"type":"result","structured_output":{"amenities":["private-rooms"],"food":"cafeteria","dietary":[],"electronics":"prohibited","smoking":null,"vaping":null,"staff_licenses":["md-psychiatrist"],"notes":null}}"#;
        let ext2 = parse_llm_json(claude_wrapped).expect("claude wrapper parses");
        assert_eq!(ext2.amenities, vec![Amenity::PrivateRooms]);
        assert_eq!(ext2.electronics, Some(ElectronicsPolicy::Prohibited));
        assert_eq!(ext2.staff_licenses, vec![StaffLicense::MdPsychiatrist]);
    }
}
