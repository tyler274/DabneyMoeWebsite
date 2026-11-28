//! Verified licensing adapter registry for 24 Western, Plains, and Midwest states:
//! AK, HI, IA, ID, IL, IN, KS, MI, MN, MO, MT, ND, NE, NM, NV, OH, OK, OR, SD, TX, UT, WA, WI, WY.
//!
//! States with dedicated custom adapters (AZ, CA, CO, NY) are handled in their own modules.
//! Every `GenericEndpointSpec` in this file was probed live and confirmed to return HTTP 200
//! with verified JSON field names. Portals without an unauthenticated API or gated by CAPTCHA
//! are modeled honestly as `BulkDownloadExport` or `PortalLookupOnly`.

use crate::schema::StaffLicense;

use super::types::{
    FacilityFieldMap, GenericEndpointSpec, LicensingAccessMode, LicensingDatasetFormat,
    LicensingScope, ProviderFieldMap, StateBoardDescriptor, StateLicensingManifest,
};

const MD_ONLY: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::MdPhysician,
];

const DO_ONLY: &[StaffLicense] = &[
    StaffLicense::DoPsychiatrist,
    StaffLicense::DoAddiction,
    StaffLicense::DoPhysician,
];

const MD_AND_PA: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::Pa,
];

const MD_DO_PA: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::DoPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::DoAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::DoPhysician,
    StaffLicense::Pa,
];

const BEHAVIORAL_CLINICIANS: &[StaffLicense] = &[
    StaffLicense::PhdPsychologist,
    StaffLicense::PsydPsychologist,
    StaffLicense::Lcsw,
    StaffLicense::LpcLmhc,
    StaffLicense::Lmft,
    StaffLicense::AddictionCounselor,
];

const NURSING_AND_APRN: &[StaffLicense] = &[
    StaffLicense::Pmhnp,
    StaffLicense::Np,
    StaffLicense::Rn,
    StaffLicense::Lpn,
];

const ALL_CLINICAL: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::DoPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::DoAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::DoPhysician,
    StaffLicense::Pmhnp,
    StaffLicense::Np,
    StaffLicense::Pa,
    StaffLicense::PhdPsychologist,
    StaffLicense::PsydPsychologist,
    StaffLicense::Lcsw,
    StaffLicense::LpcLmhc,
    StaffLicense::Lmft,
    StaffLicense::Rn,
    StaffLicense::Lpn,
    StaffLicense::AddictionCounselor,
];

const AK_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ak-doh-hflc",
        state: "AK",
        agency_name: "Alaska Department of Health - Health Facilities Licensing and Certification",
        system_name: "AK HFLC Health Care Facility Licensing",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://health.alaska.gov/hcs/licensing/Pages/default.aspx",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses hospitals, residential psychiatric treatment centers, and ambulatory health facilities; import exported state facility lists with --import-file.",
    },
    StateBoardDescriptor {
        id: "ak-cbpl-professional",
        state: "AK",
        agency_name: "Alaska Division of Corporations, Business and Professional Licensing",
        system_name: "AK Professional License Search (Unified Medical, Nursing, Behavioral Boards)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.commerce.alaska.gov/cbp/main/search/professional",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Alaska State Medical Board licenses both MDs and DOs alongside nursing, psychology, social work, and counseling boards.",
    },
];

const HI_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "hi-doh-ohca",
        state: "HI",
        agency_name: "Hawaii Department of Health - Office of Health Care Assurance",
        system_name: "HI OHCA Special Treatment Facility and Hospital Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://health.hawaii.gov/ohca/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Special Treatment Facilities (STFs), Therapeutic Living Programs (TLPs), and psychiatric hospitals.",
    },
    StateBoardDescriptor {
        id: "hi-doh-adad",
        state: "HI",
        agency_name: "Hawaii Department of Health - Alcohol and Drug Abuse Division",
        system_name: "HI ADAD Substance Use Treatment Provider Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://health.hawaii.gov/substance-abuse/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Publishes island-by-island accredited substance use treatment and therapeutic living directories.",
    },
    StateBoardDescriptor {
        id: "hi-dcca-mypvl",
        state: "HI",
        agency_name: "Hawaii Department of Commerce and Consumer Affairs - PVL",
        system_name: "HI MyPVL Public License Search (Hawaii Medical Board & Allied Boards)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://mypvl.dcca.hawaii.gov/public-license-search/",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Hawaii Medical Board licenses both MDs and DOs; MyPVL also covers nursing, psychology, LCSW, LMHC, and LMFT.",
    },
];

