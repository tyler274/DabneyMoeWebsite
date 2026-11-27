//! Typed domain schema for recovery & mental-health facility shards.
//!
//! Backwards-compatible with base SAMHSA FindTreatment.gov shards while adding:
//! - Non-care living features & amenities (pools, hot tubs, rooms, pets, etc.)
//! - Food & dining situation + dietary accommodations
//! - Electronic device allowance policy
//! - Smoking & vaping policies and languages
//! - Exact clinical staff license types (strictly distinguishing MD vs DO,
//!   Psychiatrist vs PMHNP/NP vs PA, PhD vs PsyD, LCSW vs LPC vs LMFT, etc.)
//! - Per-facility provenance metadata (`samhsa`, `nppes`, `website`, `verified`).

use serde::{Deserialize, Serialize};

/// A facility's primary care category (`typeFacility` in SAMHSA).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CareType {
    #[serde(rename = "mh")]
    MentalHealth,
    #[serde(rename = "su")]
    SubstanceUse,
}

impl CareType {
    pub const ALL: &'static [Self] = &[Self::MentalHealth, Self::SubstanceUse];

    pub const fn code(self) -> &'static str {
        match self {
            Self::MentalHealth => "mh",
            Self::SubstanceUse => "su",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MentalHealth => "Mental health",
            Self::SubstanceUse => "Substance use",
        }
    }

    pub fn from_samhsa_type(raw: &str) -> Option<Self> {
        match raw.trim() {
            "MH" => Some(Self::MentalHealth),
            "SA" => Some(Self::SubstanceUse),
            _ => None,
        }
    }
}

/// Service setting offered by a facility (SAMHSA `SET` codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Setting {
    #[serde(rename = "op")]
    Outpatient,
    #[serde(rename = "op-regular")]
    RegularOutpatient,
    #[serde(rename = "op-intensive")]
    IntensiveOutpatient,
    #[serde(rename = "op-day")]
    OutpatientDayTreatment,
    #[serde(rename = "op-detox")]
    OutpatientDetox,
    #[serde(rename = "op-medication")]
    OutpatientMedication,
    #[serde(rename = "partial")]
    PartialHospitalization,
    #[serde(rename = "res")]
    Residential,
    #[serde(rename = "res-short")]
    ShortTermResidential,
    #[serde(rename = "res-long")]
    LongTermResidential,
    #[serde(rename = "res-detox")]
    ResidentialDetox,
    #[serde(rename = "hosp")]
    HospitalInpatient,
    #[serde(rename = "hosp-treatment")]
    HospitalInpatientTreatment,
    #[serde(rename = "hosp-detox")]
    HospitalInpatientDetox,
}

impl Setting {
    pub const ALL: &'static [Self] = &[
        Self::Outpatient,
        Self::RegularOutpatient,
        Self::IntensiveOutpatient,
        Self::OutpatientDayTreatment,
        Self::OutpatientDetox,
        Self::OutpatientMedication,
        Self::PartialHospitalization,
        Self::Residential,
        Self::ShortTermResidential,
        Self::LongTermResidential,
        Self::ResidentialDetox,
        Self::HospitalInpatient,
        Self::HospitalInpatientTreatment,
        Self::HospitalInpatientDetox,
    ];

    pub const fn is_residential_or_inpatient(self) -> bool {
        matches!(
            self,
            Self::Residential
                | Self::ShortTermResidential
                | Self::LongTermResidential
                | Self::ResidentialDetox
                | Self::HospitalInpatient
                | Self::HospitalInpatientTreatment
                | Self::HospitalInpatientDetox
        )
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Outpatient => "Outpatient",
            Self::RegularOutpatient => "Regular outpatient",
            Self::IntensiveOutpatient => "Intensive outpatient (IOP)",
            Self::OutpatientDayTreatment => "Outpatient day / PHP",
            Self::OutpatientDetox => "Outpatient detox",
            Self::OutpatientMedication => "Medication-assisted (MAT)",
            Self::PartialHospitalization => "Partial hospitalization (PHP)",
            Self::Residential => "Residential (24-hr)",
            Self::ShortTermResidential => "Short-term residential",
            Self::LongTermResidential => "Long-term residential",
            Self::ResidentialDetox => "Residential detox",
            Self::HospitalInpatient => "Hospital inpatient",
            Self::HospitalInpatientTreatment => "Hospital inpatient treatment",
            Self::HospitalInpatientDetox => "Hospital inpatient detox",
        }
    }

    pub fn from_samhsa_label(label: &str) -> Option<Self> {
        match label.trim() {
            "Outpatient" => Some(Self::Outpatient),
            "Regular outpatient treatment" => Some(Self::RegularOutpatient),
            "Intensive outpatient treatment" => Some(Self::IntensiveOutpatient),
            "Outpatient day treatment or partial hospitalization" => {
                Some(Self::OutpatientDayTreatment)
            }
            "Outpatient detoxification" => Some(Self::OutpatientDetox),
            "Outpatient methadone/buprenorphine or naltrexone treatment" => {
                Some(Self::OutpatientMedication)
            }
            "Partial hospitalization/day treatment" => Some(Self::PartialHospitalization),
            "Residential/24-hour residential" => Some(Self::Residential),
            "Short-term residential" => Some(Self::ShortTermResidential),
            "Long-term residential" => Some(Self::LongTermResidential),
            "Residential detoxification" => Some(Self::ResidentialDetox),
            "Hospital inpatient/24-hour hospital inpatient" => Some(Self::HospitalInpatient),
            "Hospital inpatient treatment" => Some(Self::HospitalInpatientTreatment),
            "Hospital inpatient detoxification" => Some(Self::HospitalInpatientDetox),
            _ => None,
        }
    }
}

/// Payment, insurance, or financial assistance accepted (SAMHSA `PAY` + `PYAS` codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Payment {
    #[serde(rename = "medicaid")]
    Medicaid,
    #[serde(rename = "medicare")]
    Medicare,
    #[serde(rename = "private")]
    PrivateInsurance,
    #[serde(rename = "self")]
    SelfPay,
    #[serde(rename = "sliding")]
    SlidingFee,
    #[serde(rename = "assistance")]
    PaymentAssistance,
    #[serde(rename = "free")]
    NoPayment,
    #[serde(rename = "military")]
    MilitaryInsurance,
    #[serde(rename = "va")]
    VaFunds,
    #[serde(rename = "itu")]
    TribalFunds,
    #[serde(rename = "state-insurance")]
    StateHealthInsurance,
    #[serde(rename = "samhsa")]
    SamhsaFunding,
    #[serde(rename = "federal-su")]
    FederalSubstanceUseFunding,
    #[serde(rename = "federal-grants")]
    FederalGrants,
    #[serde(rename = "cmhbg")]
    MentalHealthBlockGrants,
    #[serde(rename = "csbg")]
    ServiceBlockGrants,
    #[serde(rename = "state-mh")]
    StateMentalHealthFunds,
    #[serde(rename = "state-welfare")]
    StateWelfareFunds,
    #[serde(rename = "state-corrections")]
    StateCorrectionsFunds,
    #[serde(rename = "state-education")]
    StateEducationFunds,
    #[serde(rename = "state-other")]
    OtherStateFunds,
    #[serde(rename = "local")]
    LocalFunds,
    #[serde(rename = "foundation")]
    FoundationFunds,
}

