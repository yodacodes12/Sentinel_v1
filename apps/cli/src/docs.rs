//! `sentinel docs`: motherboard documentation discovery (Phase 6).

use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use sentinel_client::Sentinel;
use sentinel_core::Result;
use sentinel_docs::{DocumentsView, MatchClass, RetrievalStatus, SourceTrust};

use crate::inventory::json;

pub fn class_label(c: MatchClass) -> &'static str {
    match c {
        MatchClass::Exact => "EXACT",
        MatchClass::HighConfidence => "HIGH",
        MatchClass::RelatedModel => "RELATED",
        MatchClass::Possible => "POSSIBLE",
        MatchClass::Unverified => "UNVERIFIED",
        MatchClass::Mismatch => "MISMATCH",
    }
}

fn trust_label(t: SourceTrust) -> &'static str {
    match t {
        SourceTrust::OfficialThisManufacturer => "official",
        SourceTrust::OfficialOtherManufacturer => "official (other manufacturer)",
        SourceTrust::Community => "community",
        SourceTrust::UserProvided => "your library",
        SourceTrust::Unverified => "unverified source",
    }
}

fn print(v: &DocumentsView, all: bool) {
    match &v.identity {
        Some(id) => {
            let m = |x: &Option<sentinel_docs::IdentityValue>| {
                x.as_ref()
                    .map(|v| v.original.clone())
                    .unwrap_or_else(|| "not reported".into())
            };
            println!(
                "Motherboard  {} · {} · board {} rev {}",
                id.manufacturer
                    .as_ref()
                    .map(|m| m.original.as_str())
                    .unwrap_or("manufacturer not reported"),
                m(&id.system_model),
                m(&id.board_name),
                m(&id.board_revision)
            );
            println!("Identity     {:?}", id.completeness);
        }
        None => println!(
            "Motherboard  {}",
            v.identity_problem.as_deref().unwrap_or("not identified")
        ),
    }
    println!(
        "Online       {}",
        if v.online {
            "on"
        } else {
            "off (only your document library is searched)"
        }
    );
    println!("Library      {}", v.library_dir);
    if v.progress.running {
        println!("Running      {:?} {:?}", v.progress.kind, v.progress.stage);
    }
    if let Some(n) = &v.notice {
        println!("Note         {n}");
    }
    let shown: Vec<_> = v
        .candidates
        .iter()
        .filter(|c| all || c.assessment.class != MatchClass::Mismatch)
        .collect();
    if shown.is_empty() {
        println!("\nNo documents found.");
    } else {
        println!();
        for c in shown {
            let state = match &c.retrieval {
                RetrievalStatus::Downloaded { .. } => "downloaded",
                RetrievalStatus::InLibrary { .. } => "in library",
                RetrievalStatus::ReferenceOnly { .. } => "reference only",
                RetrievalStatus::Failed { .. } => "download failed",
                RetrievalStatus::NotDownloaded => "not downloaded",
            };
            println!(
                "{:>5}  {:<10} {:<22} {:<12} {}  [{}]",
                c.id,
                class_label(c.assessment.class),
                format!("{:?}", c.doc_type).to_lowercase(),
                state,
                c.title,
                trust_label(c.source.trust)
            );
        }
    }
    let hidden = v
        .candidates
        .iter()
        .filter(|c| c.assessment.class == MatchClass::Mismatch)
        .count();
    if !all && hidden > 0 {
        println!("({hidden} document(s) for other hardware hidden; --all shows them)");
    }
}

pub fn show(s: &Sentinel, all: bool, as_json: bool) -> Result<ExitCode> {
    let v = s.documents()?;
    if as_json {
        json(&v)?;
    } else {
        print(&v, all);
    }
    Ok(ExitCode::SUCCESS)
}

pub fn search(s: &Sentinel, refresh: bool, no_wait: bool, as_json: bool) -> Result<ExitCode> {
    let mut v = s.start_document_discovery(refresh)?;
    let deadline = Instant::now() + Duration::from_secs(600);
    while !no_wait && v.progress.running && Instant::now() < deadline {
        sleep(Duration::from_millis(500));
        v = s.documents()?;
    }
    if as_json {
        json(&v)?;
    } else {
        print(&v, false);
    }
    Ok(ExitCode::SUCCESS)
}

pub fn act(v: Result<DocumentsView>) -> Result<ExitCode> {
    let v = v?;
    if let Some(n) = &v.notice {
        println!("{n}");
    }
    Ok(ExitCode::SUCCESS)
}
