//! Processor identity and topology.
//!
//! Topology (packages, cores, hardware threads, caches) comes from
//! `/sys/devices/system/cpu/cpuN/topology` and `cache/`. Identity (vendor,
//! model, family, flags) comes from `/proc/cpuinfo`. Hybrid core types come
//! from `/sys/devices/cpu_core` and `/sys/devices/cpu_atom` (Intel) or, as an
//! inference, from differing `cpu_capacity` values (ARM big.LITTLE).

use std::collections::{BTreeMap, BTreeSet};

use sentinel_inventory::{
    Availability, CacheInfo, CacheKind, Confidence, CoreKind, CpuCore, Field, ModuleOutcome,
    ProcessorInfo,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped};
use crate::modules::system_identity::architecture;
use crate::parsers::{cache_size, cpu_list, cpuinfo, microarch};

pub const CPU: &str = "sys/devices/system/cpu";

#[derive(Debug, Clone)]
struct Logical {
    cpu: u32,
    package: Option<u32>,
    die: Option<u32>,
    core_id: Option<u32>,
    cluster: Option<u32>,
    siblings: Option<Vec<u32>>,
    capacity: Option<u32>,
    max_khz: Option<u64>,
    min_khz: Option<u64>,
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<ProcessorInfo>>, Stopped> {
    let root = &ctx.root;
    let read_u32 = |rel: String| {
        root.read_trimmed(&rel)
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
    };
    let read_u64 = |rel: String| {
        root.read_trimmed(&rel)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    };

    let Ok(entries) = root.list(CPU) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["/sys/devices/system/cpu is not available".into()],
        ));
    };
    let present: Vec<u32> = entries
        .iter()
        .filter_map(|n| n.strip_prefix("cpu").and_then(|d| d.parse().ok()))
        .collect();
    let online: Option<Vec<u32>> = root
        .read_trimmed(&format!("{CPU}/online"))
        .ok()
        .and_then(|v| cpu_list(&v));
    let is_online = |cpu: u32| online.as_ref().is_none_or(|list| list.contains(&cpu));

    let mut notes = Vec::new();
    let mut logical = Vec::new();
    let mut offline = Vec::new();
    for cpu in &present {
        ctx.check()?;
        if !is_online(*cpu) {
            offline.push(*cpu);
            continue;
        }
        let base = format!("{CPU}/cpu{cpu}");
        let topo = format!("{base}/topology");
        let siblings = root
            .read_trimmed(&format!("{topo}/core_cpus_list"))
            .or_else(|_| root.read_trimmed(&format!("{topo}/thread_siblings_list")))
            .ok()
            .and_then(|v| cpu_list(&v));
        // A cluster id of 65535 (u16::MAX) or -1 means "not applicable".
        let cluster =
            read_u32(format!("{topo}/cluster_id")).filter(|v| *v != 65535 && *v != u32::MAX);
        logical.push(Logical {
            cpu: *cpu,
            package: read_u32(format!("{topo}/physical_package_id")),
            die: read_u32(format!("{topo}/die_id")),
            core_id: read_u32(format!("{topo}/core_id")),
            cluster,
            siblings,
            capacity: read_u32(format!("{base}/cpu_capacity")),
            max_khz: read_u64(format!("{base}/cpufreq/cpuinfo_max_freq")),
            min_khz: read_u64(format!("{base}/cpufreq/cpuinfo_min_freq")),
        });
    }
    if logical.is_empty() {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["no online logical CPUs were found in sysfs".into()],
        ));
    }
    let topology_known = logical
        .iter()
        .all(|l| l.siblings.is_some() && l.core_id.is_some());
    if !topology_known {
        notes.push(
            "CPU topology is incomplete in sysfs; core counts are not reported rather than guessed"
                .into(),
        );
    }
    if !offline.is_empty() {
        notes.push(format!("{} logical CPU(s) are offline", offline.len()));
    }

    // Identity from /proc/cpuinfo, keyed by logical CPU number.
    let cpuinfo_entries = root
        .read("proc/cpuinfo")
        .map(|text| cpuinfo::parse(&text))
        .unwrap_or_default();
    if cpuinfo_entries.is_empty() {
        notes.push("/proc/cpuinfo is not readable".into());
    }
    let by_cpu: BTreeMap<u32, &cpuinfo::CpuInfoEntry> = cpuinfo_entries
        .iter()
        .filter_map(|e| e.processor.map(|p| (p, e)))
        .collect();

    let kinds = core_kinds(ctx, &logical);
    let arch = architecture(ctx);

    let mut packages: BTreeMap<u32, Vec<&Logical>> = BTreeMap::new();
    for l in &logical {
        packages.entry(l.package.unwrap_or(0)).or_default().push(l);
    }

    let mut processors = Vec::new();
    for (package_id, members) in packages {
        ctx.check()?;
        let first = members.iter().find_map(|l| by_cpu.get(&l.cpu).copied());
        let cpuinfo_src = "/proc/cpuinfo";
        let text = |value: Option<&String>, what: &str| match value {
            Some(v) => Field::detected(v.clone(), cpuinfo_src, Confidence::KernelReported),
            None => Field::missing(Availability::Unavailable, Some(cpuinfo_src.into()))
                .with_note(format!("{what} is not listed in /proc/cpuinfo")),
        };
        let num = |value: Option<u32>, what: &str| match value {
            Some(v) => Field::detected(v, cpuinfo_src, Confidence::KernelReported),
            None => Field::missing(Availability::Unavailable, Some(cpuinfo_src.into()))
                .with_note(format!("{what} is not listed in /proc/cpuinfo")),
        };

        let vendor_id = first.and_then(|e| e.vendor_id.as_ref());
        let vendor = match (vendor_id, first.and_then(|e| e.implementer)) {
            (Some(id), _) => match cpuinfo::vendor_name(id) {
                Some(name) => {
                    Field::detected(name.to_string(), cpuinfo_src, Confidence::KernelReported)
                }
                None => Field::detected(id.clone(), cpuinfo_src, Confidence::KernelReported),
            },
            (None, Some(code)) => match cpuinfo::arm_implementer_name(code) {
                Some(name) => Field::detected(name.to_string(), cpuinfo_src, Confidence::Inferred)
                    .with_note(format!("from ARM implementer code {code:#04x}")),
                None => Field::missing(Availability::Unknown, Some(cpuinfo_src.into())),
            },
            _ => Field::missing(Availability::Unavailable, Some(cpuinfo_src.into())),
        };
        let family = first.and_then(|e| e.family);
        let model = first.and_then(|e| e.model);
        let microarchitecture = match (vendor_id, family, model) {
            (Some(v), Some(f), Some(m)) => match microarch::lookup(v, f, m) {
                Some(name) => Field::detected(
                    name.to_string(),
                    "CPUID family/model table",
                    Confidence::Inferred,
                )
                .with_note("derived from family and model numbers"),
                None => Field::missing(Availability::Unknown, None)
                    .with_note("this family/model is not in Sentinel's table"),
            },
            _ => Field::missing(Availability::Unknown, None),
        };

        // Cores: group logical CPUs by their sibling set.
        let mut groups: BTreeMap<Vec<u32>, Vec<&Logical>> = BTreeMap::new();
        if topology_known {
            for l in &members {
                let mut key = l.siblings.clone().unwrap_or_default();
                key.retain(|c| members.iter().any(|m| m.cpu == *c));
                if key.is_empty() {
                    key.push(l.cpu);
                }
                groups.entry(key).or_default().push(l);
            }
        }
        let mut cores: Vec<CpuCore> = groups
            .into_iter()
            .map(|(cpus, threads)| {
                let lead = threads[0];
                CpuCore {
                    index: 0,
                    core_id: lead.core_id.unwrap_or(0),
                    die_id: lead.die,
                    cluster_id: lead.cluster,
                    kind: kinds.get(&lead.cpu).copied().unwrap_or(CoreKind::Unknown),
                    max_frequency_khz: threads.iter().filter_map(|t| t.max_khz).max(),
                    logical_cpus: cpus,
                }
            })
            .collect();
        cores.sort_by_key(|c| c.logical_cpus.first().copied());
        for (i, core) in cores.iter_mut().enumerate() {
            core.index = i as u32;
        }

        let topo_src = format!("/{CPU}/cpu*/topology");
        let physical_cores = if topology_known {
            Field::detected(
                cores.len() as u32,
                topo_src.clone(),
                Confidence::KernelReported,
            )
        } else {
            Field::missing(Availability::Unavailable, Some(topo_src.clone()))
        };
        let logical_processors = Field::detected(
            members.len() as u32,
            format!("/{CPU}/online"),
            Confidence::KernelReported,
        );

        let hybrid = if kinds
            .values()
            .any(|k| matches!(k, CoreKind::Performance | CoreKind::Efficiency))
        {
            Field::detected(true, "core types", Confidence::KernelReported)
        } else if kinds.values().all(|k| *k == CoreKind::Uniform) {
            Field::detected(false, "core types", Confidence::Inferred)
        } else {
            Field::missing(Availability::Unknown, None)
        };

        let freq_src = format!("/{CPU}/cpu*/cpufreq");
        let max = members.iter().filter_map(|l| l.max_khz).max();
        let min = members.iter().filter_map(|l| l.min_khz).min();
        let freq = |v: Option<u64>| {
            match v {
            Some(khz) => Field::detected(khz, freq_src.clone(), Confidence::KernelReported)
                .with_note("the frequency driver's current limit; it can change with boost and power settings"),
            None => Field::missing(Availability::Unsupported, Some(freq_src.clone()))
                .with_note("no cpufreq driver exposes frequency limits"),
        }
        };

        let flags = match first.map(|e| e.flags.clone()).filter(|f| !f.is_empty()) {
            Some(flags) => Field::detected(flags, cpuinfo_src, Confidence::KernelReported),
            None => Field::missing(Availability::Unavailable, Some(cpuinfo_src.into())),
        };

        processors.push(ProcessorInfo {
            discovered_at: ctx.now,
            package_id,
            vendor,
            vendor_id: text(vendor_id, "vendor_id"),
            model_name: text(first.and_then(|e| e.model_name.as_ref()), "model name"),
            family: num(family, "cpu family"),
            model: num(model, "model"),
            stepping: num(first.and_then(|e| e.stepping), "stepping"),
            microarchitecture,
            architecture: arch.clone(),
            physical_cores,
            logical_processors,
            offline_cpus: offline.clone(),
            caches: caches(ctx, &members),
            cores,
            flags,
            min_frequency_khz: freq(min),
            max_frequency_khz: freq(max),
            hybrid,
        });
    }

    let outcome = if notes.is_empty() {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    };
    let count = processors.len();
    Ok(ModuleRun::new(processors, outcome, count, notes))
}