const IA_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ia-dial-facilities",
        state: "IA",
        agency_name: "Iowa Department of Inspections, Appeals, and Licensing",
        system_name: "IA DIAL Licensed Health Facilities (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://dial.iowa.gov/licenses/health/health-facilities",
        api_or_download_url: Some(
            "https://services.arcgis.com/vPD5PVLI6sfkZ5E4/arcgis/rest/services/Iowa_Licensed_Health_Facilities/FeatureServer/0/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Iowa DIAL Health Facilities Division",
            layer_query_url: "https://services.arcgis.com/vPD5PVLI6sfkZ5E4/arcgis/rest/services/Iowa_Licensed_Health_Facilities/FeatureServer/0/query",
            where_clause: "Status = 'Active'",
            page_size: 1000,
            max_records: 25_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["License_ID", "Medicaid_ID", "CCN_ID"],
                name: &["Entity_Name"],
                dba_or_site_name: &[],
                license_type: &["Entity_Type"],
                status: &["Status"],
                street: &["StreetAddress"],
                city: &["City"],
                zip: &["Zip"],
                phone: &["Phone"],
                licensed_beds: &["Max_Occupancy"],
                expires_on: &["Expiration_Date"],
                detail_url: &["URL"],
            },
        }),
        notes: "Verified live ArcGIS FeatureServer layer for Iowa DIAL licensed health and residential care facilities.",
    },
    StateBoardDescriptor {
        id: "ia-ibm",
        state: "IA",
        agency_name: "Iowa Board of Medicine (DIAL)",
        system_name: "IA Board of Medicine Public Licensure Portal",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://amanda-portal.idph.state.ia.us/ibm/portal/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Iowa Board of Medicine licenses both MDs and DOs.",
    },
];

const ID_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "id-dhw-bfs",
        state: "ID",
        agency_name: "Idaho Department of Health and Welfare - Bureau of Facility Standards",
        system_name: "ID DHW Facility Licensure and Certification",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://healthandwelfare.idaho.gov/providers/facility-standards",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses hospitals, psychiatric residential treatment facilities (PRTFs), and behavioral health facilities; import exported directories with --import-file.",
    },
    StateBoardDescriptor {
        id: "id-dopl",
        state: "ID",
        agency_name: "Idaho Division of Occupational and Professional Licenses",
        system_name: "ID DOPL Unified Health Professions License Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://dopl.idaho.gov/",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Idaho Board of Medicine licenses both MDs and DOs under DOPL alongside nursing, psychology, social work, and counseling boards.",
    },
];

const IL_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "il-idph-facilities",
        state: "IL",
        agency_name: "Illinois Department of Public Health - Office of Health Care Regulation",
        system_name: "IL IDPH Health Care Facility Lookup",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://dph.illinois.gov/topics-services/health-care-regulation/facilities.html",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses hospitals, psychiatric units, and Specialized Mental Health Rehabilitation Facilities (SMHRFs).",
    },
    StateBoardDescriptor {
        id: "il-dhs-supr",
        state: "IL",
        agency_name: "Illinois Department of Human Services - Division of Behavioral Health and Recovery (formerly SUPR)",
        system_name: "IL DHS Licensed Substance Use Treatment Provider Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://helplineil.org/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Illinois substance use disorder outpatient, IOP, detox, and residential programs.",
    },
    StateBoardDescriptor {
        id: "il-idfpr",
        state: "IL",
        agency_name: "Illinois Department of Financial and Professional Regulation",
        system_name: "IL IDFPR License Lookup",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://idfpr.illinois.gov/lookups/licenselookup.html",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Illinois Medical Licensing Board licenses both MDs and DOs; IDFPR also covers APRNs, PAs, psychologists, LCSWs, LCPCs, and LMFTs.",
    },
];

const IN_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "in-fssa-dmha",
        state: "IN",
        agency_name: "Indiana Family and Social Services Administration - Division of Mental Health and Addiction",
        system_name: "IN DMHA Certified and Licensed Mental Health and Addiction Providers",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.in.gov/fssa/dmha/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Certifies addiction residential treatment programs, CMHCs, OTPs, and private mental health institutions.",
    },
    StateBoardDescriptor {
        id: "in-pla-mylicense",
        state: "IN",
        agency_name: "Indiana Professional Licensing Agency",
        system_name: "IN PLA Free Search and Verify (MyLicense)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://mylicense.in.gov/EVerification/Search.aspx",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Medical Licensing Board of Indiana licenses both MDs and DOs; MyLicense also covers nursing and behavioral health boards.",
    },
];

const KS_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ks-kdads-bhs",
        state: "KS",
        agency_name: "Kansas Department for Aging and Disability Services - Behavioral Health Services",
        system_name: "KS KDADS Behavioral Health and SUD Facilities Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.kdads.ks.gov/commissions/behavioral-health",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses substance use disorder treatment programs, CMHCs, psychiatric hospitals, and residential care facilities.",
    },
    StateBoardDescriptor {
        id: "ks-ksbha",
        state: "KS",
        agency_name: "Kansas State Board of Healing Arts",
        system_name: "KS KSBHA Licensee Profile Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://prolicenseverify.ks.gov/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Kansas State Board of Healing Arts licenses both MDs and DOs as well as PAs.",
    },
];