impl Payment {
    /// Common patient-facing payment filters surfaced first.
    pub const PRIMARY: &'static [Self] = &[
        Self::Medicaid,
        Self::Medicare,
        Self::PrivateInsurance,
        Self::SelfPay,
        Self::SlidingFee,
        Self::PaymentAssistance,
        Self::NoPayment,
        Self::MilitaryInsurance,
        Self::VaFunds,
        Self::TribalFunds,
        Self::StateHealthInsurance,
    ];

    pub const ALL: &'static [Self] = &[
        Self::Medicaid,
        Self::Medicare,
        Self::PrivateInsurance,
        Self::SelfPay,
        Self::SlidingFee,
        Self::PaymentAssistance,
        Self::NoPayment,
        Self::MilitaryInsurance,
        Self::VaFunds,
        Self::TribalFunds,
        Self::StateHealthInsurance,
        Self::SamhsaFunding,
        Self::FederalSubstanceUseFunding,
        Self::FederalGrants,
        Self::MentalHealthBlockGrants,
        Self::ServiceBlockGrants,
        Self::StateMentalHealthFunds,
        Self::StateWelfareFunds,
        Self::StateCorrectionsFunds,
        Self::StateEducationFunds,
        Self::OtherStateFunds,
        Self::LocalFunds,
        Self::FoundationFunds,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Medicaid => "Medicaid",
            Self::Medicare => "Medicare",
            Self::PrivateInsurance => "Private insurance",
            Self::SelfPay => "Cash / self-pay",
            Self::SlidingFee => "Sliding fee scale",
            Self::PaymentAssistance => "Payment assistance",
            Self::NoPayment => "No payment required",
            Self::MilitaryInsurance => "TRICARE / Military",
            Self::VaFunds => "VA funds",
            Self::TribalFunds => "IHS / Tribal / Urban",
            Self::StateHealthInsurance => "State health plan",
            Self::SamhsaFunding => "SAMHSA block grants",
            Self::FederalSubstanceUseFunding => "Federal SU funding",
            Self::FederalGrants => "Federal grants",
            Self::MentalHealthBlockGrants => "Mental health block grant",
            Self::ServiceBlockGrants => "Community service grant",
            Self::StateMentalHealthFunds => "State mental health funds",
            Self::StateWelfareFunds => "State child/family funds",
            Self::StateCorrectionsFunds => "State corrections funds",
            Self::StateEducationFunds => "State education funds",
            Self::OtherStateFunds => "Other state funds",
            Self::LocalFunds => "County / local funds",
            Self::FoundationFunds => "Community foundation",
        }
    }

    pub fn from_samhsa_label(label: &str) -> Option<Self> {
        match label.trim() {
            "Cash or self-payment" => Some(Self::SelfPay),
            "Private health insurance" => Some(Self::PrivateInsurance),
            "Medicaid" => Some(Self::Medicaid),
            "Medicare" => Some(Self::Medicare),
            "State-financed health insurance plan other than Medicaid" => {
                Some(Self::StateHealthInsurance)
            }
            "Federal military insurance (e.g., TRICARE)" => Some(Self::MilitaryInsurance),
            "U.S. Department of VA funds" => Some(Self::VaFunds),
            "IHS/Tribal/Urban (ITU) funds" => Some(Self::TribalFunds),
            "No payment accepted" => Some(Self::NoPayment),
            "Sliding fee scale (fee is based on income and other factors)" => {
                Some(Self::SlidingFee)
            }
            "Payment assistance (check with facility for details)" => Some(Self::PaymentAssistance),
            "SAMHSA funding/block grants" => Some(Self::SamhsaFunding),
            "Federal, or any government funding for substance use treatment programs" => {
                Some(Self::FederalSubstanceUseFunding)
            }
            "Federal Grants" => Some(Self::FederalGrants),
            "Community Mental Health Block Grants" => Some(Self::MentalHealthBlockGrants),
            "Community Service Block Grants" => Some(Self::ServiceBlockGrants),
            "State mental health agency (or equivalent) funds" => {
                Some(Self::StateMentalHealthFunds)
            }
            "State welfare or child and family services funds" => Some(Self::StateWelfareFunds),
            "State corrections or juvenile justice funds" => Some(Self::StateCorrectionsFunds),
            "State education agency funds" => Some(Self::StateEducationFunds),
            "Other State funds" => Some(Self::OtherStateFunds),
            "County or local government funds" => Some(Self::LocalFunds),
            "Private or Community foundation" => Some(Self::FoundationFunds),
            _ => None,
        }
    }
}

/// Special programs / populations served (SAMHSA `SG` codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Group {
    #[serde(rename = "adolescents")]
    Adolescents,
    #[serde(rename = "young-adults")]
    YoungAdults,
    #[serde(rename = "men")]
    AdultMen,
    #[serde(rename = "women")]
    AdultWomen,
    #[serde(rename = "seniors")]
    Seniors,
    #[serde(rename = "pregnant")]
    PregnantPostpartum,
    #[serde(rename = "veterans")]
    Veterans,
    #[serde(rename = "active-duty")]
    ActiveDutyMilitary,
    #[serde(rename = "military-families")]
    MilitaryFamilies,
    #[serde(rename = "justice")]
    CriminalJustice,
    #[serde(rename = "co-occurring")]
    CoOccurring,
    #[serde(rename = "co-occurring-pain")]
    CoOccurringPain,
    #[serde(rename = "hiv")]
    HivAids,
    #[serde(rename = "trauma")]
    Trauma,
    #[serde(rename = "sexual-abuse")]
    SexualAbuse,
    #[serde(rename = "ipv")]
    IntimatePartnerViolence,
    #[serde(rename = "smi")]
    SeriousMentalIllness,
    #[serde(rename = "sed")]
    SeriousEmotionalDisturbance,
    #[serde(rename = "first-episode-psychosis")]
    FirstEpisodePsychosis,
    #[serde(rename = "ptsd")]
    Ptsd,
    #[serde(rename = "tbi")]
    TraumaticBrainInjury,
    #[serde(rename = "dementia")]
    Dementia,
    #[serde(rename = "eating-disorders")]
    EatingDisorders,
}

