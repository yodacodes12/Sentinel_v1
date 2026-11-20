# Phase 6: Automatic schematic discovery and verification

Status: **COMPLETE** for the definition of done (§16 of the plan), with real-world coverage
limited by the sources available (see 16–18). Phase 7 was not started. Design and methodology:
[DOCUMENT_DISCOVERY.md](DOCUMENT_DISCOVERY.md).

Statuses: **Fully implemented** · **Partially implemented** · **Not implemented** · **Blocked by
missing hardware information** · **Blocked by external source availability**. Validation:
**Physically validated** (real machine, real window or real service), **Mock tested**.

## 1. Documentation and roadmap reviewed

`plan.md` (the concept and roadmap sections of Phases 1–6), `docs/PHASE_1.md`–`PHASE_5.md`,
`HARDWARE_DISCOVERY.md`, `SYSTEM_INVENTORY.md`, `HARDWARE_EXPLORER.md`, `ARCHITECTURE.md`,
`SAFETY_POLICY.md`, `SECURITY.md`. Phase 6 scope: discover, classify, verify and store
documentation for the detected motherboard; no parsing (Phase 7).

## 2. Existing architecture inspected (Stage 1 audit)

- **Board identity:** Phase 2's `BoardIdentity` was designed as "the contract for future
  schematic discovery". It holds system and board manufacturer, model, family, SKU, board name
  and revision, and BIOS, each a provenance `Field`, with no serials. Its identification level
  is board-identified, system-only or unidentified.
- **Stable component IDs:** from Phase 5 (`board`, `pci:…`, `usb:…`).
- **Persistence:** `sentinel.db`, which has ordered migrations and a scan cache.
- **Configuration:** a versioned TOML file, and sections can be added with defaults.
- **Jobs:** the daemon has a background-job pattern (the scan coordinator).
- **Launching:** the client launcher, which uses `gio`.
- **Network:** there was **no network infrastructure**, and the safety suite banned network
  code outright. Phase 6 therefore needed a deliberate, confined exception.
- **Gaps found:**
  - no HTTP client (resolved from the local crate cache, see 5);
  - firmware's `product_version` is the BIOS version on Dell (handled in normalisation);
  - Lenovo reports the machine type as the product name (handled);
  - the optional systemd unit forbids non-Unix sockets (left hardened, see 18).

## 3. Phase 5 dependencies and their actual status

| Dependency | Status |
|---|---|
| Reliable manufacturer identification | Present (DMI), physically validated on the test laptop: `Dell Inc.` |
| Model and product identifiers | Present: `Inspiron 15 3535`, board `0N90HF` |
| Revision where detectable | Present: board `A02` (the `product_version` 1.32.0 is the BIOS version and is correctly ignored) |
| Source and reliability metadata | Present (`Field` provenance, Phase 2/5) |
| Stable hardware identity model | Present (Phase 5 `ComponentIndex`, `board` ID); Phase 5 is COMPLETE |
| Phase 4 | PARTIALLY COMPLETE (suspend, discharge, other vendors/desktops). None of its open items blocks Phase 6 |

## 4. New modules and files

| Area | Files |
|---|---|
| Discovery crate `crates/sentinel-docs` | `model.rs`, `identity.rs`, `vendors.rs`, `query.rs`, `classify.rs`, `trust.rs`, `matching.rs`, `provider.rs`, `providers/{local,catalogue,archive}.rs`, `engine.rs`, `net.rs` (the only network module), `download.rs`; `tests/empty_inventory.json` |
| Storage | `crates/sentinel-storage/src/docs.rs` (migration 3 records) |
| Daemon | `crates/sentinel-daemon/src/documents.rs` (coordinator) |
| CLI | `apps/cli/src/docs.rs` (`sentinel docs …`) |
| Frontend | `types/documents.ts`, `app/documentsView.ts`, `hooks/useDocuments.ts`, `pages/SchematicLibraryPage.tsx`, `app/documents.test.tsx`, `test-fixtures/documents-view.json` (written by Rust) |
| Tests | `tests/integration/documents.rs`, `tests/desktop-e2e/docs_scenarios.py` |
| Docs | `docs/DOCUMENT_DISCOVERY.md`, this file |

## 5. Existing modules modified

- **Workspace:**
  - `Cargo.toml` gains `ureq` 3.5 (rustls/ring with the system trust store, no default
    features), `sha2` and `url`. Everything was already in the local cargo cache; nothing was
    downloaded.
  - `Cargo.lock` gained the corresponding entries.