/// Core kind per logical CPU.
fn core_kinds(ctx: &DiscoveryContext, logical: &[Logical]) -> BTreeMap<u32, CoreKind> {
    let list = |rel: &str| ctx.root.read_trimmed(rel).ok().and_then(|v| cpu_list(&v));
    if let (Some(p), Some(e)) = (
        list("sys/devices/cpu_core/cpus"),
        list("sys/devices/cpu_atom/cpus"),
    ) {
        return logical
            .iter()
            .map(|l| {
                let kind = if p.contains(&l.cpu) {
                    CoreKind::Performance
                } else if e.contains(&l.cpu) {
                    CoreKind::Efficiency
                } else {
                    CoreKind::Unknown
                };
                (l.cpu, kind)
            })
            .collect();
    }
    let capacities: BTreeSet<u32> = logical.iter().filter_map(|l| l.capacity).collect();
    if capacities.len() > 1 && logical.iter().all(|l| l.capacity.is_some()) {
        let top = *capacities.iter().max().unwrap_or(&0);
        return logical
            .iter()
            .map(|l| {
                let kind = if l.capacity == Some(top) {
                    CoreKind::Performance
                } else {
                    CoreKind::Efficiency
                };
                (l.cpu, kind)
            })
            .collect();
    }
    logical.iter().map(|l| (l.cpu, CoreKind::Uniform)).collect()
}