impl Group {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Adolescents => "Adolescents",
            Self::YoungAdults => "Young adults",
            Self::AdultMen => "Adult men",
            Self::AdultWomen => "Adult women",
            Self::Seniors => "Older adults",
            Self::PregnantPostpartum => "Pregnant / postpartum",
            Self::Veterans => "Veterans",
            Self::ActiveDutyMilitary => "Active duty military",
            Self::MilitaryFamilies => "Military families",
            Self::CriminalJustice => "Justice-involved",
            Self::CoOccurring => "Co-occurring MH & SU",
            Self::CoOccurringPain => "Co-occurring pain & SU",
            Self::HivAids => "HIV / AIDS support",
            Self::Trauma => "Trauma-informed",
            Self::SexualAbuse => "Sexual abuse recovery",
            Self::IntimatePartnerViolence => "Domestic violence / IPV",
            Self::SeriousMentalIllness => "Serious mental illness (SMI)",
            Self::SeriousEmotionalDisturbance => "Youth SED",
            Self::FirstEpisodePsychosis => "First-episode psychosis",
            Self::Ptsd => "PTSD",
            Self::TraumaticBrainInjury => "Traumatic brain injury",
            Self::Dementia => "Alzheimer's / dementia",
            Self::EatingDisorders => "Eating disorders",
        }
    }

    pub fn from_samhsa_label(label: &str) -> Option<Self> {
        match label.trim() {
            "Adolescents" => Some(Self::Adolescents),
            "Young adults" => Some(Self::YoungAdults),
            "Adult men" => Some(Self::AdultMen),
            "Adult women" => Some(Self::AdultWomen),
            "Seniors or older adults" => Some(Self::Seniors),
            "Pregnant/postpartum women" => Some(Self::PregnantPostpartum),
            "Veterans" => Some(Self::Veterans),
            "Active duty military" => Some(Self::ActiveDutyMilitary),
            "Members of military families" => Some(Self::MilitaryFamilies),
            "Criminal justice (other than DUI/DWI)/Forensic clients" => Some(Self::CriminalJustice),
            "Clients with co-occurring mental and substance use disorders" => {
                Some(Self::CoOccurring)
            }
            "Clients with co-occurring pain and substance use disorders" => {
                Some(Self::CoOccurringPain)
            }
            "Clients with HIV or AIDS" => Some(Self::HivAids),
            "Clients who have experienced trauma" => Some(Self::Trauma),
            "Clients who have experienced sexual abuse" => Some(Self::SexualAbuse),
            "Clients who have experienced intimate partner violence, domestic violence" => {
                Some(Self::IntimatePartnerViolence)
            }
            "Persons  18 and older with serious mental illness (SMI)"
            | "Persons 18 and older with serious mental illness (SMI)" => {
                Some(Self::SeriousMentalIllness)
            }
            "Children/adolescents with serious emotional disturbance (SED)" => {
                Some(Self::SeriousEmotionalDisturbance)
            }
            "Persons experiencing first-episode psychosis" => Some(Self::FirstEpisodePsychosis),
            "Persons with post-traumatic stress disorder (PTSD)" => Some(Self::Ptsd),
            "Persons with traumatic brain injury (TBI)" => Some(Self::TraumaticBrainInjury),
            "Persons with Alzheimer's or dementia" => Some(Self::Dementia),
            "Persons with eating disorders" => Some(Self::EatingDisorders),
            _ => None,
        }
    }
}

/// Logical grouping for non-care facility amenities & living features.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AmenityCategory {
    WaterAndWellness,
    RoomsAndLiving,
    CampusAndRecreation,
    PoliciesAndSupport,
}

impl AmenityCategory {
    pub const ALL: &'static [Self] = &[
        Self::WaterAndWellness,
        Self::RoomsAndLiving,
        Self::CampusAndRecreation,
        Self::PoliciesAndSupport,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::WaterAndWellness => "Water & Wellness",
            Self::RoomsAndLiving => "Rooms & Living",
            Self::CampusAndRecreation => "Campus & Recreation",
            Self::PoliciesAndSupport => "Visits & Practical Support",
        }
    }
}

/// Non-care living features, amenities, and practical facilities ("everything aside from the care").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Amenity {
    // Water & Wellness
    #[serde(rename = "pool")]
    Pool,
    #[serde(rename = "hot-tub")]
    HotTub,
    #[serde(rename = "sauna")]
    Sauna,
    #[serde(rename = "gym")]
    Gym,
    #[serde(rename = "yoga")]
    YogaStudio,
    #[serde(rename = "spa")]
    SpaMassage,

    // Rooms & Living
    #[serde(rename = "private-rooms")]
    PrivateRooms,
    #[serde(rename = "shared-rooms")]
    SharedRooms,
    #[serde(rename = "pet-friendly")]
    PetFriendly,
    #[serde(rename = "laundry")]
    Laundry,
    #[serde(rename = "wifi")]
    Wifi,
    #[serde(rename = "workspace")]
    Workspace,

    // Campus & Recreation
    #[serde(rename = "outdoor-grounds")]
    OutdoorGrounds,
    #[serde(rename = "recreation-courts")]
    RecreationCourts,
    #[serde(rename = "equine-therapy")]
    EquineTherapy,
    #[serde(rename = "art-music-studio")]
    ArtMusicStudio,
    #[serde(rename = "lounge-game-room")]
    LoungeGameRoom,

    // Visits & Practical Support
    #[serde(rename = "family-visitation")]
    FamilyVisitation,
    #[serde(rename = "offsite-outings")]
    OffsiteOutings,
    #[serde(rename = "transportation")]
    Transportation,
    #[serde(rename = "housing-assistance")]
    HousingAssistance,
    #[serde(rename = "childcare")]
    Childcare,
}

