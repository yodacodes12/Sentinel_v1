//! System identity: manufacturer, model, kernel and distribution.
//!
//! Sources: `/sys/class/dmi/id/*` (firmware), `/proc/sys/kernel/*` (kernel),
//! `/etc/os-release` with `/usr/lib/os-release` as fallback.

use sentinel_inventory::{Availability, Confidence, Field, ModuleOutcome, OsInfo, SystemIdentity};

use crate::context::{DiscoveryContext, ModuleRun, outcome_for};
use crate::fields;
use crate::parsers::{classes, key_values};

pub const DMI: &str = "sys/class/dmi/id";
const OS_RELEASE: &[&str] = &["etc/os-release", "usr/lib/os-release"];

pub fn dmi(ctx: &DiscoveryContext, name: &str) -> Field<String> {
    fields::firmware_text(&ctx.root, &format!("{DMI}/{name}"))
}

pub fn architecture(ctx: &DiscoveryContext) -> Field<String> {
    let field = fields::text(
        &ctx.root,
        "proc/sys/kernel/arch",
        Confidence::KernelReported,
    );
    if field.is_detected() || !ctx.root.is_host() {
        return field;
    }
    // Older kernels lack /proc/sys/kernel/arch; the build target of this
    // program is then the best remaining evidence.
    Field::detected(
        std::env::consts::ARCH.to_string(),
        "Sentinel build target",
        Confidence::Inferred,
    )
    .with_note("this kernel does not expose /proc/sys/kernel/arch")
}

pub fn discover(ctx: &DiscoveryContext) -> ModuleRun<Option<SystemIdentity>> {
    let has_dmi = ctx.root.is_dir(DMI);
    let chassis = match ctx.root.read_trimmed(&format!("{DMI}/chassis_type")) {
        Ok(raw) => match raw.parse::<u32>().ok().and_then(classes::chassis_type) {
            Some(kind) => Field::detected(
                kind,
                ctx.root.display(&format!("{DMI}/chassis_type")),
                Confidence::FirmwareReported,
            ),
            None => Field::missing(Availability::Invalid, Some(ctx.root.display(DMI))).with_note(
                format!("firmware chassis type {raw} is \"other\" or \"unknown\""),
            ),
        },
        Err(e) => fields::missing(&ctx.root, &format!("{DMI}/chassis_type"), &e),
    };

    let identity = SystemIdentity {
        discovered_at: ctx.now,
        manufacturer: dmi(ctx, "sys_vendor"),
        product_name: dmi(ctx, "product_name"),
        product_version: dmi(ctx, "product_version"),
        product_family: dmi(ctx, "product_family"),
        product_sku: dmi(ctx, "product_sku"),
        chassis,
        architecture: architecture(ctx),
        kernel_release: fields::text(
            &ctx.root,
            "proc/sys/kernel/osrelease",
            Confidence::KernelReported,
        ),
        kernel_version: fields::text(
            &ctx.root,
            "proc/sys/kernel/version",
            Confidence::KernelReported,
        ),
        os: os_info(ctx),
    };

    let mut notes = Vec::new();
    if !has_dmi {
        notes.push(
            "No DMI/SMBIOS information (/sys/class/dmi/id is missing). This is normal on many \
             ARM systems and some virtual machines."
                .to_string(),
        );
    }
    let key = [
        identity.manufacturer.is_detected(),
        identity.product_name.is_detected(),
        identity.architecture.is_detected(),
        identity.kernel_release.is_detected(),
        identity.os.name.is_detected(),
    ];
    for (ok, what) in key.iter().zip([
        "manufacturer",
        "product name",
        "architecture",
        "kernel release",
        "distribution",
    ]) {
        if !ok {
            notes.push(format!("{what} not available"));
        }
    }
    let missing = key.iter().filter(|ok| !**ok).count();
    let outcome = if missing == key.len() {
        ModuleOutcome::Unavailable
    } else {
        outcome_for(missing)
    };
    ModuleRun::new(Some(identity), outcome, 1, notes)
}

fn os_info(ctx: &DiscoveryContext) -> OsInfo {
    let found = OS_RELEASE.iter().find_map(|rel| {
        ctx.root
            .read(rel)
            .ok()
            .map(|text| (*rel, key_values(&text)))
    });
    let Some((rel, values)) = found else {
        let none = || {
            Field::missing(Availability::Unsupported, Some("/etc/os-release".into()))
                .with_note("no os-release file found")
        };
        return OsInfo {
            name: none(),
            pretty_name: none(),
            id: none(),
            id_like: Vec::new(),
            version: none(),
            version_id: none(),
        };
    };
    let source = ctx.root.display(rel);
    let get = |key: &str| match values.get(key).filter(|v| !v.trim().is_empty()) {
        Some(value) => Field::detected(value.clone(), source.clone(), Confidence::Direct),
        None => Field::missing(Availability::Unavailable, Some(source.clone()))
            .with_note(format!("{key} is not set")),
    };
    OsInfo {
        name: get("NAME"),
        pretty_name: get("PRETTY_NAME"),
        id: get("ID"),
        id_like: values
            .get("ID_LIKE")
            .map(|v| v.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        version: get("VERSION"),
        version_id: get("VERSION_ID"),
    }
}