- **Config:** `DocumentSettings` with defaults, validated. `ConfigPatch.documents` is boxed.
  No config version bump: a version-2 file without the section loads with the defaults.
- **Storage:** migration 3. The Phase 1 upgrade test now asserts "upgraded to the latest
  migration" rather than a hard-coded 2.
- **IPC:** six requests and the `Documents` response; the `Busy` and `NotFound` error codes.
- **Daemon:**
  - the context gets `documents`;
  - startup creates the document folders and the coordinator, and marks interrupted sessions;
  - shutdown cancels a running job;
  - the handler routes the new requests.
- **Client:** document methods; `open_document` with re-validation; `OpenTarget`.
- **Launcher:** `open_with_default_application`, which uses `gio open -- <target>` for https
  addresses and checked absolute paths only.
- **Tauri:** seven commands.
- **Frontend:**
  - the Schematic Library is marked implemented in the catalogue;
  - `App` routing;
  - a Settings section;
  - a board-documentation summary in the Explorer's Motherboard category;
  - bridge calls and types.
- **Safety suite:**
  - the command and request pins are extended;
  - `ureq` and `rustls` are added to the forbidden list, exempt only in `net.rs`;
  - a new test, `network_code_is_confined_to_document_discovery`;
  - the socket test now also runs a (default, offline) document search.
- **Phase 4 test scripts:** the "Schematic Library" entry lost its "DEV" tag; the heading check
  was updated accordingly.

## 6. Motherboard identity fields supported

- **Used:**
  - system manufacturer (with the board manufacturer as fallback);
  - board manufacturer;
  - system model, with the Lenovo machine-type substitution;
  - system family;
  - board name, when distinctive;
  - board revision, unless it equals the BIOS version.

  Originals are always kept, next to a normalised key and tokens, a completeness level, notes
  and an identity hash.
- **Never used:** serial numbers, UUIDs, SKU, BIOS version and date, the fingerprint.
- **PCI identifiers:** not used for search, because PCI IDs identify chips, not boards. They
  remain available for Phase 7.

## 7. Discovery providers implemented