impl Amenity {
    pub const ALL: &'static [Self] = &[
        Self::Pool,
        Self::HotTub,
        Self::Sauna,
        Self::Gym,
        Self::YogaStudio,
        Self::SpaMassage,
        Self::PrivateRooms,
        Self::SharedRooms,
        Self::PetFriendly,
        Self::Laundry,
        Self::Wifi,
        Self::Workspace,
        Self::OutdoorGrounds,
        Self::RecreationCourts,
        Self::EquineTherapy,
        Self::ArtMusicStudio,
        Self::LoungeGameRoom,
        Self::FamilyVisitation,
        Self::OffsiteOutings,
        Self::Transportation,
        Self::HousingAssistance,
        Self::Childcare,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Pool => "pool",
            Self::HotTub => "hot-tub",
            Self::Sauna => "sauna",
            Self::Gym => "gym",
            Self::YogaStudio => "yoga",
            Self::SpaMassage => "spa",
            Self::PrivateRooms => "private-rooms",
            Self::SharedRooms => "shared-rooms",
            Self::PetFriendly => "pet-friendly",
            Self::Laundry => "laundry",
            Self::Wifi => "wifi",
            Self::Workspace => "workspace",
            Self::OutdoorGrounds => "outdoor-grounds",
            Self::RecreationCourts => "recreation-courts",
            Self::EquineTherapy => "equine-therapy",
            Self::ArtMusicStudio => "art-music-studio",
            Self::LoungeGameRoom => "lounge-game-room",
            Self::FamilyVisitation => "family-visitation",
            Self::OffsiteOutings => "offsite-outings",
            Self::Transportation => "transportation",
            Self::HousingAssistance => "housing-assistance",
            Self::Childcare => "childcare",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Pool => "Swimming pool",
            Self::HotTub => "Hot tub / jacuzzi",
            Self::Sauna => "Sauna / steam room",
            Self::Gym => "Fitness center / gym",
            Self::YogaStudio => "Yoga / meditation space",
            Self::SpaMassage => "Spa / massage",
            Self::PrivateRooms => "Private rooms",
            Self::SharedRooms => "Semi-private / shared rooms",
            Self::PetFriendly => "Pet-friendly",
            Self::Laundry => "On-site laundry",
            Self::Wifi => "Wi-Fi",
            Self::Workspace => "Workspace / executive desk",
            Self::OutdoorGrounds => "Outdoor grounds / trails",
            Self::RecreationCourts => "Sports / rec courts",
            Self::EquineTherapy => "Equine / animal program",
            Self::ArtMusicStudio => "Art / music studio",
            Self::LoungeGameRoom => "Lounge / game room",
            Self::FamilyVisitation => "Family visitation",
            Self::OffsiteOutings => "Off-site outings / passes",
            Self::Transportation => "Transportation assistance",
            Self::HousingAssistance => "Housing assistance",
            Self::Childcare => "Childcare assistance",
        }
    }

    pub const fn category(self) -> AmenityCategory {
        match self {
            Self::Pool
            | Self::HotTub
            | Self::Sauna
            | Self::Gym
            | Self::YogaStudio
            | Self::SpaMassage => AmenityCategory::WaterAndWellness,
            Self::PrivateRooms
            | Self::SharedRooms
            | Self::PetFriendly
            | Self::Laundry
            | Self::Wifi
            | Self::Workspace => AmenityCategory::RoomsAndLiving,
            Self::OutdoorGrounds
            | Self::RecreationCourts
            | Self::EquineTherapy
            | Self::ArtMusicStudio
            | Self::LoungeGameRoom => AmenityCategory::CampusAndRecreation,
            Self::FamilyVisitation
            | Self::OffsiteOutings
            | Self::Transportation
            | Self::HousingAssistance
            | Self::Childcare => AmenityCategory::PoliciesAndSupport,
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }
}

/// Primary food & dining setup at a facility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FoodSituation {
    #[serde(rename = "chef-prepared")]
    ChefPrepared,
    #[serde(rename = "dietitian-catered")]
    DietitianCatered,
    #[serde(rename = "cafeteria")]
    Cafeteria,
    #[serde(rename = "self-catered")]
    SelfCatered,
}

impl FoodSituation {
    pub const ALL: &'static [Self] = &[
        Self::ChefPrepared,
        Self::DietitianCatered,
        Self::Cafeteria,
        Self::SelfCatered,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::ChefPrepared => "chef-prepared",
            Self::DietitianCatered => "dietitian-catered",
            Self::Cafeteria => "cafeteria",
            Self::SelfCatered => "self-catered",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ChefPrepared => "Chef-prepared / gourmet meals",
            Self::DietitianCatered => "Dietitian-planned / catered meals",
            Self::Cafeteria => "Cafeteria / standard dining",
            Self::SelfCatered => "Resident kitchen / self-catering",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::ChefPrepared => "Chef-prepared meals",
            Self::DietitianCatered => "Dietitian-planned meals",
            Self::Cafeteria => "Cafeteria dining",
            Self::SelfCatered => "Resident kitchen",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }
}

/// Specific dietary accommodations & snack availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DietaryOption {
    #[serde(rename = "vegan-vegetarian")]
    VeganVegetarian,
    #[serde(rename = "gluten-free")]
    GlutenFree,
    #[serde(rename = "kosher")]
    Kosher,
    #[serde(rename = "halal")]
    Halal,
    #[serde(rename = "allergy-accommodated")]
    AllergyAccommodated,
    #[serde(rename = "snacks-coffee")]
    SnacksCoffeeBar,
}

impl DietaryOption {
    pub const ALL: &'static [Self] = &[
        Self::VeganVegetarian,
        Self::GlutenFree,
        Self::Kosher,
        Self::Halal,
        Self::AllergyAccommodated,
        Self::SnacksCoffeeBar,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::VeganVegetarian => "vegan-vegetarian",
            Self::GlutenFree => "gluten-free",
            Self::Kosher => "kosher",
            Self::Halal => "halal",
            Self::AllergyAccommodated => "allergy-accommodated",
            Self::SnacksCoffeeBar => "snacks-coffee",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::VeganVegetarian => "Vegan / vegetarian",
            Self::GlutenFree => "Gluten-free",
            Self::Kosher => "Kosher",
            Self::Halal => "Halal",
            Self::AllergyAccommodated => "Allergy accommodations",
            Self::SnacksCoffeeBar => "Snacks & coffee bar",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }
}

/// Personal phone, laptop, and tablet allowance policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ElectronicsPolicy {
    #[serde(rename = "unrestricted")]
    Unrestricted,
    #[serde(rename = "scheduled")]
    Scheduled,
    #[serde(rename = "phones-only")]
    PhonesOnly,
    #[serde(rename = "work-approved")]
    WorkApproved,
    #[serde(rename = "prohibited")]
    Prohibited,
}