const MI_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "mi-lara-sud",
        state: "MI",
        agency_name: "Michigan Department of Licensing and Regulatory Affairs - Bureau of Community and Health Systems",
        system_name: "MI LARA Substance Use Disorder Care Facilities (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://www.michigan.gov/lara/bureau-list/bchs",
        api_or_download_url: Some(
            "https://utility.arcgis.com/usrsvcs/servers/da8c64b11d1c410b9f975fb5c6d09e5a/rest/services/CSS/CSS_LARA/MapServer/7/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Michigan LARA BCHS Substance Use Disorder Programs",
            layer_query_url: "https://utility.arcgis.com/usrsvcs/servers/da8c64b11d1c410b9f975fb5c6d09e5a/rest/services/CSS/CSS_LARA/MapServer/7/query",
            where_clause: "1=1",
            page_size: 1000,
            max_records: 25_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["LicenseNumber"],
                name: &["Name"],
                dba_or_site_name: &[],
                license_type: &["FacilityType", "FacilityTypeCode"],
                status: &[],
                street: &["StreetAddress", "stdStreetAddress"],
                city: &["City", "stdCity"],
                zip: &["Zip", "stdZip"],
                phone: &[],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live Michigan LARA MapServer layer 7 (BCHS_SUD_Care) covering licensed Michigan substance use disorder facilities.",
    },
    StateBoardDescriptor {
        id: "mi-lara-medicine",
        state: "MI",
        agency_name: "Michigan Board of Medicine (LARA MiPLUS)",
        system_name: "MI MiPLUS Allopathic Medicine License Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.michigan.gov/miplus",
        api_or_download_url: None,
        covers_licenses: MD_ONLY,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate allopathic Board of Medicine for MDs under Michigan LARA.",
    },
    StateBoardDescriptor {
        id: "mi-lara-osteopathic",
        state: "MI",
        agency_name: "Michigan Board of Osteopathic Medicine and Surgery (LARA MiPLUS)",
        system_name: "MI MiPLUS Osteopathic Medicine License Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.michigan.gov/miplus",
        api_or_download_url: None,
        covers_licenses: DO_ONLY,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate Michigan Board of Osteopathic Medicine and Surgery for DOs under Michigan LARA.",
    },
];

const MN_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "mn-dhs-licensing",
        state: "MN",
        agency_name: "Minnesota Department of Human Services - Licensing Division",
        system_name: "MN DHS Licensing Information Lookup",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://licensinglookup.dhs.state.mn.us/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Primary registry for Minnesota licensed substance use disorder (245G) and mental health treatment programs.",
    },
    StateBoardDescriptor {
        id: "mn-mdh-hospitals",
        state: "MN",
        agency_name: "Minnesota Department of Health",
        system_name: "MN MDH Licensed Hospitals (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://www.health.state.mn.us/",
        api_or_download_url: Some(
            "https://enterprise.gisdata.mn.gov/aghost/rest/services/us_mn_state_mngeo/health_facility_hospitals/FeatureServer/0/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Minnesota Department of Health",
            layer_query_url: "https://enterprise.gisdata.mn.gov/aghost/rest/services/us_mn_state_mngeo/health_facility_hospitals/FeatureServer/0/query",
            where_clause: "1=1",
            page_size: 1000,
            max_records: 5_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["hfid"],
                name: &["name"],
                dba_or_site_name: &[],
                license_type: &["provider_type", "license_type"],
                status: &[],
                street: &["address"],
                city: &["city"],
                zip: &["zip"],
                phone: &["telephone"],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live MnGeo ArcGIS FeatureServer layer for Minnesota MDH licensed hospitals.",
    },
    StateBoardDescriptor {
        id: "mn-bmp",
        state: "MN",
        agency_name: "Minnesota Board of Medical Practice",
        system_name: "MN Board of Medical Practice Profile Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://mn.gov/boards/medical-practice/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Minnesota Board of Medical Practice licenses both MDs and DOs as well as PAs.",
    },
];

const MO_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "mo-dhss-hospitals",
        state: "MO",
        agency_name: "Missouri Department of Health and Senior Services",
        system_name: "MO DHSS Profile of Hospitals (Socrata Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://data.mo.gov/Health/Profile-of-Hospitals/q8me-hzr8",
        api_or_download_url: Some("https://data.mo.gov/resource/q8me-hzr8.json"),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
            agency: "Missouri Department of Health and Senior Services",
            endpoint_url: "https://data.mo.gov/resource/q8me-hzr8.json",
            where_clause: None,
            page_size: 1000,
            max_records: 5_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["dhss_license", "facility_id"],
                name: &["facility_name"],
                dba_or_site_name: &[],
                license_type: &["hospital_license_type", "facility_type"],
                status: &[],
                street: &["address"],
                city: &["city"],
                zip: &["zip"],
                phone: &["phone"],
                licensed_beds: &[
                    "psych_beds_licensed",
                    "alcohol_drug_treatment_beds",
                    "total_beds_licensed_for_this",
                ],
                expires_on: &["current_license_expiration"],
                detail_url: &[],
            },
        }),
        notes: "Verified live Socrata dataset q8me-hzr8 including psychiatric hospitals and licensed psychiatric/SUD bed counts.",
    },
    StateBoardDescriptor {
        id: "mo-dmh-bh",
        state: "MO",
        agency_name: "Missouri Department of Mental Health - Division of Behavioral Health",
        system_name: "MO DMH Certified Behavioral Health Provider Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://dmh.mo.gov/behavioral-health",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Certifies Missouri substance use disorder (CSTAR) and community mental health providers.",
    },
    StateBoardDescriptor {
        id: "mo-mopro",
        state: "MO",
        agency_name: "Missouri Division of Professional Registration",
        system_name: "MO MOPRO Licensee Search (Healing Arts & Behavioral Health)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://mopro.mo.gov/",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Missouri Board of Registration for the Healing Arts licenses both MDs and DOs.",
    },
];

const MT_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "mt-dphhs-bhdd",
        state: "MT",
        agency_name: "Montana Department of Public Health and Human Services - BHDD",
        system_name: "MT DPHHS Behavioral Health and SUD Facility Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://dphhs.mt.gov/bhdd/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Montana chemical dependency programs, mental health centers, and residential behavioral health facilities.",
    },
    StateBoardDescriptor {
        id: "mt-dli-bsd",
        state: "MT",
        agency_name: "Montana Department of Labor and Industry - Business Standards Division",
        system_name: "MT Licensee Lookup System (Board of Medical Examiners & Behavioral Health)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://boards.bsd.dli.mt.gov/",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Montana Board of Medical Examiners licenses both MDs and DOs alongside nursing and behavioral health boards.",
    },
];

