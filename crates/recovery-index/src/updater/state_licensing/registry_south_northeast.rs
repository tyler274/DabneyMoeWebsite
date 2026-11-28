//! State and territory licensing adapter registry for 27 South, Northeast, DC, and
//! U.S. Territory jurisdictions:
//! `AL, AR, CT, DC, DE, FL, GA, GU, KY, LA, MA, MD, ME, MP, MS, NC, NH, NJ, PA, PR,
//! RI, SC, TN, VA, VI, VT, WV`.
//!
//! Design notes:
//! - Every `GenericEndpointSpec` attached here was probed live (HTTP 200) with
//!   verified JSON field names from official Socrata or ArcGIS open-data portals.
//! - Portals gated by CAPTCHA, Turnstile, or login walls are modeled honestly as
//!   `BulkDownloadExport` or `PortalLookupOnly` with `generic_endpoint: None`.
//! - States with a separate Board of Osteopathic Medicine (`FL, ME, PA, TN, VT, WV`)
//!   set `separate_osteopathic_board: true` and separate MD vs DO board descriptors.

use crate::schema::StaffLicense;

use super::types::{
    FacilityFieldMap, GenericEndpointSpec, LicensingAccessMode, LicensingDatasetFormat,
    LicensingScope, ProviderFieldMap, StateBoardDescriptor, StateLicensingManifest,
};

const PHYSICIAN_COMPOSITE_LICENSES: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::DoPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::DoAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::DoPhysician,
    StaffLicense::Pa,
];

const MD_BOARD_LICENSES: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::Pa,
];

const DO_BOARD_LICENSES: &[StaffLicense] = &[
    StaffLicense::DoPsychiatrist,
    StaffLicense::DoAddiction,
    StaffLicense::DoPhysician,
];

const BH_THERAPIST_LICENSES: &[StaffLicense] = &[
    StaffLicense::PhdPsychologist,
    StaffLicense::PsydPsychologist,
    StaffLicense::Lcsw,
    StaffLicense::LpcLmhc,
    StaffLicense::Lmft,
    StaffLicense::AddictionCounselor,
    StaffLicense::PeerSupport,
];

const ALL_CLINICAL_LICENSES: &[StaffLicense] = StaffLicense::ALL;

