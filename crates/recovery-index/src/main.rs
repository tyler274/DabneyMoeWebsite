//! In-repo CLI for importing, enriching, caching, and maintaining the recovery directory index.
//!
//! Run from the repository root:
//! ```text
//! cargo run -p recovery-index --features cli -- <subcommand> [options]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use recovery_index::schema::{
    Amenity, DietaryOption, ElectronicsPolicy, FoodSituation, ProviderRole, SmokingPolicy,
    StaffArrangement, StaffLicense, VapingPolicy, WellnessRole,
};
use recovery_index::updater::cache::CacheMode;
use recovery_index::updater::llm::{LlmBackend, LlmConfig};
use recovery_index::updater::{
    run_add_provider, run_enrich, run_find, run_import, run_network, run_rebuild, run_set_override,
    run_stats, AddProviderOptions, EnrichOptions, ImportOptions, PathsConfig, SetOverrideOptions,
    UpdaterError,
};

const USAGE: &str = "\
recovery-index: Cached in-repo updater for dabney.moe/tools/recovery

USAGE:
  cargo run -p recovery-index --features cli -- <command> [options]

COMMANDS:
  import    Fetch/cache SAMHSA FindTreatment.gov pages and update state shards + index.json
            [--states CO,WY] [--from-file RAW.json] [--retrieved YYYY-MM-DD]
            [--out DIR] [--overrides DIR] [--cache-dir DIR] [--offline | --refresh]

  enrich    Enrich existing state shards via cached CMS NPPES (exact MD/DO/Psychiatrist/NP/PA
            staff licenses) + cached facility websites (pools, hot tubs, food, devices, rooms)
            + optional cached LLM extraction (codex/claude/openai/ollama) + curated TOML overrides,
            then update index.json SHA-256 hashes.
            [--states CO,AZ,NY,CA] [--limit N] [--residential-first]
            [--skip-nppes] [--skip-web]
            [--llm codex|claude|openai|ollama] [--llm-model MODEL]
            [--llm-base-url URL] [--llm-concurrency N]
            [--offline | --refresh]
            [--out DIR] [--overrides DIR] [--cache-dir DIR]

  rebuild   Offline rebuild: assign stable IDs, apply data/recovery/overrides/*.toml,
            write shards, and recompute SHA-256 digests in public/recovery/index.json.
            [--states CO,WY] [--out DIR] [--overrides DIR]

  set       Add or update a curated/verified facility override in data/recovery/overrides/<ST>.toml
            and immediately rebuild that state's shard + index.json.
            --state ST --id FACILITY_ID
            [--add-amenity CODE]... [--remove-amenity CODE]...
            [--food CODE] [--add-dietary CODE]... [--remove-dietary CODE]...
            [--electronics CODE] [--smoking CODE] [--vaping CODE]
            [--add-staff CODE]... [--remove-staff CODE]...
            [--notes TEXT] [--evidence-url URL]... [--verify]

  add-provider
            Map a shared clinician or visiting/contracted specialist (e.g. a visiting PMHNP or
            local yoga instructor) across facilities in data/recovery/overrides/<ST>.toml.
            --state ST --name NAME (--clinical CODE | --wellness CODE) --facilities ID1,ID2
            [--id ID] [--org STUDIO_OR_GROUP] [--npi 10DIGITS]
            [--arrangement dedicated|shared|visiting] [--notes TEXT]

  network   Inspect the cross-facility shared-staff & visiting-provider network for a state.
            --state ST [--top N] [--role CODE]

  find      Search facilities in a state shard to inspect stable IDs & current enrichment.
            --state ST [--query TEXT] [--limit N]

  stats     Print enrichment coverage statistics across state shards.
            [--state ST]

  codes     Print all valid enum codes for amenities, food, dietary, electronics, smoking,
            vaping, and staff licenses.
";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help" | "help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let result = match args[0].as_str() {
        "import" => cmd_import(&args[1..]).await,
        "enrich" => cmd_enrich(&args[1..]).await,
        "rebuild" => cmd_rebuild(&args[1..]),
        "set" => cmd_set(&args[1..]),
        "add-provider" => cmd_add_provider(&args[1..]),
        "network" => cmd_network(&args[1..]),
        "find" => cmd_find(&args[1..]),
        "stats" => cmd_stats(&args[1..]),
        "codes" => {
            print_codes();
            Ok(())
        }
        other => Err(UpdaterError::Usage(format!(
            "Unknown command `{other}`.\n\n{USAGE}"
        ))),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            let mut src = std::error::Error::source(&err);
            while let Some(cause) = src {
                eprintln!("  caused by: {cause}");
                src = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn next_val<'a>(
    iter: &mut std::slice::Iter<'a, String>,
    flag: &str,
) -> Result<&'a str, UpdaterError> {
    iter.next()
        .map(String::as_str)
        .ok_or_else(|| UpdaterError::Usage(format!("Missing value for {flag}")))
}

fn parse_csv_states(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .collect()
}

async fn cmd_import(args: &[String]) -> Result<(), UpdaterError> {
    let mut opts = ImportOptions {
        paths: PathsConfig::default(),
        states: Vec::new(),
        from_files: Vec::new(),
        retrieved: None,
        cache_mode: CacheMode::Normal,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--states" => opts
                .states
                .extend(parse_csv_states(next_val(&mut it, "--states")?)),
            "--from-file" => opts
                .from_files
                .push(PathBuf::from(next_val(&mut it, "--from-file")?)),
            "--retrieved" => opts.retrieved = Some(next_val(&mut it, "--retrieved")?.to_string()),
            "--out" => opts.paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            "--overrides" => {
                opts.paths.overrides_dir = PathBuf::from(next_val(&mut it, "--overrides")?)
            }
            "--cache-dir" => {
                opts.paths.cache_dir = PathBuf::from(next_val(&mut it, "--cache-dir")?)
            }
            "--offline" => opts.cache_mode = CacheMode::Offline,
            "--refresh" => opts.cache_mode = CacheMode::Refresh,
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    run_import(opts).await
}

async fn cmd_enrich(args: &[String]) -> Result<(), UpdaterError> {
    let mut opts = EnrichOptions {
        paths: PathsConfig::default(),
        states: Vec::new(),
        limit: None,
        residential_first: false,
        skip_nppes: false,
        skip_web: false,
        llm: None,
        cache_mode: CacheMode::Normal,
    };
    let mut llm_backend: Option<LlmBackend> = None;
    let mut llm_model: Option<String> = None;
    let mut llm_base_url: Option<String> = None;
    let mut llm_concurrency: usize = 3;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--states" => opts
                .states
                .extend(parse_csv_states(next_val(&mut it, "--states")?)),
            "--limit" => {
                let raw = next_val(&mut it, "--limit")?;
                let n = raw
                    .parse::<usize>()
                    .map_err(|_| UpdaterError::Usage(format!("Invalid --limit `{raw}`")))?;
                opts.limit = Some(n);
            }
            "--residential-first" => opts.residential_first = true,
            "--skip-nppes" => opts.skip_nppes = true,
            "--skip-web" => opts.skip_web = true,
            "--llm" => {
                let raw = next_val(&mut it, "--llm")?;
                let backend = LlmBackend::parse(raw).ok_or_else(|| {
                    UpdaterError::Usage(format!(
                        "Unknown --llm backend `{raw}`. Use codex, claude, openai, or ollama."
                    ))
                })?;
                llm_backend = Some(backend);
            }
            "--llm-model" => llm_model = Some(next_val(&mut it, "--llm-model")?.to_string()),
            "--llm-base-url" => {
                llm_base_url = Some(next_val(&mut it, "--llm-base-url")?.to_string())
            }
            "--llm-concurrency" => {
                let raw = next_val(&mut it, "--llm-concurrency")?;
                llm_concurrency = raw.parse::<usize>().map_err(|_| {
                    UpdaterError::Usage(format!("Invalid --llm-concurrency `{raw}`"))
                })?;
            }
            "--out" => opts.paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            "--overrides" => {
                opts.paths.overrides_dir = PathBuf::from(next_val(&mut it, "--overrides")?)
            }
            "--cache-dir" => {
                opts.paths.cache_dir = PathBuf::from(next_val(&mut it, "--cache-dir")?)
            }
            "--offline" => opts.cache_mode = CacheMode::Offline,
            "--refresh" => opts.cache_mode = CacheMode::Refresh,
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    if let Some(backend) = llm_backend {
        let mut cfg = LlmConfig::new(backend);
        cfg.model = llm_model;
        cfg.base_url = llm_base_url;
        cfg.concurrency = llm_concurrency.max(1);
        opts.llm = Some(cfg);
    }
    run_enrich(opts).await
}

fn cmd_rebuild(args: &[String]) -> Result<(), UpdaterError> {
    let mut paths = PathsConfig::default();
    let mut states = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--states" => states.extend(parse_csv_states(next_val(&mut it, "--states")?)),
            "--out" => paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            "--overrides" => paths.overrides_dir = PathBuf::from(next_val(&mut it, "--overrides")?),
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    run_rebuild(&paths, &states)
}

fn cmd_set(args: &[String]) -> Result<(), UpdaterError> {
    let mut opts = SetOverrideOptions::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => opts.state = next_val(&mut it, "--state")?.to_string(),
            "--id" => opts.id = next_val(&mut it, "--id")?.to_string(),
            "--add-amenity" => {
                for code in next_val(&mut it, "--add-amenity")?.split(',') {
                    let v = Amenity::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!(
                            "Unknown amenity `{code}`. Run `recovery-index codes` for valid codes."
                        ))
                    })?;
                    opts.add_amenities.push(v);
                }
            }
            "--remove-amenity" => {
                for code in next_val(&mut it, "--remove-amenity")?.split(',') {
                    let v = Amenity::from_code(code)
                        .ok_or_else(|| UpdaterError::Usage(format!("Unknown amenity `{code}`.")))?;
                    opts.remove_amenities.push(v);
                }
            }
            "--food" => {
                let code = next_val(&mut it, "--food")?;
                opts.food =
                    Some(FoodSituation::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!("Unknown food code `{code}`."))
                    })?);
            }
            "--add-dietary" => {
                for code in next_val(&mut it, "--add-dietary")?.split(',') {
                    let v = DietaryOption::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!("Unknown dietary code `{code}`."))
                    })?;
                    opts.add_dietary.push(v);
                }
            }
            "--remove-dietary" => {
                for code in next_val(&mut it, "--remove-dietary")?.split(',') {
                    let v = DietaryOption::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!("Unknown dietary code `{code}`."))
                    })?;
                    opts.remove_dietary.push(v);
                }
            }
            "--electronics" => {
                let code = next_val(&mut it, "--electronics")?;
                opts.electronics = Some(ElectronicsPolicy::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!("Unknown electronics policy `{code}`."))
                })?);
            }
            "--smoking" => {
                let code = next_val(&mut it, "--smoking")?;
                opts.smoking = Some(SmokingPolicy::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!("Unknown smoking policy `{code}`."))
                })?);
            }
            "--vaping" => {
                let code = next_val(&mut it, "--vaping")?;
                opts.vaping = Some(VapingPolicy::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!("Unknown vaping policy `{code}`."))
                })?);
            }
            "--add-staff" => {
                for code in next_val(&mut it, "--add-staff")?.split(',') {
                    let v = StaffLicense::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!(
                            "Unknown staff license `{code}`. Run `recovery-index codes` for valid codes."
                        ))
                    })?;
                    opts.add_staff.push(v);
                }
            }
            "--remove-staff" => {
                for code in next_val(&mut it, "--remove-staff")?.split(',') {
                    let v = StaffLicense::from_code(code).ok_or_else(|| {
                        UpdaterError::Usage(format!("Unknown staff license `{code}`."))
                    })?;
                    opts.remove_staff.push(v);
                }
            }
            "--notes" => opts.notes = Some(next_val(&mut it, "--notes")?.to_string()),
            "--evidence-url" => {
                opts.evidence_urls
                    .push(next_val(&mut it, "--evidence-url")?.to_string());
            }
            "--verify" => opts.verify = true,
            "--out" => opts.paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            "--overrides" => {
                opts.paths.overrides_dir = PathBuf::from(next_val(&mut it, "--overrides")?)
            }
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    if opts.state.is_empty() || opts.id.is_empty() {
        return Err(UpdaterError::Usage(
            "`set` requires both --state ST and --id FACILITY_ID.".to_string(),
        ));
    }
    run_set_override(opts)
}

fn cmd_add_provider(args: &[String]) -> Result<(), UpdaterError> {
    let mut paths = PathsConfig::default();
    let mut state = String::new();
    let mut id: Option<String> = None;
    let mut name = String::new();
    let mut organization: Option<String> = None;
    let mut npi: Option<String> = None;
    let mut role: Option<ProviderRole> = None;
    let mut arrangement = StaffArrangement::Visiting;
    let mut facility_ids: Vec<String> = Vec::new();
    let mut notes: Option<String> = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => state = next_val(&mut it, "--state")?.to_string(),
            "--id" => id = Some(next_val(&mut it, "--id")?.to_string()),
            "--name" => name = next_val(&mut it, "--name")?.to_string(),
            "--org" => organization = Some(next_val(&mut it, "--org")?.to_string()),
            "--npi" => npi = Some(next_val(&mut it, "--npi")?.to_string()),
            "--clinical" => {
                let code = next_val(&mut it, "--clinical")?;
                let lic = StaffLicense::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!("Unknown clinical license `{code}`."))
                })?;
                role = Some(ProviderRole::Clinical(lic));
            }
            "--wellness" => {
                let code = next_val(&mut it, "--wellness")?;
                let w = WellnessRole::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!("Unknown wellness role `{code}`."))
                })?;
                role = Some(ProviderRole::Wellness(w));
            }
            "--arrangement" => {
                let code = next_val(&mut it, "--arrangement")?;
                arrangement = StaffArrangement::from_code(code).ok_or_else(|| {
                    UpdaterError::Usage(format!(
                        "Unknown arrangement `{code}`. Use dedicated, shared, or visiting."
                    ))
                })?;
            }
            "--facilities" => {
                for fid in next_val(&mut it, "--facilities")?.split(',') {
                    let trimmed = fid.trim();
                    if !trimmed.is_empty() {
                        facility_ids.push(trimmed.to_string());
                    }
                }
            }
            "--notes" => notes = Some(next_val(&mut it, "--notes")?.to_string()),
            "--out" => paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            "--overrides" => paths.overrides_dir = PathBuf::from(next_val(&mut it, "--overrides")?),
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }

    if state.is_empty() || name.is_empty() || facility_ids.is_empty() {
        return Err(UpdaterError::Usage(
            "`add-provider` requires --state ST, --name NAME, --facilities ID1,ID2, and --clinical CODE or --wellness CODE.".to_string(),
        ));
    }
    let role = role.ok_or_else(|| {
        UpdaterError::Usage(
            "`add-provider` requires either --clinical CODE or --wellness CODE.".to_string(),
        )
    })?;

    run_add_provider(AddProviderOptions {
        paths,
        state,
        id,
        name,
        organization,
        npi,
        role,
        arrangement,
        facility_ids,
        notes,
    })
}

fn cmd_network(args: &[String]) -> Result<(), UpdaterError> {
    let mut paths = PathsConfig::default();
    let mut state = String::new();
    let mut top = 25usize;
    let mut role: Option<String> = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => state = next_val(&mut it, "--state")?.to_string(),
            "--top" => {
                let raw = next_val(&mut it, "--top")?;
                top = raw
                    .parse()
                    .map_err(|_| UpdaterError::Usage(format!("Invalid --top `{raw}`")))?;
            }
            "--role" => role = Some(next_val(&mut it, "--role")?.to_string()),
            "--out" => paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    if state.is_empty() {
        return Err(UpdaterError::Usage(
            "`network` requires --state ST.".to_string(),
        ));
    }
    run_network(&paths, &state, top, role.as_deref())
}

fn cmd_find(args: &[String]) -> Result<(), UpdaterError> {
    let mut paths = PathsConfig::default();
    let mut state = String::new();
    let mut query = String::new();
    let mut limit = 20usize;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => state = next_val(&mut it, "--state")?.to_string(),
            "--query" => query = next_val(&mut it, "--query")?.to_string(),
            "--limit" => {
                let raw = next_val(&mut it, "--limit")?;
                limit = raw
                    .parse()
                    .map_err(|_| UpdaterError::Usage(format!("Invalid --limit `{raw}`")))?;
            }
            "--out" => paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    if state.is_empty() {
        return Err(UpdaterError::Usage(
            "`find` requires --state ST.".to_string(),
        ));
    }
    run_find(&paths, &state, &query, limit)
}

fn cmd_stats(args: &[String]) -> Result<(), UpdaterError> {
    let mut paths = PathsConfig::default();
    let mut state: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => state = Some(next_val(&mut it, "--state")?.to_string()),
            "--out" => paths.out_dir = PathBuf::from(next_val(&mut it, "--out")?),
            other => return Err(UpdaterError::Usage(format!("Unknown flag `{other}`"))),
        }
    }
    run_stats(&paths, state.as_deref())
}

fn print_codes() {
    println!("Amenities (--add-amenity):");
    for v in Amenity::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nFood situation (--food):");
    for v in FoodSituation::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nDietary options (--add-dietary):");
    for v in DietaryOption::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nElectronic device policy (--electronics):");
    for v in ElectronicsPolicy::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nSmoking policy (--smoking):");
    for v in SmokingPolicy::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nVaping policy (--vaping):");
    for v in VapingPolicy::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!(
        "\nStaff license types (--add-staff / --clinical, strictly distinct, never conflated):"
    );
    for v in StaffLicense::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
    println!("\nVisiting / wellness roles (--wellness for add-provider):");
    for v in WellnessRole::ALL {
        println!("  {:22} {}", v.code(), v.label());
    }
}