const ND_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "nd-hhs-bh",
        state: "ND",
        agency_name: "North Dakota Department of Health and Human Services",
        system_name: "ND HHS Licensed Behavioral Health and SUD Program Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.hhs.nd.gov/behavioral-health",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Centralized registry for North Dakota state-licensed substance use disorder and mental health treatment programs.",
    },
    StateBoardDescriptor {
        id: "nd-bom",
        state: "ND",
        agency_name: "North Dakota Board of Medicine",
        system_name: "ND Board of Medicine License Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.ndbom.org/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified North Dakota Board of Medicine licenses both MDs and DOs as well as PAs.",
    },
];

const NE_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ne-dhhs-facilities",
        state: "NE",
        agency_name: "Nebraska Department of Health and Human Services - Division of Public Health",
        system_name: "NE DHHS Health Care Facility Licensure Search",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://dhhs.ne.gov/Pages/Behavioral-Health.aspx",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Nebraska mental health centers, substance abuse treatment centers, and psychiatric hospitals.",
    },
    StateBoardDescriptor {
        id: "ne-dhhs-lis",
        state: "NE",
        agency_name: "Nebraska Department of Health and Human Services - Licensure Unit",
        system_name: "NE License Information System (LIS) Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.nebraska.gov/LISSearch/search.cgi",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Nebraska Board of Medicine and Surgery licenses both MDs and DOs; LIS also covers APRNs, PAs, psychologists, and mental health practitioners.",
    },
];

const NM_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "nm-hca-bhsd",
        state: "NM",
        agency_name: "New Mexico Health Care Authority - Division of Health Improvement and BHSD",
        system_name: "NM Health Facility Licensing and Behavioral Health Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.hca.nm.gov/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses behavioral health agencies, crisis triage centers, and psychiatric facilities under NMAC 7.20.12.",
    },
    StateBoardDescriptor {
        id: "nm-nmmb",
        state: "NM",
        agency_name: "New Mexico Medical Board",
        system_name: "NM Medical Board License Search (Unified MD and DO)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://nmrldlpi.my.site.com/nmmb/s/searchlicense",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Former New Mexico Board of Osteopathic Medicine merged into the unified New Mexico Medical Board effective June 18, 2021.",
    },
];

const NV_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "nv-dpbh-hcqc",
        state: "NV",
        agency_name: "Nevada Division of Public and Behavioral Health - Bureau of Health Care Quality and Compliance",
        system_name: "NV DPBH Licensed Health and Behavioral Facilities Search",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://dpbh.nv.gov/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Nevada psychiatric hospitals, residential treatment centers, and substance use facilities under NRS Chapter 449.",
    },
    StateBoardDescriptor {
        id: "nv-nsbme",
        state: "NV",
        agency_name: "Nevada State Board of Medical Examiners",
        system_name: "NV NSBME Allopathic Physician License Register",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://nsbme.us.thentiacloud.net/webs/nsbme/register/",
        api_or_download_url: None,
        covers_licenses: MD_AND_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Separate allopathic board for Nevada MDs and PAs (DOs are licensed by the separate Nevada State Board of Osteopathic Medicine).",
    },
    StateBoardDescriptor {
        id: "nv-bom-osteopathic",
        state: "NV",
        agency_name: "Nevada State Board of Osteopathic Medicine",
        system_name: "NV Osteopathic Medicine License Verification",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://bom.nv.gov/",
        api_or_download_url: None,
        covers_licenses: DO_ONLY,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Independent Nevada State Board of Osteopathic Medicine licensing DOs separately from NSBME.",
    },
];

const OH_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "oh-dbh-licensure",
        state: "OH",
        agency_name: "Ohio Department of Mental Health and Addiction Services (OhioMHAS / DBH)",
        system_name: "OH Licensure and Certification Behavioral Health Provider Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://dbh.ohio.gov/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Certifies Ohio community behavioral health providers, OTPs, residential SUD facilities, and private psychiatric hospitals.",
    },
    StateBoardDescriptor {
        id: "oh-elicense",
        state: "OH",
        agency_name: "Ohio eLicense Portal (State Medical Board, Nursing, CSWMFT, Psychology, OCDP)",
        system_name: "OH eLicense Verify a License",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://elicense.ohio.gov/oh_verifylicense",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified State Medical Board of Ohio licenses both MDs and DOs; eLicense also covers nursing, psychology, CSWMFT, and chemical dependency boards.",
    },
];