impl ElectronicsPolicy {
    pub const ALL: &'static [Self] = &[
        Self::Unrestricted,
        Self::Scheduled,
        Self::PhonesOnly,
        Self::WorkApproved,
        Self::Prohibited,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Unrestricted => "unrestricted",
            Self::Scheduled => "scheduled",
            Self::PhonesOnly => "phones-only",
            Self::WorkApproved => "work-approved",
            Self::Prohibited => "prohibited",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Unrestricted => "Phones & laptops allowed freely",
            Self::Scheduled => "Allowed during scheduled hours / after blackout",
            Self::PhonesOnly => "Phones allowed (limited); no laptops",
            Self::WorkApproved => "Allowed for approved work / executive needs",
            Self::Prohibited => "Personal phones & devices not permitted",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Unrestricted => "Devices: Unrestricted",
            Self::Scheduled => "Devices: Scheduled hours",
            Self::PhonesOnly => "Devices: Phones only",
            Self::WorkApproved => "Devices: Work-approved",
            Self::Prohibited => "Devices: Not permitted",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }
}

/// Facility tobacco smoking policy (directly mapped from SAMHSA `SMP` or verified override).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SmokingPolicy {
    #[serde(rename = "not-permitted")]
    NotPermitted,
    #[serde(rename = "designated-area")]
    DesignatedArea,
    #[serde(rename = "permitted")]
    Permitted,
}

impl SmokingPolicy {
    pub const ALL: &'static [Self] = &[Self::NotPermitted, Self::DesignatedArea, Self::Permitted];

    pub const fn code(self) -> &'static str {
        match self {
            Self::NotPermitted => "not-permitted",
            Self::DesignatedArea => "designated-area",
            Self::Permitted => "permitted",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotPermitted => "Smoking not permitted",
            Self::DesignatedArea => "Smoking in designated area",
            Self::Permitted => "Smoking permitted",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }

    pub fn from_samhsa_label(label: &str) -> Option<Self> {
        let lower = label.trim().to_ascii_lowercase();
        if lower.contains("not permitted") {
            Some(Self::NotPermitted)
        } else if lower.contains("designated") {
            Some(Self::DesignatedArea)
        } else if lower.contains("without restriction") || lower.contains("permitted") {
            Some(Self::Permitted)
        } else {
            None
        }
    }
}

/// Facility vaping policy (directly mapped from SAMHSA `FVP` or verified override).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VapingPolicy {
    #[serde(rename = "not-permitted")]
    NotPermitted,
    #[serde(rename = "designated-area")]
    DesignatedArea,
    #[serde(rename = "permitted")]
    Permitted,
}

impl VapingPolicy {
    pub const ALL: &'static [Self] = &[Self::NotPermitted, Self::DesignatedArea, Self::Permitted];

    pub const fn code(self) -> &'static str {
        match self {
            Self::NotPermitted => "not-permitted",
            Self::DesignatedArea => "designated-area",
            Self::Permitted => "permitted",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotPermitted => "Vaping not permitted",
            Self::DesignatedArea => "Vaping in designated area",
            Self::Permitted => "Vaping permitted",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }

    pub fn from_samhsa_label(label: &str) -> Option<Self> {
        let lower = label.trim().to_ascii_lowercase();
        if lower.contains("not permitted") {
            Some(Self::NotPermitted)
        } else if lower.contains("designated") {
            Some(Self::DesignatedArea)
        } else if lower.contains("without restriction") || lower.contains("permitted") {
            Some(Self::Permitted)
        } else {
            None
        }
    }
}

/// Clinical licensure tier used to organize staff license filters without conflating credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StaffLicenseTier {
    Physicians,
    AdvancedPractice,
    Psychologists,
    TherapistsAndSocialWorkers,
    NursingAndAddictionSupport,
}

impl StaffLicenseTier {
    pub const ALL: &'static [Self] = &[
        Self::Physicians,
        Self::AdvancedPractice,
        Self::Psychologists,
        Self::TherapistsAndSocialWorkers,
        Self::NursingAndAddictionSupport,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Physicians => "Physicians (MD vs DO — distinct)",
            Self::AdvancedPractice => "Advanced Practice (NP / PMHNP / PA — not physicians)",
            Self::Psychologists => "Doctoral Psychologists (PhD vs PsyD)",
            Self::TherapistsAndSocialWorkers => "Master's Therapists & Clinical Social Workers",
            Self::NursingAndAddictionSupport => "Nursing & Addiction Support",
        }
    }
}

/// Exact clinical staff license types.
///
/// Deliberately never conflates credentials:
/// - `MdPsychiatrist` is distinct from `DoPsychiatrist` (an MD is not a DO).
/// - Both are distinct from `Pmhnp` and `Np` (a psychiatrist is not a nurse practitioner) and `Pa`.
/// - `PhdPsychologist` is distinct from `PsydPsychologist`.
/// - `Lcsw`, `LpcLmhc`, and `Lmft` remain separate licenses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StaffLicense {
    // Physicians — MD vs DO strictly separated, and psychiatry/addiction separated from general medical
    #[serde(rename = "md-psychiatrist")]
    MdPsychiatrist,
    #[serde(rename = "do-psychiatrist")]
    DoPsychiatrist,
    #[serde(rename = "md-addiction")]
    MdAddiction,
    #[serde(rename = "do-addiction")]
    DoAddiction,
    #[serde(rename = "md-physician")]
    MdPhysician,
    #[serde(rename = "do-physician")]
    DoPhysician,

    // Advanced Practice — strictly distinct from MD/DO psychiatrists and physicians
    #[serde(rename = "pmhnp")]
    Pmhnp,
    #[serde(rename = "np")]
    Np,
    #[serde(rename = "pa")]
    Pa,

    // Psychologists
    #[serde(rename = "phd-psychologist")]
    PhdPsychologist,
    #[serde(rename = "psyd-psychologist")]
    PsydPsychologist,

    // Master's-level therapists & clinical social workers
    #[serde(rename = "lcsw")]
    Lcsw,
    #[serde(rename = "lpc-lmhc")]
    LpcLmhc,
    #[serde(rename = "lmft")]
    Lmft,

    // Nursing & addiction counselors
    #[serde(rename = "rn")]
    Rn,
    #[serde(rename = "lpn")]
    Lpn,
    #[serde(rename = "addiction-counselor")]
    AddictionCounselor,
    #[serde(rename = "peer-support")]
    PeerSupport,
}