static MANIFESTS: &[StateLicensingManifest] = &[
    // -------------------------------------------------------------------------
    // AL - Alabama
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "AL",
        state_name: "Alabama",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "al-adph-facilities",
                state: "AL",
                agency_name: "Alabama Department of Public Health (ADPH)",
                system_name: "ADPH Health Care Facilities Directory",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://www.alabamapublichealth.gov/facilities/",
                api_or_download_url: Some("https://www.alabamapublichealth.gov/healthcat/index.html"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "ADPH licenses hospitals, psychiatric facilities, and healthcare centers; DMH certifies community behavioral health and SUD providers.",
            },
            StateBoardDescriptor {
                id: "al-albme-providers",
                state: "AL",
                agency_name: "Alabama Board of Medical Examiners (ALBME)",
                system_name: "ALBME Licensee Verification Search",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://dashboard.albme.gov/verification/search.aspx",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating both MDs and DOs plus PAs; interactive verification portal.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // AR - Arkansas
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "AR",
        state_name: "Arkansas",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ar-dhs-daabhs-facilities",
                state: "AR",
                agency_name: "Arkansas Department of Human Services (DHS / DAABHS)",
                system_name: "Arkansas DHS Provider Licensing Portal",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://ardhslicensing.my.site.com/elicensing/s/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "DHS Division of Aging, Adult, and Behavioral Health Services oversees behavioral health and SUD facility licensure.",
            },
            StateBoardDescriptor {
                id: "ar-asmb-providers",
                state: "AR",
                agency_name: "Arkansas State Medical Board (ASMB)",
                system_name: "ASMB License Verification Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://armedicalboard.adh.arkansas.gov/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs; interactive lookup.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // CT - Connecticut (LIVE Socrata Open Data API for Facilities & Providers)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "CT",
        state_name: "Connecticut",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ct-dph-elicense-facilities",
                state: "CT",
                agency_name: "Connecticut Department of Public Health (DPH)",
                system_name: "Connecticut eLicense Behavioral Health & SUD Facilities (data.ct.gov)",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::SocrataJson,
                portal_url: "https://www.elicense.ct.gov/",
                api_or_download_url: Some("https://data.ct.gov/resource/ngch-56tr.json"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
                    agency: "CT DPH eLicense",
                    endpoint_url: "https://data.ct.gov/resource/ngch-56tr.json",
                    where_clause: Some(
                        "type != 'INDIVIDUAL' AND credential in('Substance Abuse','Psychiatric Outpatient Clinic','Mental Health Day Treatment','Mental Health Residential Living','Mental Health Community Residence','Hospital for Psychiatric Disabilities')",
                    ),
                    page_size: 1000,
                    max_records: 5000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &["fullcredentialcode", "credentialnumber", "credentialid"],
                        name: &["name", "businessname"],
                        dba_or_site_name: &["dba"],
                        license_type: &["credential"],
                        status: &["status"],
                        street: &["address"],
                        city: &["city"],
                        zip: &["zip"],
                        phone: &[],
                        licensed_beds: &[],
                        expires_on: &["expirationdate"],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live Socrata dataset ngch-56tr (updated daily from CT eLicense) covering DPH-licensed SUD and mental health facilities.",
            },
            StateBoardDescriptor {
                id: "ct-dph-elicense-providers",
                state: "CT",
                agency_name: "Connecticut Department of Public Health (DPH)",
                system_name: "Connecticut eLicense Clinical Practitioners (data.ct.gov)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::SocrataJson,
                portal_url: "https://www.elicense.ct.gov/",
                api_or_download_url: Some("https://data.ct.gov/resource/ngch-56tr.json"),
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::SocrataProvider {
                    board: "CT DPH eLicense",
                    endpoint_url: "https://data.ct.gov/resource/ngch-56tr.json",
                    where_clause: Some(
                        "type = 'INDIVIDUAL' AND credential in('Physician/Surgeon','Advanced Practice Registered Nurse','Physician Assistant','Psychologist','Licensed Clinical Social Worker','Professional Counselor','Marital and Family Therapist','Licensed Alcohol and Drug Counselor','Certified Alcohol and Drug Counselor')",
                    ),
                    page_size: 1000,
                    max_records: 25000,
                    verification_url_template: None,
                    fields: ProviderFieldMap {
                        license_number: &["fullcredentialcode", "credentialnumber", "credentialid"],
                        full_name: &["name"],
                        first_name: &[],
                        last_name: &[],
                        credential_or_type: &["credential"],
                        board_name: &[],
                        status: &["status"],
                        city: &["city"],
                        zip: &["zip"],
                        expires_on: &["expirationdate"],
                        disciplinary_flag: &[],
                    },
                }),
                notes: "Verified live Socrata dataset ngch-56tr covering CT DPH individual physician, APRN, PA, psychologist, LCSW, LPC, LMFT, and LADC/CADC licenses.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // DC - District of Columbia (LIVE ArcGIS Feature Layers for DBH & DC Health)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "DC",
        state_name: "District of Columbia",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "dc-dbh-sud-mhrs-facilities",
                state: "DC",
                agency_name: "DC Department of Behavioral Health (DBH)",
                system_name: "Open Data DC SUD and MHRS Certified Providers",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::ArcGisFeatureLayer,
                portal_url: "https://opendata.dc.gov/datasets/DCGIS::sud-and-mhrs-provider-contact-list",
                api_or_download_url: Some(
                    "https://maps2.dcgis.dc.gov/dcgis/rest/services/DCGIS_DATA/Public_Service_WebMercator/MapServer/66/query",
                ),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
                    agency: "DC DBH",
                    layer_query_url: "https://maps2.dcgis.dc.gov/dcgis/rest/services/DCGIS_DATA/Public_Service_WebMercator/MapServer/66/query",
                    where_clause: "1=1",
                    page_size: 500,
                    max_records: 5000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &["SITEMAPPINGID", "PROVIDERMAPPINGID", "OBJECTID"],
                        name: &["AGENCY"],
                        dba_or_site_name: &[],
                        license_type: &["SUD", "MHRS"],
                        status: &[],
                        street: &["ADDRESS"],
                        city: &[],
                        zip: &[],
                        phone: &[],
                        licensed_beds: &[],
                        expires_on: &[],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live ArcGIS MapServer layer 66 from Open Data DC listing DBH-certified SUD and MHRS behavioral health facilities.",
            },
            StateBoardDescriptor {
                id: "dc-health-opioid-treatment",
                state: "DC",
                agency_name: "DC Health (Department of Health)",
                system_name: "Open Data DC Opioid Dependence Treatment Facilities",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::ArcGisFeatureLayer,
                portal_url: "https://opendata.dc.gov/datasets/DCGIS::opioid-dependence-treatment-facilities",
                api_or_download_url: Some(
                    "https://maps2.dcgis.dc.gov/dcgis/rest/services/DCGIS_DATA/Health_WebMercator/MapServer/10/query",
                ),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
                    agency: "DC Health",
                    layer_query_url: "https://maps2.dcgis.dc.gov/dcgis/rest/services/DCGIS_DATA/Health_WebMercator/MapServer/10/query",
                    where_clause: "1=1",
                    page_size: 500,
                    max_records: 2000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &["GIS_ID", "OBJECTID"],
                        name: &["ORG"],
                        dba_or_site_name: &[],
                        license_type: &["PROGRAM"],
                        status: &[],
                        street: &["ADDRESS"],
                        city: &["CITY"],
                        zip: &["ZIPCODE"],
                        phone: &["PHONE"],
                        licensed_beds: &[],
                        expires_on: &[],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live ArcGIS MapServer layer 10 from Open Data DC listing opioid dependence treatment facilities.",
            },
            StateBoardDescriptor {
                id: "dc-health-providers",
                state: "DC",
                agency_name: "DC Health Health Regulation and Licensing Administration (HRLA)",
                system_name: "DC Health Professional License Search",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://dchealth.dc.gov/service/health-licensing-and-regulation",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite Board of Medicine (MDs and DOs) and allied health boards under DC Health HRLA.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // DE - Delaware
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "DE",
        state_name: "Delaware",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "de-dhss-dsamh-facilities",
                state: "DE",
                agency_name: "Delaware DHSS Division of Substance Abuse and Mental Health (DSAMH)",
                system_name: "DSAMH Licensed Behavioral Health & SUD Provider List",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://dhss.delaware.gov/dhss/dsamh/provlist.html",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Official DSAMH directory of licensed SUD treatment and mental health facilities in Delaware.",
            },
            StateBoardDescriptor {
                id: "de-delpros-providers",
                state: "DE",
                agency_name: "Delaware Division of Professional Regulation (DELPROS)",
                system_name: "DELPROS License Look-Up Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://delpros.delaware.gov/ohrc/wbh4/index.html#/license/search",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Unified DELPROS verification for Board of Medical Licensure and Discipline (MDs and DOs), nursing, psychology, social work, and counseling.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // FL - Florida (Separate Board of Osteopathic Medicine)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "FL",
        state_name: "Florida",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "fl-ahca-healthfinder-facilities",
                state: "FL",
                agency_name: "Florida Agency for Health Care Administration (AHCA)",
                system_name: "FloridaHealthFinder Facility Locator",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://www.floridahealthfinder.gov/",
                api_or_download_url: Some("https://quality.healthfinder.fl.gov/order-data.aspx"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "AHCA licenses psychiatric hospitals, residential treatment centers, and healthcare facilities with downloadable exports; FL DCF separately licenses SUD treatment providers.",
            },
            StateBoardDescriptor {
                id: "fl-doh-mqa-md-board",
                state: "FL",
                agency_name: "Florida Department of Health (MQA) - Board of Medicine",
                system_name: "FL DOH MQA License Verification (Allopathic MDs & Allied Health)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://mqa-internet.doh.state.fl.us/MQASearchServices/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "MQA bulk data download portal requires login; Board of Medicine licenses MDs separately from DOs, while MQA also hosts nursing, psychology, and behavioral health boards.",
            },
            StateBoardDescriptor {
                id: "fl-doh-mqa-do-board",
                state: "FL",
                agency_name: "Florida Department of Health (MQA) - Board of Osteopathic Medicine",
                system_name: "FL Board of Osteopathic Medicine Verification (DOs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://mqa-internet.doh.state.fl.us/MQASearchServices/HealthCareProviders",
                api_or_download_url: None,
                covers_licenses: DO_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate Florida Board of Osteopathic Medicine regulating DOs (OS license prefix) independently from the allopathic Board of Medicine (ME license prefix).",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // GA - Georgia
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "GA",
        state_name: "Georgia",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ga-dch-gamap2care-facilities",
                state: "GA",
                agency_name: "Georgia Department of Community Health (DCH / HFRD)",
                system_name: "GaMap2Care Healthcare Facility Search",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://gamap2care.dch.georgia.gov/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "DCH Healthcare Facility Regulation Division licenses drug abuse treatment programs, psychiatric hospitals, and clinics; DBHDD oversees community behavioral health providers.",
            },
            StateBoardDescriptor {
                id: "ga-gcmb-providers",
                state: "GA",
                agency_name: "Georgia Composite Medical Board (GCMB)",
                system_name: "GCMB License Verification Gateway",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://gcmb.mylicense.com/verification",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs; Secretary of State GOALS portal verifies nursing, counseling, and social work boards.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // GU - Guam
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "GU",
        state_name: "Guam",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "gu-gbhwc-facilities",
                state: "GU",
                agency_name: "Guam Behavioral Health and Wellness Center (GBHWC)",
                system_name: "GBHWC Behavioral Health & SUD Facility Directory",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://gbhwc.guam.gov/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Territorial behavioral health authority overseeing public mental health and SUD treatment services in Guam.",
            },
            StateBoardDescriptor {
                id: "gu-hplo-providers",
                state: "GU",
                agency_name: "Guam DPHSS Health Professional Licensing Office (HPLO)",
                system_name: "Guam Board of Medical Examiners & Allied Health Licensing",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://guamhplo.org/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "HPLO administers Guam Board of Medical Examiners (MDs and DOs), nursing, and allied behavioral health boards.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // KY - Kentucky
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "KY",
        state_name: "Kentucky",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ky-chfs-oig-facilities",
                state: "KY",
                agency_name: "Kentucky CHFS Office of Inspector General (OIG)",
                system_name: "CHFS OIG Health Care Facility Directories",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://chfs.ky.gov/agencies/os/oig/dhc/Pages/hcf.aspx",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "OIG Division of Health Care publishes directories of licensed behavioral health service organizations, AODEs, and psychiatric hospitals.",
            },
            StateBoardDescriptor {
                id: "ky-kbml-providers",
                state: "KY",
                agency_name: "Kentucky Board of Medical Licensure (KBML)",
                system_name: "KBML Online Verification & Physician Profile Search",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://kbml.ky.gov/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // LA - Louisiana
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "LA",
        state_name: "Louisiana",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "la-ldh-hss-facilities",
                state: "LA",
                agency_name: "Louisiana Department of Health (LDH) Health Standards Section",
                system_name: "LDH Health Standards Program Provider Directories",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://ldh.la.gov/page/info-hss",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "LDH Health Standards Section licenses Behavioral Health Service Providers (BHSP), psychiatric hospitals, and residential facilities.",
            },
            StateBoardDescriptor {
                id: "la-lsbme-providers",
                state: "LA",
                agency_name: "Louisiana State Board of Medical Examiners (LSBME)",
                system_name: "LSBME Online License Verification",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.lsbme.la.gov/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs in Louisiana.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // MA - Massachusetts
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "MA",
        state_name: "Massachusetts",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ma-dph-bsas-facilities",
                state: "MA",
                agency_name: "Massachusetts DPH Bureau of Substance Addiction Services (BSAS)",
                system_name: "BSAS Licensed Substance Use Treatment Program Directory",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.mass.gov/info-details/bureau-of-substance-addiction-services-licensing-system",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "BSAS licenses SUD treatment programs while DMH licenses inpatient psychiatric facilities.",
            },
            StateBoardDescriptor {
                id: "ma-dph-borm-providers",
                state: "MA",
                agency_name: "Massachusetts DPH Health Professions & Board of Registration in Medicine",
                system_name: "Mass.gov Check a Health Profession License",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.mass.gov/how-to/check-a-health-profession-license",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Board of Registration in Medicine regulates both MDs and DOs (FindMyDoctor); DPH Health Professions Licensing covers nursing, social work, psychology, and counseling.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // MD - Maryland (LIVE Socrata Open Data API for Psychiatric Hospitals)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "MD",
        state_name: "Maryland",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "md-ohcq-psych-hospitals",
                state: "MD",
                agency_name: "Maryland Department of Health Office of Health Care Quality (OHCQ)",
                system_name: "Maryland Open Data Psychiatric Hospitals (opendata.maryland.gov)",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::SocrataJson,
                portal_url: "https://health.maryland.gov/ohcq/Pages/Licensee-Directories.aspx",
                api_or_download_url: Some("https://opendata.maryland.gov/resource/spv3-bsy5.json"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
                    agency: "MD OHCQ",
                    endpoint_url: "https://opendata.maryland.gov/resource/spv3-bsy5.json",
                    where_clause: None,
                    page_size: 500,
                    max_records: 2000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &["ohcq_index_no", "objectid"],
                        name: &["facility_name"],
                        dba_or_site_name: &[],
                        license_type: &[],
                        status: &[],
                        street: &["facility_address"],
                        city: &["facility_city"],
                        zip: &["facility_zip"],
                        phone: &["facility_phone"],
                        licensed_beds: &["license_capacity"],
                        expires_on: &[],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live Socrata dataset spv3-bsy5 for Maryland psychiatric hospitals; BHA separately lists community behavioral health programs at health.maryland.gov/bha.",
            },
            StateBoardDescriptor {
                id: "md-bha-facilities",
                state: "MD",
                agency_name: "Maryland Behavioral Health Administration (BHA)",
                system_name: "Maryland BHA Licensed Behavioral Health Provider Finder",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://health.maryland.gov/bha/Pages/provider-finder.aspx",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Directory of BHA-licensed outpatient mental health, SUD, residential rehabilitation, and crisis stabilization programs.",
            },
            StateBoardDescriptor {
                id: "md-mbp-providers",
                state: "MD",
                agency_name: "Maryland Board of Physicians (MBP)",
                system_name: "MBP Practitioner Profile & Verification Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.mbp.state.md.us/bpqapp/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs in Maryland.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // ME - Maine (Separate Board of Osteopathic Licensure)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "ME",
        state_name: "Maine",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "me-dhhs-dlc-facilities",
                state: "ME",
                agency_name: "Maine DHHS Division of Licensing and Certification (DLC)",
                system_name: "Maine DHHS Licensed Facility & Behavioral Health Search",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://mainedhhs.org/DLC-ProviderSearch",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "DLC licenses behavioral health agencies, SUD treatment programs, and healthcare facilities.",
            },
            StateBoardDescriptor {
                id: "me-bolim-md-board",
                state: "ME",
                agency_name: "Maine Board of Licensure in Medicine (BOLIM)",
                system_name: "Maine Board of Licensure in Medicine Search (MDs & PAs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.maine.gov/md/licensing/search",
                api_or_download_url: None,
                covers_licenses: MD_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Allopathic medical board licensing MDs and allopathic PAs separately from Maine DOs.",
            },
            StateBoardDescriptor {
                id: "me-bol-do-board",
                state: "ME",
                agency_name: "Maine Board of Osteopathic Licensure",
                system_name: "Maine Board of Osteopathic Licensure Verification (DOs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.maine.gov/osteo/licensure",
                api_or_download_url: None,
                covers_licenses: DO_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate Maine Board of Osteopathic Licensure regulating DOs independently from the allopathic MD board.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // MP - Northern Mariana Islands (CNMI)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "MP",
        state_name: "Northern Mariana Islands",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "mp-chcc-facilities",
                state: "MP",
                agency_name: "Commonwealth Healthcare Corporation (CHCC)",
                system_name: "CHCC Behavioral Health & Community Guidance Services",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.chcc.health/behavioralhealth.php",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "CNMI public health and behavioral health authority overseeing hospital and community behavioral health services.",
            },
            StateBoardDescriptor {
                id: "mp-hcplb-providers",
                state: "MP",
                agency_name: "CNMI Health Care Professions Licensing Board (HCPLB)",
                system_name: "CNMI HCPLB Licensing & Roster Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.cnmilicensing.gov.mp/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Composite board regulating MDs, DOs, PAs, psychologists, counselors, and allied health professionals in CNMI.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // MS - Mississippi
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "MS",
        state_name: "Mississippi",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ms-dmh-facilities",
                state: "MS",
                agency_name: "Mississippi Department of Mental Health (DMH)",
                system_name: "DMH Certified Behavioral Health Provider Directory",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://mentalhealthms.com/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "DMH certifies community mental health centers and SUD treatment facilities; MSDH licenses hospitals.",
            },
            StateBoardDescriptor {
                id: "ms-msbml-providers",
                state: "MS",
                agency_name: "Mississippi State Board of Medical Licensure (MSBML)",
                system_name: "MSBML Licensee Lookup Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://gateway.msbml.ms.gov/common/queries/index.jsp",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs; DMH PLACE separately verifies state-credentialed mental health and addiction therapists.",
            },
            StateBoardDescriptor {
                id: "ms-dmh-place-providers",
                state: "MS",
                agency_name: "Mississippi DMH Professional Licensure and Certification (PLACE)",
                system_name: "DMH PLACE Behavioral Health Credential Verification",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.dmh.ms.gov/professional-licensure-and-certification/",
                api_or_download_url: None,
                covers_licenses: BH_THERAPIST_LICENSES,
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Verifies DMH-credentialed mental health, addiction, and peer support professionals.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // NC - North Carolina
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "NC",
        state_name: "North Carolina",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "nc-dhsr-mhlcs-facilities",
                state: "NC",
                agency_name: "North Carolina DHHS Division of Health Service Regulation (DHSR)",
                system_name: "DHSR Mental Health Licensure & Facility Listings",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://info.ncdhhs.gov/dhsr/mhlcs/publicrecords.html",
                api_or_download_url: Some("https://info.ncdhhs.gov/dhsr/genfac/geninfo.html"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "DHSR Mental Health Licensure and Certification Section licenses mental health, developmental disability, and SUD facilities in North Carolina.",
            },
            StateBoardDescriptor {
                id: "nc-ncmb-providers",
                state: "NC",
                agency_name: "North Carolina Medical Board (NCMB)",
                system_name: "NCMB Licensee Search Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://portal.ncmedboard.org/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Composite medical board regulating MDs, DOs, and PAs in North Carolina.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // NH - New Hampshire
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "NH",
        state_name: "New Hampshire",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "nh-dhhs-hfa-facilities",
                state: "NH",
                agency_name: "New Hampshire DHHS Health Facilities Administration",
                system_name: "NH DHHS Licensed Health & Residential Facility Directories",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::BulkDownloadExport,
                format: LicensingDatasetFormat::LocalCsvImport,
                portal_url: "https://www.dhhs.nh.gov/programs-services/health-care/health-facilities-administration",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Publishes licensed healthcare, psychiatric, and residential treatment facility directories under RSA 151.",
            },
            StateBoardDescriptor {
                id: "nh-oplc-providers",
                state: "NH",
                agency_name: "New Hampshire Office of Professional Licensure and Certification (OPLC)",
                system_name: "NH OPLC License Lookup Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://sos.nh.gov/online-services/licensing/license-lookup/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Unified OPLC verification covering the composite Board of Medicine (MDs and DOs), nursing, mental health practice, psychology, and alcohol/drug counselors.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // NJ - New Jersey (LIVE Socrata Open Data API for DOH Licensed Facilities)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "NJ",
        state_name: "New Jersey",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "nj-doh-facilities",
                state: "NJ",
                agency_name: "New Jersey Department of Health (NJDOH)",
                system_name: "NJ Open Data Licensed Healthcare & Psychiatric Facilities (data.nj.gov)",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::SocrataJson,
                portal_url: "https://healthapps.nj.gov/ohcq/hcs/hfdirSearch.aspx?dat=a",
                api_or_download_url: Some("https://data.nj.gov/resource/mrzk-zrvp.json"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
                    agency: "NJ DOH",
                    endpoint_url: "https://data.nj.gov/resource/mrzk-zrvp.json",
                    where_clause: Some(
                        "facility_type in('PSYCHIATRIC HOSPITAL','SPECIAL HOSPITAL - PSYCHIATRIC','GENERAL ACUTE CARE HOSPITAL','AMBULATORY CARE FACILITY','AMBULATORY CARE FACILITY - SATELLITE','FEDERALLY QUALIFIED HEALTH CENTERS')",
                    ),
                    page_size: 1000,
                    max_records: 5000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &["facid", "lic"],
                        name: &["licensed_name"],
                        dba_or_site_name: &["licensed_owner"],
                        license_type: &["facility_type"],
                        status: &[],
                        street: &["address"],
                        city: &["fac_city"],
                        zip: &["zip"],
                        phone: &["telephone"],
                        licensed_beds: &["hospbedtotal"],
                        expires_on: &["lic_expires"],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live Socrata dataset mrzk-zrvp on data.nj.gov covering NJDOH psychiatric hospitals, acute hospitals, and ambulatory care facilities; NJ DHS DMHAS oversees community SUD and mental health licensure.",
            },
            StateBoardDescriptor {
                id: "nj-dca-providers",
                state: "NJ",
                agency_name: "New Jersey Division of Consumer Affairs",
                system_name: "NJ Consumer Affairs Professional License Verification",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://newjersey.mylicense.com/verification/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Unified MyLicense verification for State Board of Medical Examiners (composite MD and DO board), nursing, psychology, social work, and addiction counselors.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // PA - Pennsylvania (Separate State Board of Osteopathic Medicine + LIVE Socrata DDAP)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "PA",
        state_name: "Pennsylvania",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "pa-ddap-facilities",
                state: "PA",
                agency_name: "Pennsylvania Department of Drug and Alcohol Programs (DDAP)",
                system_name: "PA Open Data DDAP-Licensed Drug & Alcohol Treatment Facilities (data.pa.gov)",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::OpenDataApi,
                format: LicensingDatasetFormat::SocrataJson,
                portal_url: "https://sais.health.pa.gov/commonpoc/content/publicweb/allfacilities.asp",
                api_or_download_url: Some("https://data.pa.gov/resource/eswt-bam9.json"),
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
                    agency: "PA DDAP",
                    endpoint_url: "https://data.pa.gov/resource/eswt-bam9.json",
                    where_clause: Some("type_of_facility = 'DDAP-Licensed Treatment Facility'"),
                    page_size: 1000,
                    max_records: 5000,
                    verification_url_template: None,
                    fields: FacilityFieldMap {
                        license_number: &[],
                        name: &["name_of_facility"],
                        dba_or_site_name: &[],
                        license_type: &["type_of_facility"],
                        status: &[],
                        street: &["street_address"],
                        city: &["county"],
                        zip: &[],
                        phone: &["phone_number"],
                        licensed_beds: &[],
                        expires_on: &[],
                        detail_url: &[],
                    },
                }),
                notes: "Verified live Socrata dataset eswt-bam9 on data.pa.gov for DDAP-licensed treatment facilities; SAIS portal provides live inspection status and PA DHS licenses mental health programs.",
            },
            StateBoardDescriptor {
                id: "pa-pals-md-and-allied",
                state: "PA",
                agency_name: "Pennsylvania Department of State (PALS) - State Board of Medicine & Allied Boards",
                system_name: "PALS License Verification (MDs & Allied Health)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.pals.pa.gov/#/page/search",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "PALS hosts the State Board of Medicine (MDs, MD license prefix) as well as nursing, psychology, social work, and professional counselor boards.",
            },
            StateBoardDescriptor {
                id: "pa-pals-do-board",
                state: "PA",
                agency_name: "Pennsylvania Department of State (PALS) - State Board of Osteopathic Medicine",
                system_name: "PA State Board of Osteopathic Medicine Verification (DOs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.pals.pa.gov/#/page/search",
                api_or_download_url: None,
                covers_licenses: DO_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate Pennsylvania State Board of Osteopathic Medicine regulating DOs (OS license prefix) independently from the State Board of Medicine.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // PR - Puerto Rico
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "PR",
        state_name: "Puerto Rico",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "pr-assmca-facilities",
                state: "PR",
                agency_name: "Puerto Rico ASSMCA (Administracion de Servicios de Salud Mental y Contra la Adiccion)",
                system_name: "ASSMCA Oficina de Organismos Reguladores (Facility Licensing)",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.assmca.pr.gov/licenciamiento",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "ASSMCA licenses mental health and substance use disorder treatment facilities in Puerto Rico.",
            },
            StateBoardDescriptor {
                id: "pr-orcps-providers",
                state: "PR",
                agency_name: "Puerto Rico Departamento de Salud (ORCPS / JLDM)",
                system_name: "ORCPS & Junta de Licenciamiento y Disciplina Medica Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://orcps.salud.pr.gov/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Junta de Licenciamiento y Disciplina Medica (composite MD/DO board) and ORCPS health professions registry.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // RI - Rhode Island
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "RI",
        state_name: "Rhode Island",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "ri-bhddh-facilities",
                state: "RI",
                agency_name: "Rhode Island Department of Behavioral Healthcare, Developmental Disabilities and Hospitals (BHDDH)",
                system_name: "BHDDH Licensed Behavioral Health Organizations",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://bhddh.ri.gov/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "BHDDH licenses behavioral healthcare organizations, community mental health centers, and SUD programs; RIDOH also lists healthcare facilities.",
            },
            StateBoardDescriptor {
                id: "ri-doh-providers",
                state: "RI",
                agency_name: "Rhode Island Department of Health (RIDOH)",
                system_name: "RIDOH Health Professionals & Facility License Verification",
                scope: LicensingScope::Both,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://healthri.force.com/hordlookup/s/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Unified RIDOH verification covering Board of Medical Licensure and Discipline (composite MD/DO), nursing, psychology, social work, and counseling.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // SC - South Carolina
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "SC",
        state_name: "South Carolina",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "sc-dph-facilities",
                state: "SC",
                agency_name: "South Carolina Department of Public Health (SC DPH, formerly DHEC)",
                system_name: "SC DPH Healthcare Quality Find a Facility Directory",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.arcgis.com/apps/instant/interactivelegend/index.html?appid=fd09063e5e034b329f97c43ea8e8c4c2",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "SC DPH Healthcare Quality licenses psychiatric hospitals, SUD treatment facilities, and healthcare facilities.",
            },
            StateBoardDescriptor {
                id: "sc-llr-medical-providers",
                state: "SC",
                agency_name: "South Carolina Department of Labor, Licensing and Regulation (LLR)",
                system_name: "SC LLR Board of Medical Examiners & Behavioral Health Lookup",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://llr.sc.gov/renewals/licensesearch/index.asp?board=medical",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "LLR hosts the composite Board of Medical Examiners (MDs and DOs) plus nursing, psychology, social work, and counseling boards.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // TN - Tennessee (Separate Board of Osteopathic Examination)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "TN",
        state_name: "Tennessee",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "tn-tdmhsas-facilities",
                state: "TN",
                agency_name: "Tennessee Department of Mental Health and Substance Abuse Services (TDMHSAS)",
                system_name: "TDMHSAS Facility Licensing & Inquiry Portal",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.tn.gov/behavioral-health/licensing.html",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "TDMHSAS licenses mental health, alcohol and drug abuse treatment, residential, and crisis facilities across Tennessee.",
            },
            StateBoardDescriptor {
                id: "tn-hrb-md-and-allied",
                state: "TN",
                agency_name: "Tennessee Department of Health - Board of Medical Examiners & Health Related Boards",
                system_name: "TN Health Related Boards Licensure Verification (MDs & Allied Health)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://apps.health.tn.gov/Licensure/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Verifies allopathic MDs (Board of Medical Examiners), nursing, psychology, social work, counseling, and alcohol/drug abuse counselors.",
            },
            StateBoardDescriptor {
                id: "tn-hrb-do-board",
                state: "TN",
                agency_name: "Tennessee Department of Health - Board of Osteopathic Examination",
                system_name: "TN Board of Osteopathic Examination Verification (DOs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://apps.health.tn.gov/Licensure/",
                api_or_download_url: None,
                covers_licenses: DO_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate Tennessee Board of Osteopathic Examination regulating DOs independently from the Board of Medical Examiners.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // VA - Virginia
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "VA",
        state_name: "Virginia",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "va-dbhds-facilities",
                state: "VA",
                agency_name: "Virginia Department of Behavioral Health and Developmental Services (DBHDS)",
                system_name: "DBHDS Licensed Provider Search System",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://vadbhdsv7prod.glsuite.us/GLSuiteWeb/Clients/vadbhds/Public/ProviderSearch/ProviderSearchSearch.aspx",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "DBHDS Office of Licensing regulates mental health, substance use, and developmental service providers in Virginia.",
            },
            StateBoardDescriptor {
                id: "va-dhp-providers",
                state: "VA",
                agency_name: "Virginia Department of Health Professions (DHP)",
                system_name: "Virginia DHP License Lookup Portal",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://license.dhp.virginia.gov/publiclookup/",
                api_or_download_url: None,
                covers_licenses: ALL_CLINICAL_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Unified DHP lookup covering the composite Board of Medicine (MDs and DOs), nursing, psychology, social work, and counseling.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // VI - U.S. Virgin Islands
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "VI",
        state_name: "U.S. Virgin Islands",
        separate_osteopathic_board: false,
        systems: &[
            StateBoardDescriptor {
                id: "vi-doh-facilities",
                state: "VI",
                agency_name: "U.S. Virgin Islands Department of Health (VIDOH)",
                system_name: "VIDOH Behavioral Health & Healthcare Facility Licensing",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://doh.vi.gov/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "VIDOH Division of Behavioral Health and Healthcare Facility Licensing oversees behavioral health and SUD facilities.",
            },
            StateBoardDescriptor {
                id: "vi-bme-providers",
                state: "VI",
                agency_name: "U.S. Virgin Islands Board of Medical Examiners",
                system_name: "USVI Board of Medical Examiners License Lookup",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.vimedicalboard.org/physicians-license-lookup/",
                api_or_download_url: None,
                covers_licenses: PHYSICIAN_COMPOSITE_LICENSES,
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "Composite medical board licensing MDs, DOs, and PAs in the U.S. Virgin Islands.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // VT - Vermont (Separate Board of Osteopathic Physicians and Surgeons)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "VT",
        state_name: "Vermont",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "vt-vdh-facilities",
                state: "VT",
                agency_name: "Vermont Department of Health (VDH)",
                system_name: "Vermont Health SUD & Behavioral Health Program Certification",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.healthvermont.gov/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "VDH Division of Substance Use Programs certifies SUD treatment providers; DMH oversees designated mental health agencies.",
            },
            StateBoardDescriptor {
                id: "vt-vdh-md-board",
                state: "VT",
                agency_name: "Vermont Board of Medical Practice (Department of Health)",
                system_name: "Vermont Board of Medical Practice Lookup (MDs & PAs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.healthvermont.gov/systems/board-medical-practice",
                api_or_download_url: None,
                covers_licenses: MD_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Regulates allopathic MDs and PAs under the Vermont Department of Health separately from OPR osteopathic physicians.",
            },
            StateBoardDescriptor {
                id: "vt-opr-do-and-allied",
                state: "VT",
                agency_name: "Vermont Secretary of State Office of Professional Regulation (OPR) - Board of Osteopathic Physicians and Surgeons",
                system_name: "Vermont OPR Find a Professional (DOs & Behavioral Health)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://sos.vermont.gov/opr/find-a-professional/",
                api_or_download_url: None,
                covers_licenses: &[
                    StaffLicense::DoPsychiatrist,
                    StaffLicense::DoAddiction,
                    StaffLicense::DoPhysician,
                    StaffLicense::Pmhnp,
                    StaffLicense::Np,
                    StaffLicense::PhdPsychologist,
                    StaffLicense::PsydPsychologist,
                    StaffLicense::Lcsw,
                    StaffLicense::LpcLmhc,
                    StaffLicense::Lmft,
                    StaffLicense::Rn,
                    StaffLicense::Lpn,
                    StaffLicense::AddictionCounselor,
                ],
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate Board of Osteopathic Physicians and Surgeons under Vermont OPR regulates DOs independently from the VDH Board of Medical Practice; OPR also licenses nursing, psychology, social work, counseling, and LADC.",
            },
        ],
    },
    // -------------------------------------------------------------------------
    // WV - West Virginia (Separate Board of Osteopathic Medicine)
    // -------------------------------------------------------------------------
    StateLicensingManifest {
        state: "WV",
        state_name: "West Virginia",
        separate_osteopathic_board: true,
        systems: &[
            StateBoardDescriptor {
                id: "wv-ohflac-facilities",
                state: "WV",
                agency_name: "West Virginia Office of Health Facility Licensure and Certification (OHFLAC)",
                system_name: "WV OHFLAC Health Care & Behavioral Health Facility Search",
                scope: LicensingScope::Facility,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://ohflac.wvdhhr.org/Apps/FacilitySearch/",
                api_or_download_url: None,
                covers_licenses: &[],
                captcha_or_login_gated: false,
                generic_endpoint: None,
                notes: "OHFLAC licenses behavioral health centers, opioid treatment programs, residential facilities, and hospitals in West Virginia.",
            },
            StateBoardDescriptor {
                id: "wv-wvbom-md-board",
                state: "WV",
                agency_name: "West Virginia Board of Medicine (WVBOM)",
                system_name: "WVBOM Licensee Search (MDs & Allopathic PAs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://wvbom.wv.gov/",
                api_or_download_url: None,
                covers_licenses: MD_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Regulates allopathic MDs and PAs separately from the West Virginia Board of Osteopathic Medicine.",
            },
            StateBoardDescriptor {
                id: "wv-wvbdosteo-do-board",
                state: "WV",
                agency_name: "West Virginia Board of Osteopathic Medicine",
                system_name: "WV Board of Osteopathic Medicine Licensee Search (DOs)",
                scope: LicensingScope::Provider,
                access_mode: LicensingAccessMode::PortalLookupOnly,
                format: LicensingDatasetFormat::PortalWebOnly,
                portal_url: "https://www.wvbdosteo.org/",
                api_or_download_url: None,
                covers_licenses: DO_BOARD_LICENSES,
                captcha_or_login_gated: true,
                generic_endpoint: None,
                notes: "Separate West Virginia Board of Osteopathic Medicine regulating DOs and osteopathic PAs independently from WVBOM.",
            },
        ],
    },
];

/// Returns all 27 verified South, Northeast, DC, and Territory licensing manifests
/// in alphabetical order (`AL` through `WV`).
pub fn manifests() -> &'static [StateLicensingManifest] {
    MANIFESTS
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const EXPECTED_STATES: &[&str] = &[
        "AL", "AR", "CT", "DC", "DE", "FL", "GA", "GU", "KY", "LA", "MA", "MD", "ME", "MP", "MS",
        "NC", "NH", "NJ", "PA", "PR", "RI", "SC", "TN", "VA", "VI", "VT", "WV",
    ];

    const EXPECTED_SEPARATE_DO_STATES: &[&str] = &["FL", "ME", "PA", "TN", "VT", "WV"];

    #[test]
    fn covers_all_27_jurisdictions_uniquely_and_alphabetically() {
        let items = manifests();
        assert_eq!(items.len(), 27, "expected 27 jurisdictions");

        let codes: Vec<&str> = items.iter().map(|m| m.state).collect();
        assert_eq!(codes, EXPECTED_STATES);

        let unique_codes: BTreeSet<&str> = codes.iter().copied().collect();
        assert_eq!(unique_codes.len(), 27);
    }

    #[test]
    fn every_jurisdiction_has_facility_and_provider_systems_with_https_urls() {
        let mut system_ids = BTreeSet::new();

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

            assert!(has_facility, "{} missing facility system", manifest.state);
            assert!(has_provider, "{} missing provider system", manifest.state);

            for sys in manifest.systems {
                assert_eq!(sys.state, manifest.state);
                assert!(system_ids.insert(sys.id), "duplicate system id {}", sys.id);
                assert!(
                    sys.portal_url.starts_with("https://"),
                    "{} portal_url must be https://",
                    sys.id
                );
                if let Some(url) = sys.api_or_download_url {
                    assert!(
                        url.starts_with("https://"),
                        "{} api_or_download_url must be https://",
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
    fn separate_osteopathic_board_flags_match_verified_states() {
        let actual_do: Vec<&str> = manifests()
            .iter()
            .filter(|m| m.separate_osteopathic_board)
            .map(|m| m.state)
            .collect();
        assert_eq!(actual_do, EXPECTED_SEPARATE_DO_STATES);
    }
}
