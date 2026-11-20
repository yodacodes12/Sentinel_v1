//! Phase 6: document discovery through the real daemon and CLI, offline
//! (the default). The hardware is the AMD laptop fixture: Example Corp
//! "Example Book 14 AMD", board EXB14-A revision A01.

use sentinel_docs::{MatchClass, RetrievalStatus, SessionStatus, SourceTrust};
use sentinel_tests::Sandbox;
use sentinel_tests::hwfixture;

fn sandbox(fixture: &sentinel_tests::hwfixture::HwFixture) -> Sandbox {
    Sandbox::new().with_env("SENTINEL_DISCOVERY_ROOT", fixture.root())
}

#[test]
fn offline_discovery_matches_library_documents_and_survives_restarts() {
    let fixture = hwfixture::amd_laptop();
    let sb = sandbox(&fixture);
    sb.cli(&["start"]).expect_code(0);
    // Before a scan there is no identity, and a search is refused.
    let v = sb.client().documents().unwrap();
    assert!(v.identity.is_none() && v.identity_problem.unwrap().contains("hardware scan"));
    assert!(sb.client().start_document_discovery(false).is_err());

    sb.cli(&["scan", "--hardware"]).expect_code(0);
    let library = sb.paths().data_dir.join("documents/library");
    assert!(library.is_dir(), "the daemon creates the library folder");
    std::fs::write(library.join("exb14-a.pdf"), b"%PDF-1.4\n<< /Title (Example Corp Example Book 14 AMD service manual, board EXB14-A rev A01) >>").unwrap();
    std::fs::write(
        library.join("other-vendor.pdf"),
        b"%PDF-1.4\n<< /Title (Lenovo ThinkPad T480 schematic) >>",
    )
    .unwrap();
    std::fs::write(library.join("installer.pdf"), b"\x7fELF not a document").unwrap();

    let out = sb.cli(&["docs", "search", "--json"]).expect_code(0).json();
    let view: sentinel_docs::DocumentsView = serde_json::from_value(out).unwrap();
    let id = view.identity.as_ref().unwrap();
    assert_eq!(id.board_name.as_ref().unwrap().original, "EXB14-A");
    assert_eq!(id.board_revision.as_ref().unwrap().original, "A01");
    assert!(!view.online);
    assert_eq!(
        view.candidates.len(),
        2,
        "the executable is not a document: {:?} running={}",
        view.candidates.iter().map(|c| &c.title).collect::<Vec<_>>(),
        view.progress.running
    );
    let exact = &view.candidates[0];
    assert_eq!(exact.assessment.class, MatchClass::Exact);
    assert_eq!(exact.source.trust, SourceTrust::UserProvided);
    assert!(matches!(exact.retrieval, RetrievalStatus::InLibrary { .. }));
    assert_eq!(view.candidates[1].assessment.class, MatchClass::Mismatch);
    assert_eq!(view.sessions[0].status, SessionStatus::Completed);
    assert!(
        view.sessions[0]
            .providers
            .iter()
            .filter(|p| p.provider_id != "local_library")
            .all(|p| format!("{:?}", p.state) == "SkippedOffline")
    );

    // Human output hides other hardware by default.
    sb.cli(&["docs"])
        .expect_code(0)
        .expect_stdout("EXACT")
        .expect_stdout("hidden");

    // Downloads are refused while offline, and a library file needs none.
    assert!(sb.client().download_document(exact.id).is_err());

    // Results and history survive a daemon restart; a fresh cache is reused.
    sb.cli(&["stop"]).expect_code(0);
    sb.cli(&["start"]).expect_code(0);
    let again = sb.client().start_document_discovery(false).unwrap();
    assert!(again.notice.unwrap().contains("still current"));
    assert_eq!(again.candidates.len(), 2);
    assert_eq!(again.sessions.len(), 1);

    // Clearing forgets results; nothing in the library is touched.
    sb.cli(&["docs", "clear"]).expect_code(0);
    assert!(sb.client().documents().unwrap().candidates.is_empty());
    assert!(library.join("exb14-a.pdf").is_file());
    sb.cli(&["stop"]).expect_code(0);
}