const OK_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ok-odmhsas",
        state: "OK",
        agency_name: "Oklahoma Department of Mental Health and Substance Abuse Services",
        system_name: "OK ODMHSAS Certified Behavioral Health and SUD Providers",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://oklahoma.gov/odmhsas.html",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Certifies Oklahoma alcohol and drug treatment programs, CMHCs, crisis centers, and OTPs.",
    },
    StateBoardDescriptor {
        id: "ok-obmls",
        state: "OK",
        agency_name: "Oklahoma State Board of Medical Licensure and Supervision",
        system_name: "OK Medical Board Allopathic License Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.okmedicalboard.org/search",
        api_or_download_url: None,
        covers_licenses: MD_AND_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Separate allopathic board for Oklahoma MDs and PAs (DOs are licensed by OSBOE).",
    },
    StateBoardDescriptor {
        id: "ok-osboe",
        state: "OK",
        agency_name: "Oklahoma State Board of Osteopathic Examiners",
        system_name: "OK OSBOE Osteopathic License Register",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://osboe.portalus.thentiacloud.net/webs/portal/register",
        api_or_download_url: None,
        covers_licenses: DO_ONLY,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Independent Oklahoma State Board of Osteopathic Examiners licensing DOs separately from OBMLS.",
    },
];

const OR_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "or-oha-facilities",
        state: "OR",
        agency_name: "Oregon Health Authority",
        system_name: "OR OHA Licensed Medical Facilities (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://www.oregon.gov/oha/HPA/ANALYTICS/Pages/Data-Profiles.aspx",
        api_or_download_url: Some(
            "https://services.arcgis.com/uUvqNMGPm7axC2dD/ArcGIS/rest/services/OHA_LicensedFacilities/FeatureServer/0/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Oregon Health Authority",
            layer_query_url: "https://services.arcgis.com/uUvqNMGPm7axC2dD/ArcGIS/rest/services/OHA_LicensedFacilities/FeatureServer/0/query",
            where_clause: "1=1",
            page_size: 1000,
            max_records: 25_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["USER_STATE_ID", "USER_CMS_ID"],
                name: &["USER_FACILITY_NAME"],
                dba_or_site_name: &[],
                license_type: &["USER_TYPE"],
                status: &[],
                street: &["USER_FULL_ADDRESS", "Match_addr"],
                city: &[],
                zip: &[],
                phone: &[],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live ArcGIS FeatureServer layer for Oregon Health Authority licensed facilities.",
    },
    StateBoardDescriptor {
        id: "or-omb",
        state: "OR",
        agency_name: "Oregon Medical Board",
        system_name: "OR Medical Board Licensee Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://omb.oregon.gov/search",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Oregon Medical Board licenses both MDs and DOs as well as PAs.",
    },
];

const SD_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "sd-dss-ola",
        state: "SD",
        agency_name: "South Dakota Department of Social Services - Office of Licensing and Accreditation",
        system_name: "SD OLA Constituent Portal (Accredited SUD and CMHC Directory)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://olapublic.sd.gov/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Public portal for South Dakota accredited substance use disorder treatment agencies and community mental health centers.",
    },
    StateBoardDescriptor {
        id: "sd-bmoe",
        state: "SD",
        agency_name: "South Dakota Board of Medical and Osteopathic Examiners",
        system_name: "SD Board of Medical and Osteopathic Examiners Verification",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://www.sdbmoe.gov/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified South Dakota Board of Medical and Osteopathic Examiners licenses both MDs and DOs as well as PAs.",
    },
];

const TX_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "tx-hhsc-facilities",
        state: "TX",
        agency_name: "Texas Health and Human Services Commission - Health Care Regulation",
        system_name: "TX HHSC Health Care Facilities and Chemical Dependency Treatment Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.hhs.texas.gov/providers/health-care-facilities-regulation",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Licenses Texas chemical dependency treatment facilities (CDTFs), private psychiatric hospitals, and crisis stabilization units; supports Excel/CSV directory import.",
    },
    StateBoardDescriptor {
        id: "tx-tmb",
        state: "TX",
        agency_name: "Texas Medical Board",
        system_name: "TX Medical Board All Licenses (Socrata Open Data)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://profile.tmb.state.tx.us/Search.aspx",
        api_or_download_url: Some("https://data.texas.gov/resource/tm3v-pfq9.json"),
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataProvider {
            board: "Texas Medical Board",
            endpoint_url: "https://data.texas.gov/resource/tm3v-pfq9.json",
            where_clause: Some(
                "currently_licensed = 'Y' AND (license_type = 'Physician License' OR license_type = 'Physician Assistant License')",
            ),
            page_size: 1000,
            max_records: 150_000,
            verification_url_template: None,
            fields: ProviderFieldMap {
                license_number: &["license_number"],
                full_name: &[],
                first_name: &["first_name"],
                last_name: &["last_name"],
                credential_or_type: &["degree", "license_type"],
                board_name: &[],
                status: &["registration_status", "license_status"],
                city: &["practice_city"],
                zip: &["practice_zip"],
                expires_on: &["license_expiration_date"],
                disciplinary_flag: &["disciplinary_status"],
            },
        }),
        notes: "Verified live Socrata dataset tm3v-pfq9 on data.texas.gov. Unified Texas Medical Board licenses both MDs and DOs with explicit degree field (MD vs DO).",
    },
    StateBoardDescriptor {
        id: "tx-bhec",
        state: "TX",
        agency_name: "Texas Behavioral Health Executive Council",
        system_name: "TX BHEC Online License Verification (Psychologists, LCSW, LPC, LMFT)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://bhec.texas.gov/verify-a-license/",
        api_or_download_url: None,
        covers_licenses: BEHAVIORAL_CLINICIANS,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Verifies Texas psychologists, clinical social workers, professional counselors, and marriage and family therapists.",
    },
];