impl StaffLicense {
    pub const ALL: &'static [Self] = &[
        Self::MdPsychiatrist,
        Self::DoPsychiatrist,
        Self::MdAddiction,
        Self::DoAddiction,
        Self::MdPhysician,
        Self::DoPhysician,
        Self::Pmhnp,
        Self::Np,
        Self::Pa,
        Self::PhdPsychologist,
        Self::PsydPsychologist,
        Self::Lcsw,
        Self::LpcLmhc,
        Self::Lmft,
        Self::Rn,
        Self::Lpn,
        Self::AddictionCounselor,
        Self::PeerSupport,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::MdPsychiatrist => "md-psychiatrist",
            Self::DoPsychiatrist => "do-psychiatrist",
            Self::MdAddiction => "md-addiction",
            Self::DoAddiction => "do-addiction",
            Self::MdPhysician => "md-physician",
            Self::DoPhysician => "do-physician",
            Self::Pmhnp => "pmhnp",
            Self::Np => "np",
            Self::Pa => "pa",
            Self::PhdPsychologist => "phd-psychologist",
            Self::PsydPsychologist => "psyd-psychologist",
            Self::Lcsw => "lcsw",
            Self::LpcLmhc => "lpc-lmhc",
            Self::Lmft => "lmft",
            Self::Rn => "rn",
            Self::Lpn => "lpn",
            Self::AddictionCounselor => "addiction-counselor",
            Self::PeerSupport => "peer-support",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MdPsychiatrist => "MD — Psychiatrist",
            Self::DoPsychiatrist => "DO — Psychiatrist",
            Self::MdAddiction => "MD — Addiction Medicine",
            Self::DoAddiction => "DO — Addiction Medicine",
            Self::MdPhysician => "MD — Physician (Other)",
            Self::DoPhysician => "DO — Physician (Other)",
            Self::Pmhnp => "PMHNP — Psychiatric Nurse Practitioner",
            Self::Np => "NP / APRN — Nurse Practitioner",
            Self::Pa => "PA / PA-C — Physician Assistant",
            Self::PhdPsychologist => "PhD — Psychologist",
            Self::PsydPsychologist => "PsyD — Psychologist",
            Self::Lcsw => "LCSW — Clinical Social Worker",
            Self::LpcLmhc => "LPC / LMHC — Professional Counselor",
            Self::Lmft => "LMFT — Marriage & Family Therapist",
            Self::Rn => "RN — Registered Nurse",
            Self::Lpn => "LPN / LVN — Licensed Practical Nurse",
            Self::AddictionCounselor => "LADC / CADC — Addiction Counselor",
            Self::PeerSupport => "Peer Support Specialist",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::MdPsychiatrist => "MD Psychiatrist",
            Self::DoPsychiatrist => "DO Psychiatrist",
            Self::MdAddiction => "MD Addiction Med",
            Self::DoAddiction => "DO Addiction Med",
            Self::MdPhysician => "MD Physician",
            Self::DoPhysician => "DO Physician",
            Self::Pmhnp => "PMHNP (Psych NP)",
            Self::Np => "NP / APRN",
            Self::Pa => "PA / PA-C",
            Self::PhdPsychologist => "PhD Psychologist",
            Self::PsydPsychologist => "PsyD Psychologist",
            Self::Lcsw => "LCSW",
            Self::LpcLmhc => "LPC / LMHC",
            Self::Lmft => "LMFT",
            Self::Rn => "RN",
            Self::Lpn => "LPN / LVN",
            Self::AddictionCounselor => "LADC / CADC",
            Self::PeerSupport => "Peer Support",
        }
    }

    pub const fn tier(self) -> StaffLicenseTier {
        match self {
            Self::MdPsychiatrist
            | Self::DoPsychiatrist
            | Self::MdAddiction
            | Self::DoAddiction
            | Self::MdPhysician
            | Self::DoPhysician => StaffLicenseTier::Physicians,
            Self::Pmhnp | Self::Np | Self::Pa => StaffLicenseTier::AdvancedPractice,
            Self::PhdPsychologist | Self::PsydPsychologist => StaffLicenseTier::Psychologists,
            Self::Lcsw | Self::LpcLmhc | Self::Lmft => StaffLicenseTier::TherapistsAndSocialWorkers,
            Self::Rn | Self::Lpn | Self::AddictionCounselor | Self::PeerSupport => {
                StaffLicenseTier::NursingAndAddictionSupport
            }
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }

    pub const fn is_physician(self) -> bool {
        matches!(
            self,
            Self::MdPsychiatrist
                | Self::DoPsychiatrist
                | Self::MdAddiction
                | Self::DoAddiction
                | Self::MdPhysician
                | Self::DoPhysician
        )
    }

    pub const fn is_prescriber(self) -> bool {
        self.is_physician() || matches!(self, Self::Pmhnp | Self::Np | Self::Pa)
    }
}

/// Non-clinical / wellness / contracted specialty roles often shared across clinics and rehabs
/// in a metro area (e.g. a local yoga studio instructor serving multiple Phoenix/Scottsdale programs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WellnessRole {
    #[serde(rename = "yoga-instructor")]
    YogaInstructor,
    #[serde(rename = "fitness-trainer")]
    FitnessTrainer,
    #[serde(rename = "equine-specialist")]
    EquineSpecialist,
    #[serde(rename = "art-music-therapist")]
    ArtMusicTherapist,
    #[serde(rename = "culinary-chef")]
    CulinaryChef,
    #[serde(rename = "dietitian")]
    DietitianNutritionist,
    #[serde(rename = "bodywork")]
    AcupuncturistMassage,
    #[serde(rename = "recreation-therapist")]
    RecreationTherapist,
    #[serde(rename = "chaplain")]
    SpiritualChaplain,
}

impl WellnessRole {
    pub const ALL: &'static [Self] = &[
        Self::YogaInstructor,
        Self::FitnessTrainer,
        Self::EquineSpecialist,
        Self::ArtMusicTherapist,
        Self::CulinaryChef,
        Self::DietitianNutritionist,
        Self::AcupuncturistMassage,
        Self::RecreationTherapist,
        Self::SpiritualChaplain,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::YogaInstructor => "yoga-instructor",
            Self::FitnessTrainer => "fitness-trainer",
            Self::EquineSpecialist => "equine-specialist",
            Self::ArtMusicTherapist => "art-music-therapist",
            Self::CulinaryChef => "culinary-chef",
            Self::DietitianNutritionist => "dietitian",
            Self::AcupuncturistMassage => "bodywork",
            Self::RecreationTherapist => "recreation-therapist",
            Self::SpiritualChaplain => "chaplain",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::YogaInstructor => "Yoga / Meditation Instructor",
            Self::FitnessTrainer => "Fitness / Personal Trainer",
            Self::EquineSpecialist => "Equine / Animal Specialist",
            Self::ArtMusicTherapist => "Art / Music Therapist",
            Self::CulinaryChef => "Chef / Culinary Service",
            Self::DietitianNutritionist => "Dietitian / Nutritionist",
            Self::AcupuncturistMassage => "Acupuncture / Massage",
            Self::RecreationTherapist => "Recreation / Adventure Guide",
            Self::SpiritualChaplain => "Spiritual / Chaplaincy",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        Self::ALL.iter().copied().find(|v| v.code() == trimmed)
    }
}

