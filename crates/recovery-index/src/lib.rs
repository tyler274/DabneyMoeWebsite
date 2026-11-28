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
            staffing_profile: None,
            state_licenses: Vec::new(),
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
            staff_network: Vec::new(),
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
            staff_network: Vec::new(),
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

    #[cfg(feature = "cli")]
    #[test]
    fn shared_staff_network_distinguishes_dedicated_physicians_from_visiting_providers() {
        use crate::updater::network::{
            rebuild_state_staff_network, NetworkRebuildMode, ProviderObservation,
        };

        let mut fac_a = sample_facility(
            "Scottsdale Dedicated Clinic",
            vec![Amenity::Pool],
            Some(FoodSituation::ChefPrepared),
            Some(ElectronicsPolicy::Scheduled),
            vec![StaffLicense::MdPsychiatrist],
        );
        fac_a.address.state = "AZ".to_string();
        fac_a.address.city = "Scottsdale".to_string();
        fac_a.address.zip = "85251".to_string();
        let id_a = fac_a.stable_id();

        let mut fac_b = sample_facility(
            "Phoenix Shared Residential",
            vec![Amenity::YogaStudio],
            Some(FoodSituation::Cafeteria),
            Some(ElectronicsPolicy::Prohibited),
            vec![StaffLicense::Pmhnp],
        );
        fac_b.address.state = "AZ".to_string();
        fac_b.address.city = "Phoenix".to_string();
        fac_b.address.zip = "85016".to_string();
        let id_b = fac_b.stable_id();

        let mut catalog = StateCatalog {
            state: "AZ".to_string(),
            retrieved: "2026-11-27".to_string(),
            facilities: vec![fac_a, fac_b],
            staff_network: Vec::new(),
        };

        // 1 dedicated MD Psychiatrist at Clinic A, 1 shared PMHNP across A & B, and 1 curated
        // visiting yoga instructor serving A & B.
        let observations = vec![
            ProviderObservation {
                id: "npi-1111111111".to_string(),
                name: "Dr. Elena Vance, MD".to_string(),
                organization: None,
                npi: Some("1111111111".to_string()),
                role: ProviderRole::Clinical(StaffLicense::MdPsychiatrist),
                arrangement_hint: StaffArrangement::Dedicated,
                facility_id: id_a.clone(),
                source: DataSource::Nppes,
                notes: None,
            },
            ProviderObservation {
                id: "npi-2222222222".to_string(),
                name: "Jordan Lee, PMHNP-BC".to_string(),
                organization: None,
                npi: Some("2222222222".to_string()),
                role: ProviderRole::Clinical(StaffLicense::Pmhnp),
                arrangement_hint: StaffArrangement::Dedicated,
                facility_id: id_a.clone(),
                source: DataSource::Nppes,
                notes: None,
            },
            ProviderObservation {
                id: "npi-2222222222".to_string(),
                name: "Jordan Lee, PMHNP-BC".to_string(),
                organization: None,
                npi: Some("2222222222".to_string()),
                role: ProviderRole::Clinical(StaffLicense::Pmhnp),
                arrangement_hint: StaffArrangement::Dedicated,
                facility_id: id_b.clone(),
                source: DataSource::Nppes,
                notes: None,
            },
        ];

        let curated = vec![NetworkProvider {
            id: "staff-az-jacob-yoga".to_string(),
            name: "Jacob".to_string(),
            organization: Some("Scottsdale Recovery Yoga".to_string()),
            npi: None,
            role: ProviderRole::Wellness(WellnessRole::YogaInstructor),
            arrangement: StaffArrangement::Visiting,
            facility_ids: vec![id_a.clone(), id_b.clone()],
            sources: vec![DataSource::Verified],
            state_licenses: Vec::new(),
            notes: Some("Provides yoga classes across Phoenix/Scottsdale programs.".to_string()),
        }];

        rebuild_state_staff_network(
            &mut catalog,
            &observations,
            &curated,
            NetworkRebuildMode::FullNppesRefresh,
        );

        // Published network keeps shared PMHNP + visiting yoga instructor (not single-site MD).
        assert_eq!(catalog.staff_network.len(), 2);
        let prof_a = catalog.facilities[0]
            .staffing_profile
            .as_ref()
            .expect("profile A");
        assert_eq!(prof_a.dedicated_physicians, 1);
        assert_eq!(prof_a.shared_prescribers, 1);
        assert_eq!(prof_a.visiting_specialists, 1);
        assert_eq!(prof_a.shared_with_facilities, 1);

        let prof_b = catalog.facilities[1]
            .staffing_profile
            .as_ref()
            .expect("profile B");
        assert_eq!(prof_b.dedicated_physicians, 0);
        assert_eq!(prof_b.shared_prescribers, 1);
        assert_eq!(prof_b.visiting_specialists, 1);

        // Incremental merge (e.g. `add-provider` or `--skip-nppes` LLM pass) must preserve
        // existing NPPES shared providers AND dedicated single-site physician counts.
        let mut curated_plus = curated.clone();
        curated_plus.push(NetworkProvider {
            id: "staff-az-art-therapist".to_string(),
            name: "Maya Lin".to_string(),
            organization: None,
            npi: None,
            role: ProviderRole::Wellness(WellnessRole::ArtMusicTherapist),
            arrangement: StaffArrangement::Visiting,
            facility_ids: vec![id_b.clone()],
            sources: vec![DataSource::Verified],
            state_licenses: Vec::new(),
            notes: None,
        });
        rebuild_state_staff_network(
            &mut catalog,
            &[],
            &curated_plus,
            NetworkRebuildMode::IncrementalMerge,
        );
        assert_eq!(catalog.staff_network.len(), 3);
        let prof_a_after = catalog.facilities[0]
            .staffing_profile
            .as_ref()
            .expect("profile A after incremental merge");
        assert_eq!(prof_a_after.dedicated_physicians, 1);
        assert_eq!(prof_a_after.shared_prescribers, 1);
        assert_eq!(prof_a_after.visiting_specialists, 1);
        let prof_b_after = catalog.facilities[1]
            .staffing_profile
            .as_ref()
            .expect("profile B after incremental merge");
        assert_eq!(prof_b_after.visiting_specialists, 2);

        // Dedicated physician filter isolates Clinic A only.
        let dedicated_only = search_catalog(
            &catalog,
            &RecoveryQuery {
                only_dedicated_physician: true,
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(dedicated_only.total, 1);
        assert_eq!(
            dedicated_only.hits[0].facility.name,
            "Scottsdale Dedicated Clinic"
        );

        // Filtering by visiting yoga instructor shows both Clinic A and Clinic B.
        let by_yoga = search_catalog(
            &catalog,
            &RecoveryQuery {
                provider_id: Some("staff-az-jacob-yoga".to_string()),
                ..RecoveryQuery::default()
            },
        );
        assert_eq!(by_yoga.total, 2);
    }
}