const UT_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ut-dhhs-facilities",
        state: "UT",
        agency_name: "Utah Department of Health and Human Services - Office of Licensing",
        system_name: "UT DHHS Licensed Health Care Facilities (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://dlbc.utah.gov/",
        api_or_download_url: Some(
            "https://services1.arcgis.com/99lidPhWCzftIe9K/arcgis/rest/services/LicensedHealthCareFacilities/FeatureServer/0/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Utah DHHS Office of Licensing (UGRC)",
            layer_query_url: "https://services1.arcgis.com/99lidPhWCzftIe9K/arcgis/rest/services/LicensedHealthCareFacilities/FeatureServer/0/query",
            where_clause: "1=1",
            page_size: 1000,
            max_records: 25_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["ID_NUMBER", "UT_NUMBER"],
                name: &["FACILITY_NAME"],
                dba_or_site_name: &[],
                license_type: &["LICENSE_TYPE"],
                status: &[],
                street: &["ADDRESS"],
                city: &["CITY"],
                zip: &["ZIP"],
                phone: &["TELEPHONE"],
                licensed_beds: &["CAPACITY"],
                expires_on: &["LICENSE_EXPIRATION_DATE"],
                detail_url: &[],
            },
        }),
        notes: "Verified live UGRC/AGRC ArcGIS FeatureServer layer for Utah licensed health care facilities; behavioral health human services programs also searchable at dlbc.utah.gov.",
    },
    StateBoardDescriptor {
        id: "ut-dopl-medicine",
        state: "UT",
        agency_name: "Utah Physicians and Surgeons Licensing Board (DOPL)",
        system_name: "UT DOPL Allopathic Physician License Lookup",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://secure.utah.gov/llv/search/index.html",
        api_or_download_url: None,
        covers_licenses: MD_ONLY,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate allopathic Physicians and Surgeons Licensing Board for MDs administered under Utah DOPL.",
    },
    StateBoardDescriptor {
        id: "ut-dopl-osteopathic",
        state: "UT",
        agency_name: "Utah Osteopathic Physician and Surgeon Licensing Board (DOPL)",
        system_name: "UT DOPL Osteopathic Physician License Lookup",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://secure.utah.gov/llv/search/index.html",
        api_or_download_url: None,
        covers_licenses: DO_ONLY,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate Utah Osteopathic Physician and Surgeon Licensing Board (Utah Code Title 58 Chapter 68) for DOs administered under Utah DOPL.",
    },
];

const WA_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "wa-doh-bha",
        state: "WA",
        agency_name: "Washington State Department of Health",
        system_name: "WA DOH Behavioral Health Agencies and Facility Credential Search",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://fortress.wa.gov/doh/providercredentialsearch/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Licenses Washington Behavioral Health Agencies (BHAs) for mental health and SUD; supports importing the DOH BHA directory export via --import-file.",
    },
    StateBoardDescriptor {
        id: "wa-doh-providers",
        state: "WA",
        agency_name: "Washington State Department of Health (Medical Commission, Osteopathic Board, Nursing & Behavioral Health)",
        system_name: "WA DOH Health Care Provider Credential Data (Socrata Open Data)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://doh.wa.gov/licenses-permits-and-certificates/provider-credential-or-facility-search",
        api_or_download_url: Some("https://data.wa.gov/resource/qxh8-f4bd.json"),
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataProvider {
            board: "Washington State Department of Health",
            endpoint_url: "https://data.wa.gov/resource/qxh8-f4bd.json",
            where_clause: Some("status = 'Active'"),
            page_size: 1000,
            max_records: 200_000,
            verification_url_template: None,
            fields: ProviderFieldMap {
                license_number: &["credentialnumber"],
                full_name: &[],
                first_name: &["firstname"],
                last_name: &["lastname"],
                credential_or_type: &["credentialtype"],
                board_name: &[],
                status: &["status"],
                city: &[],
                zip: &[],
                expires_on: &["expirationdate"],
                disciplinary_flag: &["actiontaken"],
            },
        }),
        notes: "Verified live Socrata dataset qxh8-f4bd updated daily by WA DOH. Strictly separates Washington Medical Commission (MD) and Board of Osteopathic Medicine and Surgery (DO) via credentialtype.",
    },
    StateBoardDescriptor {
        id: "wa-boms-osteopathic",
        state: "WA",
        agency_name: "Washington Board of Osteopathic Medicine and Surgery (DOH)",
        system_name: "WA Board of Osteopathic Medicine and Surgery Verification",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://doh.wa.gov/licenses-permits-and-certificates/professions-new-renew-or-update/osteopathic-physician-and-surgeon",
        api_or_download_url: None,
        covers_licenses: DO_ONLY,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Separate Washington Board of Osteopathic Medicine and Surgery for DOs (distinct from Washington Medical Commission for MDs).",
    },
];