/// Strictly separated provider role: either an exact clinical license or a wellness/specialty role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "code")]
pub enum ProviderRole {
    #[serde(rename = "clinical")]
    Clinical(StaffLicense),
    #[serde(rename = "wellness")]
    Wellness(WellnessRole),
}

impl ProviderRole {
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Clinical(lic) => lic.short_label(),
            Self::Wellness(w) => w.label(),
        }
    }

    pub const fn is_clinical(self) -> bool {
        matches!(self, Self::Clinical(_))
    }

    pub const fn is_prescriber(self) -> bool {
        matches!(self, Self::Clinical(lic) if lic.is_prescriber())
    }
}

/// How a clinician or wellness provider works with the facilities they serve.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum StaffArrangement {
    /// Practices exclusively at one facility.
    #[default]
    #[serde(rename = "dedicated")]
    Dedicated,
    /// Practices at two or more facilities in the region.
    #[serde(rename = "shared")]
    Shared,
    /// Visiting or contracted provider rotating across multiple clinics/rehabs.
    #[serde(rename = "visiting")]
    Visiting,
}

impl StaffArrangement {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Dedicated => "dedicated",
            Self::Shared => "shared",
            Self::Visiting => "visiting",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dedicated => "Dedicated",
            Self::Shared => "Shared across sites",
            Self::Visiting => "Visiting / contracted",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "dedicated" => Some(Self::Dedicated),
            "shared" => Some(Self::Shared),
            "visiting" | "contracted" => Some(Self::Visiting),
            _ => None,
        }
    }
}

/// A shared, visiting, or curated provider/instructor in a state's cross-facility network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProvider {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npi: Option<String>,
    pub role: ProviderRole,
    pub arrangement: StaffArrangement,
    #[serde(default)]
    pub facility_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<DataSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Per-facility summary of dedicated vs shared/visiting staff so families can compare care models.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaffingProfile {
    /// Matched clinical staff who only appear at this single facility.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub dedicated_clinicians: u32,
    /// Matched clinical staff who also practice at other facilities in the state.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub shared_clinicians: u32,
    /// Single-site MD/DO physicians (psychiatrists, addiction med, or general physicians).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub dedicated_physicians: u32,
    /// Shared/visiting prescribers (MD/DO/PMHNP/NP/PA practicing across 2+ facilities).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub shared_prescribers: u32,
    /// Visiting/contracted wellness or specialty providers (yoga, equine, culinary, etc.).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub visiting_specialists: u32,
    /// Provider IDs in `StateCatalog::staff_network` linked to this facility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_provider_ids: Vec<String>,
    /// Count of distinct peer facilities in this state that share at least one provider with this facility.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub shared_with_facilities: u32,
}

impl StaffingProfile {
    pub fn is_empty(&self) -> bool {
        self.dedicated_clinicians == 0
            && self.shared_clinicians == 0
            && self.dedicated_physicians == 0
            && self.shared_prescribers == 0
            && self.visiting_specialists == 0
            && self.network_provider_ids.is_empty()
            && self.shared_with_facilities == 0
    }
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// Where an enriched attribute came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DataSource {
    #[serde(rename = "samhsa")]
    Samhsa,
    #[serde(rename = "nppes")]
    Nppes,
    #[serde(rename = "website")]
    Website,
    #[serde(rename = "llm")]
    Llm,
    #[serde(rename = "verified")]
    Verified,
}

impl DataSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Samhsa => "SAMHSA",
            Self::Nppes => "CMS NPPES",
            Self::Website => "Facility Website",
            Self::Llm => "LLM Extracted",
            Self::Verified => "Verified",
        }
    }
}

/// Non-care lifestyle, amenities, food, device, and policy details for a facility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacilityLifestyle {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amenities: Vec<Amenity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub food: Option<FoodSituation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dietary: Vec<DietaryOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electronics: Option<ElectronicsPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoking: Option<SmokingPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vaping: Option<VapingPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl FacilityLifestyle {
    pub fn is_empty(&self) -> bool {
        self.amenities.is_empty()
            && self.food.is_none()
            && self.dietary.is_empty()
            && self.electronics.is_none()
            && self.smoking.is_none()
            && self.vaping.is_none()
            && self.languages.is_empty()
            && self
                .notes
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
    }

    pub fn normalize(&mut self) {
        self.amenities.sort_unstable();
        self.amenities.dedup();
        self.dietary.sort_unstable();
        self.dietary.dedup();
        self.languages.sort_unstable();
        self.languages.dedup();
        if self
            .notes
            .as_deref()
            .map(str::trim)
            .is_some_and(str::is_empty)
        {
            self.notes = None;
        }
    }
}

/// Provenance and cache freshness metadata attached to an enriched facility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentMeta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<DataSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nppes_provider_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_checked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_urls: Vec<String>,
}

impl EnrichmentMeta {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
            && self.nppes_provider_count.is_none()
            && self.website_checked.is_none()
            && self.verified_at.is_none()
            && self.evidence_urls.is_empty()
    }

    pub fn add_source(&mut self, source: DataSource) {
        if !self.sources.contains(&source) {
            self.sources.push(source);
            self.sources.sort_unstable();
        }
    }

    pub fn add_evidence_url(&mut self, url: String) {
        if !self.evidence_urls.contains(&url) {
            self.evidence_urls.push(url);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    pub lat: f64,
    pub lng: f64,
}

impl Coordinates {
    /// Great-circle distance in kilometres using the Haversine formula.
    pub fn distance_km(self, other: Self) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6_371.008_8;
        let lat1 = self.lat.to_radians();
        let lat2 = other.lat.to_radians();
        let dlat = (other.lat - self.lat).to_radians();
        let dlng = (other.lng - self.lng).to_radians();
        let sin_lat = (dlat * 0.5).sin();
        let sin_lng = (dlng * 0.5).sin();
        let a = sin_lat * sin_lat + lat1.cos() * lat2.cos() * sin_lng * sin_lng;
        let c = 2.0 * a.sqrt().min(1.0).asin();
        EARTH_RADIUS_KM * c
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    #[serde(default)]
    pub street: Option<String>,
    #[serde(default)]
    pub street2: Option<String>,
    pub city: String,
    pub state: String,
    pub zip: String,
}

impl Address {
    pub fn display_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(street) = self
            .street
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(street.to_string());
        }
        if let Some(street2) = self
            .street2
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(street2.to_string());
        }
        parts.push(format!("{}, {} {}", self.city, self.state, self.zip));
        parts.join(", ")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Facility {
    /// Optional stable slug persisted in enriched shards; computed deterministically if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub site_name: Option<String>,
    pub address: Address,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub location: Option<Coordinates>,
    #[serde(default)]
    pub care: Vec<CareType>,
    #[serde(default)]
    pub settings: Vec<Setting>,
    #[serde(default)]
    pub payments: Vec<Payment>,
    #[serde(default)]
    pub groups: Vec<Group>,
    /// Non-care features: pools, hot tubs, rooms, food, electronic devices, smoking/vaping, etc.
    #[serde(default, skip_serializing_if = "FacilityLifestyle::is_empty")]
    pub lifestyle: FacilityLifestyle,
    /// Exact clinical staff license types present at this facility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staff_licenses: Vec<StaffLicense>,
    /// Dedicated vs shared/visiting staffing summary and links into `StateCatalog::staff_network`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staffing_profile: Option<StaffingProfile>,
    /// Provenance & freshness metadata for enriched fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrichment: Option<EnrichmentMeta>,
}