| Provider | Status | Validation |
|---|---|---|
| Document library (offline) | Fully implemented | **Physically validated**: real window and real daemon on the laptop's own identity, with a synthetic PDF; no socket opened |
| Internet Archive public search | Fully implemented (references only) | **Physically validated**: live API, generic identity (ThinkPad T480): the archive's "ThinkPad T480 Hardware Maintenance Manual" classed high-confidence; real-window search through the daemon with the fictitious fixture identity (5 results, 4–6 s) |
| Documentation catalogues (JSON), from HTTPS addresses or `*.sentinel-catalogue.json` files in the library | Fully implemented | **Physically validated**: live HTTPS fetch of a configured address through the daemon (rejected correctly as not a catalogue); a local catalogue's official entry downloaded through the window and daemon (see 14). **A valid catalogue served over real HTTPS** (a public echo endpoint returning a one-entry catalogue for Lenovo's official T480 PDF) was fetched by the daemon (2 requests, robots.txt honoured), its entry classed high-confidence from the manufacturer's site, and downloaded and removed from the window. **Physically validated** end to end from a catalogue entry for the manufacturer's own document: `live_official_manufacturer_download` (generic identity ThinkPad T480, Lenovo's official PSREF PDF on `psref.lenovo.com`): classed high-confidence, trust "manufacturer site", downloadable, then really downloaded (35 742 bytes, robots.txt honoured, 2 requests), content-checked as PDF, hashed, verified in storage, re-download de-duplicated, size cap enforced |
| Manufacturer portals | Not implemented: **blocked by external source availability** (JavaScript/accounts; terms do not allow automated searching) | — |
| Authorised repair providers | Not implemented: blocked (accounts or paid access) | — |

## 8. Document formats recognised

- **Types:** schematic, boardview, service manual, hardware manual, specification sheet,
  firmware documentation, unclassified.
- **Formats:**
  - PDF (`%PDF-`);
  - ZIP (central directory inspected, never extracted);
  - boardview by extension (`.brd .bdv .bv .bvr .fz .cad .asc .tvw .gr .cst .f2b .bvr3`);
  - web page;
  - unknown.
- **Basis:** each format records whether it came from the content, the extension or
  `Content-Type`.
- **Rejected:** HTML in place of a document, executables, scripts, empty or unrecognised
  content.

## 9. Verification and confidence methodology

The match is decided by rules from evidence. Each evidence item records the signal, its effect
(supports, contradicts or neutral), where it was found, the detected and found values, its
points and a note.

- **Supporting points:** exact model +6, board ID +6, model number +3, revision +2,
  manufacturer +2, family +1.
- **Contradicting points:** another manufacturer −5, another model number −5, another product
  −5, other boards or models in a catalogue −5, another revision −3.
- **Critical contradictions** prevent any exact or high-confidence class, whatever the point
  total.
- **Missing information** adds nothing and is listed.
- **Revision:** a document is exact only if firmware reports a revision and the document states
  the same one.
- **Trust:** source trust is a separate dimension, taken from the host.

The full tables are in DOCUMENT_DISCOVERY.md §6.

## 10. Source provenance implementation

Each candidate carries a `SourceInfo`:

- provider;
- source name;
- domain;
- page and document addresses;
- category (manufacturer, authorised, community, user library, unknown);
- trust (manufacturer site, other manufacturer, community, your library, unverified) with the
  reason;
- access status (downloadable, reference only with the reason, local file);
- usage note.

The candidate also records its discovery time, last verification time and retrieval status.
Trust comes only from the host, checked against Sentinel's domain table, never from a page's or
a catalogue's claims.

## 11. Download and storage security protections

The full list is in DOCUMENT_DISCOVERY.md §7. Each protection below is tested:

- **Addresses:**
  - HTTPS on port 443 only;
  - no credentials, spaces or control characters in addresses;
  - no private, loopback, link-local, metadata, carrier-NAT, reserved or local-name targets,
    refused before the request and again at DNS-resolution time;
  - every redirect re-validated, at most 4.
- **Fetching:** robots.txt honoured; per-host rate limiting; HTTP 429 stops that provider for
  the session.
- **Size and cleanup:** bodies are size-capped, and interrupted or cancelled downloads leave no
  partial files.
- **Content checks:**
  - content sniffing rejects executables and web pages;
  - ZIP archives are refused if any entry could escape a folder or is a link or a non-document
    type, and are never extracted.
- **Storage:**
  - files are stored as `<sha256>.<ext>` with mode 0600, in a folder with mode 0700;
  - a file is never overwritten;
  - duplicates are reused.
- **Opening:**
  - stored files are re-verified (name, regular file, folder, hash, PDF type) before opening;
  - nothing is opened, extracted or run automatically;
  - downloads happen only when asked (the automatic option is off by default and limited to
    the manufacturer's own site).

## 12. Database changes and migrations

Migration 3 adds `doc_identities`, `doc_sessions` (indexed per identity, at most 50 per
identity) and `doc_candidates` (unique per identity and key). Documents stay on disk.

Tested:

- migration from a Phase 5 database, with data preserved;
- read-only clients of a database that hasn't been migrated yet;
- de-duplication;
- first discovery time kept across searches;
- verification results updated;
- cache expiry and clearing, with downloaded files kept;
- bounded history;
- interrupted sessions after a restart.

## 13. Frontend features added

- **Schematic Library page:**
  - **Overview:** manufacturer, model, board, revision and identification reliability, plus
    identity notes, last discovery and online/offline status.
  - **Controls:** search (reuses current results), refresh, cancel, clear results, a link to
    the discovery settings.
  - **Progress:** the seven real stages; no percentages.
  - **Sources:** each provider's status and last outcome, and previous sessions.
  - **Results:** grouped by match class, with visually distinct styles; mismatches folded away.
  - **Detail panel:** what matched, what contradicts it, missing information, revision, the
    source shown separately, file metadata, retrieval, and Open source page / Open local copy /
    Download / Remove actions.
  - **Empty and failure states:** service off, no motherboard, incomplete identity, not
    searched yet, no providers, cancelled, every provider failed, none found online or offline,
    only related documents, no exact matches, provider unavailable, download unavailable.
- **Explorer:** a board-documentation summary in the Motherboard category.
- **Settings:** a Documentation discovery section.
- **Polling:** the page polls only while a job runs, and only while it is open.

## 14. Tests executed and their results

| Suite | Result |
|---|---|
| Rust workspace (`cargo test --workspace`) | **461 passed, 0 failed**, 4 ignored (three live network tests, one fixture-export helper); 38 test binaries. Phase 5 ended at 385; 74 new |
| — of which `sentinel-docs` | 62 passed (identity, queries, sanitisation, vendors, classification, archives, PDF titles, trust, matching (13 scenarios incl. misleading names, official-but-wrong, community-but-right, conflicting revision, catalogue claims), URL/address checks, robots, redirects, rate limiting, 429, timeouts, cancellation, bounded bodies, providers success/empty/failure/offline/invalid/duplicates, engine, downloads, stored-file verification, frontend fixture currency) |
| — storage / config / daemon / integration / safety | 15 (5 new) · 18 (1 new) · coordinator 4 new · integration 43 (1 new) · safety 21 (1 new, 3 pins extended) |
| Live (`cargo test -p sentinel-docs -- --ignored live`) | 3/3 passed: HTTPS through the public-only resolver with the system trust store; Internet Archive search with a generic model; a real download of a manufacturer's official PDF through every download check |
| Frontend (`npm test`) | **107 passed, 0 failed** (93 from earlier phases, 14 new) |
| fmt, clippy `-D warnings`, `tsc --noEmit` | Clean |
| Real window, Schematic Library (`docs_scenarios.py --online --real-download --web-catalogue <url>`) | **8/8 on three consecutive full runs, and 5/5 on four further offline runs**, after the race fix below. Covered: the offline library search on this computer's identity (no socket opened); an online Internet Archive search with a fictitious identity; a download clicked in the window of Lenovo's official PSREF PDF from a local catalogue (ThinkPad T480 fixture), 35 742 bytes from psref.lenovo.com, stored as `<sha256>.pdf` with mode 0600, then removed; a live non-catalogue HTTPS address reported as "not a document catalogue"; and a valid catalogue at a real HTTPS address read by the daemon, with its official entry downloaded and removed from the window |
| Real window, Phase 4 regression (`window_scenarios.py`) | **12/12** |
| Real window, Phase 5 explorer (`explorer_scenarios.py`) | **12/12** (resource measurement skipped) |
| Headless browser (`browser_settings.mjs`) | **7/7**; an idle window called `get_documents` 0 times in 20 s |

Existing tests changed, without loosening any of them:

- **Phase 1 migration test:** now asserts the latest migration instead of the number 2.
- **Navigation catalogue test:** "schematics" is now listed as implemented.
- **Safety pins:** extended consciously, with stricter network rules added.
- **Phase 4 window script:** the page label and heading were updated.

Defects found and fixed during testing:

- **A finished search could show "No documents found".** The view read the stored results
  before the job state. A search that finished in between (an offline search takes about 2 ms)
  was reported as finished with the previous, empty list, and the page stopped polling. This
  was what caused the intermittent real-window failure (2 of about 8 runs), first taken for an
  accessibility quirk. The view now snapshots the job state first. The new test
  `a_finished_view_always_contains_the_results` (100 rounds) fails against the old order and
  passes against the new one.

- **Lost results under concurrent access.** A search's results could be lost when the window
  read the database while the worker wrote it (SQLite "database is locked", no busy timeout on
  writable connections). Writable connections now wait up to 5 s. The integration test that
  exposed it passed 6/6 afterwards, and the full suite passes.

- **The archive returned nothing.** Its search requires every word, so the type words
  ("schematic", "manual") excluded the real documents. Identity-only queries were added; the
  model alone is used only when it carries a distinctive number.
- **Rows missing for screen readers.** WebKitGTK did not expose the identity rows (spans inside
  a grid `div`); they are now paragraphs.
- **Results missing from the accessibility tree.** The results did not replace the empty state
  there; the panel is now keyed so it remounts.
- **Source scan matches.** A `Signal` enum and an `"rpm"` literal tripped the scan. The enum was
  renamed, and the archive check became an allow-list instead of a deny-list.

## 15. Performance observations (test laptop)

| Measurement | Result |
|---|---|
| Daemon idle (Schematic Library open, nothing running) | 0 % CPU, 1 thread, 8.4 MiB RSS |
| Window on the Schematic Library page, idle (30 s) | 0.57 % CPU, 419 MiB RSS (WebKit processes) |
| Offline search of a 200-file library (20 KB files) | 0.31 s wall, 0.18 s daemon CPU, peak 10.1 MiB RSS, 2 threads during, back to 1 after |
| Online search (fixture identity, Internet Archive) | 4–6 s, dominated by the 1.5 s per-host interval |
| Frontend bundle | 436 kB JavaScript (126 kB gzip), +26 kB |

There's no background searching and no polling while idle (browser check), and network use is
bounded: at most 2 providers at a time, and one request per host per interval.

## 16. Known limitations

- **Out-of-the-box coverage is small.** Online discovery is off by default. When on, only the
  Internet Archive (references only) is searched, plus any catalogues you add. No catalogue is
  shipped, so no manufacturer document is found automatically. **Blocked by external source
  availability.**
- **Matching uses names and metadata, not contents.** It can be fooled by mislabelled
  documents.
- **No revision, no exact match.** Boards whose firmware reports no revision can never be an
  exact match (by design). **Blocked by missing hardware information** where firmware omits it.
- **Download from a real manufacturer site:** physically validated with Lenovo's official PSREF
  PDF, both in the live unit test and clicked in the real window through the daemon.
- **Trust coverage:** vendors outside Sentinel's table of about 20 get no domain trust.
- **Cancellation:** a request already waiting on a slow server finishes or times out (at most
  20 s by default) before cancellation takes effect.
- **Physical validation:** one machine (Dell, GNOME Wayland).


## 17. Unsupported manufacturers or document sources

- **Sources:**
  - manufacturer support portals (Dell, Lenovo PSREF, HP, ASUS, …), because they need
    JavaScript or an account and their terms forbid automated searching;
  - authorised repair-documentation services (accounts or paid access);
  - forums and file-sharing sites, whose rights can't be verified;
  - Google, Bing and DuckDuckGo search, which need API keys or forbid automated queries.
- **Manufacturers:** none is unsupported for matching, but domain trust exists only for the
  vendors in `vendors.rs`.

## 18. Remaining blockers and decisions for you

1. **systemd unit:** resolved with an opt-in path. The optional unit stays limited to Unix
   sockets. `resources/systemd/sentinel-online-discovery.conf` is a drop-in you install by hand
   (instructions in the file) that adds IPv4 and IPv6 sockets for online discovery and changes
   nothing else. The safety suite pins both files, and `systemd-analyze --user verify` accepts
   them. Sentinel never installs or edits units itself.

   **Physically validated with a daemon started by systemd.** I used transient
   `systemd-run --user` services with the unit's exact hardening, a throwaway home and the
   T480 fixture. No unit file was written, and nothing was left afterwards.
   - **With the unit as shipped:** online discovery failed safely, reported as "the source
     could not be reached" (the name lookup is blocked). The first run reported "robots.txt does
     not allow this address" instead; I fixed that, and an unreachable host now reports its real
     cause.
   - **With the drop-in's `RestrictAddressFamilies`:** the daemon read the real web catalogue,
     classed Lenovo's official PDF high-confidence from the manufacturer's site, and downloaded
     it.
2. **Default catalogue:** deliberately none. The plan doesn't require one, and shipping a list of
   addresses Sentinel cannot vet would risk presenting unverified documents as discoveries.
   You can configure web catalogues, or keep a local `*.sentinel-catalogue.json` file in the
   library.
3. **Still open from Phase 4:** P4-15 (polling while minimised) and P4-24 (database write
   volume).

## 19. Read-only and privacy boundaries

- **No system changes:** no BIOS, firmware, kernel, device, service, power or network setting
  is touched. Nothing is installed or executed, and no recommendation is applied.
- **Files:** written only inside Sentinel's own data directory. The safety test
  `only_sentinels_own_files_are_written` passes.
- **Network:**
  - nothing is sent unless you enable online discovery, and none of it runs at startup;
  - even then only the model, board identifier and revision words are sent, in sanitised
    queries;
  - serial numbers, UUIDs, SKU and BIOS versions are never sent;
  - the inventory is never uploaded;
  - no account, cookie or credential is used.
- **Physically verified on this laptop:**
  - the daemon opened no TCP/UDP socket during an offline search (safety test and the real
    window);
  - this laptop's identity was never sent during testing: live tests used a generic model and
    the real-window online run used the fictitious fixture identity.
- **Access controls:** no authentication, paywall or access control is bypassed. Community
  uploads with unverified rights are never downloaded.

## 20. Phase 7 not implemented

There is no schematic parsing, OCR, component or net extraction, and no boardview coordinate
processing. Downloaded and library files are only identified (type, format, PDF title, archive
entry names) and never interpreted. Phase 7 can start from:

- `DocumentCandidate` records whose retrieval status is `Downloaded` or `InLibrary`;
- `download::verify_stored`;
- the match class, used as a gate.