/// `(level, kind) -> (size, distinct sets of CPUs sharing an instance)`.
type CacheGroups = BTreeMap<(u8, u8), (Option<u64>, BTreeSet<Vec<u32>>)>;

fn caches(ctx: &DiscoveryContext, members: &[&Logical]) -> Vec<CacheInfo> {
    // (level, kind) -> (size, distinct sharing sets)
    let mut found: CacheGroups = BTreeMap::new();
    for l in members {
        let base = format!("{CPU}/cpu{}/cache", l.cpu);
        for index in ctx.root.list(&base).unwrap_or_default() {
            if !index.starts_with("index") {
                continue;
            }
            let dir = format!("{base}/{index}");
            let read = |name: &str| ctx.root.read_trimmed(&format!("{dir}/{name}")).ok();
            let Some(level) = read("level").and_then(|v| v.parse::<u8>().ok()) else {
                continue;
            };
            let kind = match read("type").as_deref() {
                Some("Data") => 0,
                Some("Instruction") => 1,
                Some("Unified") => 2,
                _ => 3,
            };
            let size = read("size").and_then(|v| cache_size(&v));
            let shared = read("shared_cpu_list")
                .and_then(|v| cpu_list(&v))
                .unwrap_or_else(|| vec![l.cpu]);
            let entry = found
                .entry((level, kind))
                .or_insert((size, BTreeSet::new()));
            entry.0 = entry.0.or(size);
            entry.1.insert(shared);
        }
    }
    found
        .into_iter()
        .map(|((level, kind), (size, sets))| CacheInfo {
            instances: sets.len() as u32,
            shared_by: sets.iter().next().cloned().unwrap_or_default(),
            instance_cpus: sets.into_iter().collect(),
            level,
            kind: match kind {
                0 => CacheKind::Data,
                1 => CacheKind::Instruction,
                2 => CacheKind::Unified,
                _ => CacheKind::Other,
            },
            size_bytes: size,
        })
        .collect()
}