const WI_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "wi-dhs-psych-hospitals",
        state: "WI",
        agency_name: "Wisconsin Department of Health Services - Division of Quality Assurance",
        system_name: "WI DHS Psychiatric Hospitals (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: "https://www.dhs.wisconsin.gov/guide/index.htm",
        api_or_download_url: Some(
            "https://dhsgis.wi.gov/server/rest/services/DHS_GIS/Facilities/MapServer/16/query",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Wisconsin DHS Division of Quality Assurance",
            layer_query_url: "https://dhsgis.wi.gov/server/rest/services/DHS_GIS/Facilities/MapServer/16/query",
            where_clause: "1=1",
            page_size: 1000,
            max_records: 5_000,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &[
                    "WI_LICENSE_NUM",
                    "ASPEN_FACILITY_ID",
                    "FACILITY_INTERNAL_ID",
                ],
                name: &["FACILITY_NAME"],
                dba_or_site_name: &[],
                license_type: &["SUB_TYPE_DESC", "TYPE_DESC"],
                status: &[],
                street: &["ADDRESS"],
                city: &["CITY"],
                zip: &["ZIP"],
                phone: &[],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live WI DHS Enterprise GIS MapServer layer 16 (Psychiatric Hospital); DHS 75 SUD and DHS 35 mental health provider directories are also exportable from dhs.wisconsin.gov.",
    },
    StateBoardDescriptor {
        id: "wi-dhs-dqa-bh",
        state: "WI",
        agency_name: "Wisconsin Department of Health Services - DQA and ForwardHealth",
        system_name: "WI DHS Substance Use (DHS 75) and Mental Health (DHS 35) Provider Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.dhs.wisconsin.gov/substanceuse/resources.htm",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Exports certified Wisconsin DHS 75 substance use and DHS 35 outpatient mental health treatment programs.",
    },
    StateBoardDescriptor {
        id: "wi-dsps",
        state: "WI",
        agency_name: "Wisconsin Department of Safety and Professional Services",
        system_name: "WI DSPS License Lookup",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://license.wi.gov/s/license",
        api_or_download_url: None,
        covers_licenses: ALL_CLINICAL,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified Wisconsin Medical Examining Board licenses both MDs and DOs under DSPS alongside nursing, psychology, and counseling boards.",
    },
];

const WY_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "wy-wdh-hls",
        state: "WY",
        agency_name: "Wyoming Department of Health - Healthcare Licensing and Surveys",
        system_name: "WY Healthcare Facility Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://health.wyo.gov/aging/hls/healthcare-facility-directory/",
        api_or_download_url: None,
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Publishes downloadable Wyoming psychiatric hospital, PRTF, and community mental health center directories.",
    },
    StateBoardDescriptor {
        id: "wy-bom",
        state: "WY",
        agency_name: "Wyoming Board of Medicine",
        system_name: "WY Board of Medicine License Verification",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://wyomedboard.wyo.gov/",
        api_or_download_url: None,
        covers_licenses: MD_DO_PA,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Unified Wyoming Board of Medicine licenses both MDs and DOs as well as PAs.",
    },
    StateBoardDescriptor {
        id: "wy-mental-health-board",
        state: "WY",
        agency_name: "Wyoming Mental Health Professions Licensing Board",
        system_name: "WY Mental Health Professions License Lookup",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://mentalhealth.wyo.gov/",
        api_or_download_url: None,
        covers_licenses: BEHAVIORAL_CLINICIANS,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Verifies Wyoming LCSW, LPC, LMFT, and LAT (addictions therapist) credentials.",
    },
    StateBoardDescriptor {
        id: "wy-wsbn",
        state: "WY",
        agency_name: "Wyoming State Board of Nursing",
        system_name: "WY Board of Nursing License Verification",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://wsbn.wyo.gov/",
        api_or_download_url: None,
        covers_licenses: NURSING_AND_APRN,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Verifies Wyoming APRN (including PMHNP), RN, and LPN licenses.",
    },
];

const MANIFESTS: &[StateLicensingManifest] = &[
    StateLicensingManifest {
        state: "AK",
        state_name: "Alaska",
        separate_osteopathic_board: false,
        systems: AK_SYSTEMS,
    },
    StateLicensingManifest {
        state: "HI",
        state_name: "Hawaii",
        separate_osteopathic_board: false,
        systems: HI_SYSTEMS,
    },
    StateLicensingManifest {
        state: "IA",
        state_name: "Iowa",
        separate_osteopathic_board: false,
        systems: IA_SYSTEMS,
    },
    StateLicensingManifest {
        state: "ID",
        state_name: "Idaho",
        separate_osteopathic_board: false,
        systems: ID_SYSTEMS,
    },
    StateLicensingManifest {
        state: "IL",
        state_name: "Illinois",
        separate_osteopathic_board: false,
        systems: IL_SYSTEMS,
    },
    StateLicensingManifest {
        state: "IN",
        state_name: "Indiana",
        separate_osteopathic_board: false,
        systems: IN_SYSTEMS,
    },
    StateLicensingManifest {
        state: "KS",
        state_name: "Kansas",
        separate_osteopathic_board: false,
        systems: KS_SYSTEMS,
    },
    StateLicensingManifest {
        state: "MI",
        state_name: "Michigan",
        separate_osteopathic_board: true,
        systems: MI_SYSTEMS,
    },
    StateLicensingManifest {
        state: "MN",
        state_name: "Minnesota",
        separate_osteopathic_board: false,
        systems: MN_SYSTEMS,
    },
    StateLicensingManifest {
        state: "MO",
        state_name: "Missouri",
        separate_osteopathic_board: false,
        systems: MO_SYSTEMS,
    },
    StateLicensingManifest {
        state: "MT",
        state_name: "Montana",
        separate_osteopathic_board: false,
        systems: MT_SYSTEMS,
    },
    StateLicensingManifest {
        state: "ND",
        state_name: "North Dakota",
        separate_osteopathic_board: false,
        systems: ND_SYSTEMS,
    },
    StateLicensingManifest {
        state: "NE",
        state_name: "Nebraska",
        separate_osteopathic_board: false,
        systems: NE_SYSTEMS,
    },
    StateLicensingManifest {
        state: "NM",
        state_name: "New Mexico",
        separate_osteopathic_board: false,
        systems: NM_SYSTEMS,
    },
    StateLicensingManifest {
        state: "NV",
        state_name: "Nevada",
        separate_osteopathic_board: true,
        systems: NV_SYSTEMS,
    },
    StateLicensingManifest {
        state: "OH",
        state_name: "Ohio",
        separate_osteopathic_board: false,
        systems: OH_SYSTEMS,
    },
    StateLicensingManifest {
        state: "OK",
        state_name: "Oklahoma",
        separate_osteopathic_board: true,
        systems: OK_SYSTEMS,
    },
    StateLicensingManifest {
        state: "OR",
        state_name: "Oregon",
        separate_osteopathic_board: false,
        systems: OR_SYSTEMS,
    },
    StateLicensingManifest {
        state: "SD",
        state_name: "South Dakota",
        separate_osteopathic_board: false,
        systems: SD_SYSTEMS,
    },
    StateLicensingManifest {
        state: "TX",
        state_name: "Texas",
        separate_osteopathic_board: false,
        systems: TX_SYSTEMS,
    },
    StateLicensingManifest {
        state: "UT",
        state_name: "Utah",
        separate_osteopathic_board: true,
        systems: UT_SYSTEMS,
    },
    StateLicensingManifest {
        state: "WA",
        state_name: "Washington",
        separate_osteopathic_board: true,
        systems: WA_SYSTEMS,
    },
    StateLicensingManifest {
        state: "WI",
        state_name: "Wisconsin",
        separate_osteopathic_board: false,
        systems: WI_SYSTEMS,
    },
    StateLicensingManifest {
        state: "WY",
        state_name: "Wyoming",
        separate_osteopathic_board: false,
        systems: WY_SYSTEMS,
    },
];