impl Facility {
    /// Deterministic, human-readable facility identifier stable across SAMHSA re-imports.
    ///
    /// Format: `<state>-<zip>-<slug-of-name-and-site>-<4-hex-street-hash>`
    pub fn stable_id(&self) -> String {
        if let Some(existing) = self.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            return existing.to_string();
        }
        compute_stable_id(
            &self.address.state,
            &self.address.zip,
            &self.name,
            self.site_name.as_deref(),
            self.address.street.as_deref(),
        )
    }

    pub fn is_enriched(&self) -> bool {
        !self.lifestyle.is_empty() || !self.staff_licenses.is_empty()
    }

    /// Safe external website URL (strictly `http://` or `https://`).
    pub fn safe_website(&self) -> Option<&str> {
        let url = self.website.as_deref()?.trim();
        if url.starts_with("https://") || url.starts_with("http://") {
            Some(url)
        } else {
            None
        }
    }

    /// Clean `tel:` URI built from digits of the main phone number.
    pub fn tel_href(&self) -> Option<String> {
        let raw = self.phone.as_deref()?.trim();
        let main = raw.split(['x', 'X']).next().unwrap_or(raw);
        let digits: String = main.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 10 {
            Some(format!("tel:{digits}"))
        } else {
            None
        }
    }

    /// Maps lookup URL (safe query string).
    pub fn maps_href(&self) -> String {
        let q = format!("{} {}", self.name, self.address.display_line());
        format!(
            "https://www.google.com/maps/search/?api=1&query={}",
            percent_encode_query(&q)
        )
    }
}

/// Computes a deterministic, collision-resistant slug for a facility.
pub fn compute_stable_id(
    state: &str,
    zip: &str,
    name: &str,
    site_name: Option<&str>,
    street: Option<&str>,
) -> String {
    let state_slug = state.trim().to_ascii_lowercase();
    let zip_slug: String = zip.chars().filter(|c| c.is_ascii_digit()).take(5).collect();
    let combined_name = match site_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(site) => format!("{name} {site}"),
        None => name.to_string(),
    };
    let slug = slugify(&combined_name, 48);
    // Lightweight FNV-1a 32-bit suffix over full identity to disambiguate multi-site clinics
    // sharing a name and ZIP code without pulling SHA-256 into WASM.
    let identity = format!(
        "{}|{}|{}|{}|{}",
        state_slug,
        zip_slug,
        name.trim().to_ascii_lowercase(),
        site_name.unwrap_or_default().trim().to_ascii_lowercase(),
        street.unwrap_or_default().trim().to_ascii_lowercase(),
    );
    let mut hash: u32 = 0x811c_9dc5;
    for byte in identity.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{state_slug}-{zip_slug}-{slug}-{:06x}", hash & 0x00ff_ffff)
}

fn slugify(input: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= max_len {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn percent_encode_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateCatalog {
    pub state: String,
    pub retrieved: String,
    pub facilities: Vec<Facility>,
    /// Cross-facility network of shared clinicians and visiting/contracted specialists in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staff_network: Vec<NetworkProvider>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateIndexEntry {
    pub state: String,
    pub facilities: usize,
    pub retrieved: String,
    /// Content SHA-256 hex digest of `<ST>.json`, used by the browser Cache Storage layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Number of facilities in this state with non-care lifestyle or staff license enrichment.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub enriched_facilities: usize,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateIndex {
    pub states: Vec<StateIndexEntry>,
}

/// Human-readable US state/territory name for a two-letter postal code.
pub fn state_full_name(code: &str) -> &'static str {
    match code {
        "AK" => "Alaska",
        "AL" => "Alabama",
        "AR" => "Arkansas",
        "AZ" => "Arizona",
        "CA" => "California",
        "CO" => "Colorado",
        "CT" => "Connecticut",
        "DC" => "District of Columbia",
        "DE" => "Delaware",
        "FL" => "Florida",
        "GA" => "Georgia",
        "GU" => "Guam",
        "HI" => "Hawaii",
        "IA" => "Iowa",
        "ID" => "Idaho",
        "IL" => "Illinois",
        "IN" => "Indiana",
        "KS" => "Kansas",
        "KY" => "Kentucky",
        "LA" => "Louisiana",
        "MA" => "Massachusetts",
        "MD" => "Maryland",
        "ME" => "Maine",
        "MI" => "Michigan",
        "MN" => "Minnesota",
        "MO" => "Missouri",
        "MP" => "Northern Mariana Islands",
        "MS" => "Mississippi",
        "MT" => "Montana",
        "NC" => "North Carolina",
        "ND" => "North Dakota",
        "NE" => "Nebraska",
        "NH" => "New Hampshire",
        "NJ" => "New Jersey",
        "NM" => "New Mexico",
        "NV" => "Nevada",
        "NY" => "New York",
        "OH" => "Ohio",
        "OK" => "Oklahoma",
        "OR" => "Oregon",
        "PA" => "Pennsylvania",
        "PR" => "Puerto Rico",
        "RI" => "Rhode Island",
        "SC" => "South Carolina",
        "SD" => "South Dakota",
        "TN" => "Tennessee",
        "TX" => "Texas",
        "UT" => "Utah",
        "VA" => "Virginia",
        "VI" => "U.S. Virgin Islands",
        "VT" => "Vermont",
        "WA" => "Washington",
        "WI" => "Wisconsin",
        "WV" => "West Virginia",
        "WY" => "Wyoming",
        _ => "United States",
    }
}