/// Returns verified licensing manifests for all 24 Western, Plains, and Midwest states
/// in alphabetical order (`AK` through `WY`).
pub fn manifests() -> &'static [StateLicensingManifest] {
    MANIFESTS
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const EXPECTED_STATES: &[&str] = &[
        "AK", "HI", "IA", "ID", "IL", "IN", "KS", "MI", "MN", "MO", "MT", "ND", "NE", "NM", "NV",
        "OH", "OK", "OR", "SD", "TX", "UT", "WA", "WI", "WY",
    ];

    #[test]
    fn all_24_states_present_unique_and_alphabetical() {
        let all = manifests();
        assert_eq!(all.len(), 24);
        let codes: Vec<&str> = all.iter().map(|m| m.state).collect();
        assert_eq!(codes, EXPECTED_STATES);
        let unique: HashSet<&str> = codes.iter().copied().collect();
        assert_eq!(unique.len(), 24);
    }

    #[test]
    fn every_state_has_facility_and_provider_systems_and_https_urls() {
        let mut system_ids = HashSet::new();
        for manifest in manifests() {
            assert!(
                !manifest.systems.is_empty(),
                "{} has no systems",
                manifest.state
            );
            let has_facility = manifest
                .systems
                .iter()
                .any(|s| matches!(s.scope, LicensingScope::Facility | LicensingScope::Both));
            let has_provider = manifest
                .systems
                .iter()
                .any(|s| matches!(s.scope, LicensingScope::Provider | LicensingScope::Both));
            assert!(
                has_facility,
                "{} is missing a facility licensing system",
                manifest.state
            );
            assert!(
                has_provider,
                "{} is missing a provider licensing system",
                manifest.state
            );

            for sys in manifest.systems {
                assert_eq!(sys.state, manifest.state);
                assert!(system_ids.insert(sys.id), "duplicate system id {}", sys.id);
                assert!(
                    sys.portal_url.starts_with("https://"),
                    "{} portal_url must start with https://",
                    sys.id
                );
                if let Some(api_url) = sys.api_or_download_url {
                    assert!(
                        api_url.starts_with("https://"),
                        "{} api_or_download_url must start with https://",
                        sys.id
                    );
                }
                if let Some(spec) = &sys.generic_endpoint {
                    match spec {
                        GenericEndpointSpec::SocrataFacility { endpoint_url, .. }
                        | GenericEndpointSpec::SocrataProvider { endpoint_url, .. } => {
                            assert!(endpoint_url.starts_with("https://"));
                        }
                        GenericEndpointSpec::ArcGisFacility {
                            layer_query_url, ..
                        } => {
                            assert!(layer_query_url.starts_with("https://"));
                        }
                        GenericEndpointSpec::CkanFacility {
                            datastore_search_url,
                            ..
                        } => {
                            assert!(datastore_search_url.starts_with("https://"));
                        }
                        GenericEndpointSpec::CsvFacility { csv_url, .. }
                        | GenericEndpointSpec::CsvProvider { csv_url, .. } => {
                            assert!(csv_url.starts_with("https://"));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn separate_osteopathic_boards_match_verified_states() {
        let do_states: Vec<&str> = manifests()
            .iter()
            .filter(|m| m.separate_osteopathic_board)
            .map(|m| m.state)
            .collect();
        assert_eq!(do_states, vec!["MI", "NV", "OK", "UT", "WA"]);
    }
}
