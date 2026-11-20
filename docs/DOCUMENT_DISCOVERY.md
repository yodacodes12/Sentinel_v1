# Document discovery and verification (Phase 6)

Sentinel looks for documentation of the detected motherboard (schematics, boardviews, service
and hardware manuals, specification sheets, firmware documents), judges how well each document
matches the board, and says so with the evidence. **Finding a document is not verifying it**:
every result carries a hardware-match class, a separate source-trust label and an explanation.

Nothing is searched until you ask. **Online discovery is off by default**: with it off, Sentinel
makes no network request at all and only reads your document library.

## 1. Architecture

```text
 SystemInventory.motherboard (Phase 2) ──▶ SearchIdentity (identity.rs: original + normalised)
                                               │
                                               ▼
                                         query.rs: conservative queries
                                               │
   ┌───────────────── engine.rs: ≤2 providers in parallel, cancellable ─────────────────┐
   │ local_library (offline)     catalogue (online)          internet_archive (online)   │
   └──────────────┬───────────────────────────────────────────────────────────────────────┘
                  │ raw candidates (de-duplicated by normalised address)
                  ▼
     classify.rs (type, format)   matching.rs (hardware match)   trust.rs (source trust)
                  │
                  ▼
   daemon documents.rs ──▶ sentinel.db (migration 3) + files in ~/.local/share/sentinel/documents
                  │ IPC (6 typed requests)
                  ▼
   client: open_document (re-validates, `gio open`)   ◀── Tauri commands ◀── Schematic Library page
```

| Concern | Module |
|---|---|
| Hardware identity | `crates/sentinel-docs/src/identity.rs`, `vendors.rs` |
| Search-query generation | `query.rs` |
| Source providers | `provider.rs`, `providers/{local,catalogue,archive}.rs` |
| Candidate collection | `engine.rs` |
| Document type identification | `classify.rs` |
| Hardware-match verification | `matching.rs` |
| Source provenance | `trust.rs`, `model.rs` (`SourceInfo`) |
| Network access (the only module) | `net.rs` |
| Downloads and stored files | `download.rs` |
| Persistence and caching | `crates/sentinel-storage/src/docs.rs`, `crates/sentinel-daemon/src/documents.rs` |
| Frontend | `apps/desktop/src/pages/SchematicLibraryPage.tsx`, `app/documentsView.ts`, `hooks/useDocuments.ts` |

The daemon runs a search or download on one worker thread that exists only while the job runs.
The frontend never searches and never sees provider credentials (there are none); it refers to
documents by the daemon's numeric ID only.

## 2. Motherboard identity

From Phase 2's `BoardIdentity` (DMI/SMBIOS, `/sys/class/dmi/id`), plus `product_version` for
Lenovo machines whose product name is a machine type.

| Field | Used for |
|---|---|
| System manufacturer (board manufacturer if the system one is a placeholder) | Queries, matching, trust |
| Board manufacturer | Queries for the board identifier; matching (an ODM-built board may name another vendor) |
| System model | Queries, matching; its longest digit-bearing token (`3535`, `T480`) is the *distinctive model number* |
| System family | Matching only (never searched alone) |
| Board name | Queries and matching, only when distinctive (letters and digits, ≥ 4 characters) and not equal to the model |
| Board revision | Revision queries and revision matching |

Never used, never sent: serial numbers, UUIDs, SKU, BIOS version and date, asset tags, the
fingerprint, the inventory.

Normalisation keeps the original value and adds an upper-case key without separators
(`Inspiron 15 3535` → `INSPIRON153535`) and its tokens. Manufacturer spellings map to one vendor
(`Dell Inc.`, `DELL INC` → Dell; `ASUSTeK COMPUTER INC.` → ASUS; company suffixes removed, whole
words only: `HPC Systems` is not HP). Firmware placeholders (`To Be Filled By O.E.M.`,
`Default string`, ...) are dropped; a "board version" equal to the BIOS version is a firmware
version and is dropped. Distinct models never merge: `3535` and `3530` give different identity
hashes. Completeness is `full`, `board_only`, `system_only` or `insufficient`; the identity hash
(SHA-256 of the normalised fields) is the cache key.

## 3. Queries

Only manufacturer + exact model, or board manufacturer + board identifier, never a family or a
manufacturer alone:

```text
Dell Inspiron 15 3535 schematic | boardview | service manual | technical specifications
Dell Inspiron 15 3535 · Inspiron 15 3535          (identity only, for all-words search services)
Dell 0N90HF schematic | boardview · Dell 0N90HF · Dell 0N90HF rev A02
```

The model alone is searched only when it has a distinctive number. A missing revision drops the
revision query; a missing board drops the board queries; an `insufficient` identity produces no
query, and online providers are then not used at all. Queries are sanitised (letters, digits,
spaces and `- . / + ( )`, at most 96 characters), so an identifier cannot inject search
operators, and duplicates are removed.

## 4. Providers

| Provider | Online | Category | What it does |
|---|---|---|---|
| **Document library** (`local_library`) | no | your library | Reads the regular files (no links) directly in `~/.local/share/sentinel/documents/library`: up to 2000, PDF, ZIP or boardview by extension, format confirmed from the bytes, PDF title read from the first 256 KiB / last 64 KiB. Files are never changed, moved or opened by Sentinel |
| **Documentation catalogues** (`catalogue`) | yes | per document | Reads JSON catalogues at HTTPS addresses you configure (at most 8, 4 MiB each), format below. A catalogue can also be kept locally: a library file named `*.sentinel-catalogue.json` is read as a catalogue (offline), its entries matched and trusted the same way |
| **Internet Archive** (`internet_archive`) | yes | community | The documented public search API `https://archive.org/advancedsearch.php`, text items only, 10 rows per query, at most 4 identity queries. Results are **references only**: anyone can upload to the Archive, so rights are unverified and nothing is downloaded from it |

The `Provider` trait is one method (`search`) plus a description; adding a provider is a new
module registered in `Documents::providers`. One provider failing (timeout, HTTP error, invalid
answer, rate limiting) never stops the others: the session is `partial` and the failure is shown.

No manufacturer website is scraped: manufacturer support portals need JavaScript or accounts and
their terms do not permit automated searching, so manufacturer documents come from catalogues
that list their addresses. A catalogue entry pointing at a manufacturer's own domain is
recognised as *published by the manufacturer*.

### Catalogue format

```json
{
  "format": "sentinel-document-catalogue",
  "version": 1,
  "publisher": "Name of whoever maintains the list",
  "documents": [
    {
      "title": "Inspiron 15 3535 Owner's Manual",
      "url": "https://dl.dell.com/.../inspiron-15-3535-owners-manual.pdf",
      "page_url": "https://www.dell.com/support/...",
      "manufacturer": "Dell",
      "models": ["Inspiron 15 3535"],
      "boards": ["0N90HF"],
      "revisions": ["A02"],
      "filename": "inspiron-15-3535-owners-manual.pdf",
      "size_bytes": 5242880,
      "description": "optional"
    }
  ]
}
```

At least one of `url` and `page_url` is required; both must be public HTTPS addresses (others
are dropped). At most 20 000 entries. Entries for another recognised manufacturer are skipped.
The `models`, `boards` and `revisions` lists are exact-match evidence. A catalogue's own claims
about publishers are not trusted: trust comes from the document's host.

## 5. Document types and formats

Types from whole words of the title, filename, PDF title and description: schematic
(`schematic`, `circuit diagram`, `block diagram`), boardview (word, or `.brd .bdv .bv .bvr .fz
.cad .asc .tvw .gr .cst .f2b`), service manual (`service manual`, `owner's manual`, `hardware
maintenance manual`, `repair manual`, `disassembly`, ...), firmware documentation, specification
sheet, hardware manual, otherwise unclassified. Formats: PDF, ZIP, boardview, web page, unknown,
with how they were established: from the **content** (library files and downloads), or only from
the extension or the server's `Content-Type`, which the UI labels "not verified". Basic metadata
kept: filename (sanitised, never used as a path), extension, MIME hint, size, source and page
addresses, discovery and verification times, retrieval status, PDF title, description, the
queries that found it. No schematic parsing, OCR or boardview reading (Phase 7).

## 6. Verification

`matching::assess` compares the identity with the candidate's title, filename, address path,
description, PDF title, structured catalogue claims and host. Each signal records where it was
found, what was compared and its points:

| Signal | Effect | Points |
|---|---|---|
| Exact system model (token sequence, or catalogue model list) | supports | +6 |
| Board identifier as a whole word (or catalogue board list) | supports | +6 |
| Distinctive model number as a whole word (`3535`, not `35350`) | supports | +3 |
| Board revision stated next to `rev`/`revision` or after the board ID (or catalogue list) | supports | +2 |
| Manufacturer named, or hosted on the manufacturer's domain | supports | +2 |
| Product family named | supports | +1 |
| Another manufacturer named (and ours not), or hosted on another manufacturer's domain | contradicts, critical | −5 |
| Another model number of the same shape next to our family or model words (`Inspiron 15 3530`) | contradicts, critical | −5 |
| Another product of the same manufacturer (`Latitude 5420`) | contradicts, critical | −5 |
| Catalogue lists other models / other boards only | contradicts, critical | −5 |
| Another board revision stated | contradicts | −3 |
| Board design manufacturer (Compal, Quanta, ...) named | context | 0 |
| Several models named including ours | context | 0 |

Classes are decided by rules; the points are shown for transparency and never decide alone:

| Class | Rule |
|---|---|
| **Exact match** | Exact model or board identifier (the board needing manufacturer, number or family support), firmware reports a revision, the document states the same revision, nothing contradicts |
| **High-confidence match** | As above, but the document states no revision or firmware reports none |
| **Related model** | Same model with a different revision, or a different model of the same family. "Similar is not electrically compatible" |
| **Possible match** | Some identifiers match (model number alone, board ID without support, manufacturer + family), or identity matches *and* is contradicted (a matching filename with a title naming another model) |
| **Unverified** | Nothing identifies the hardware, or only the manufacturer matches |
| **Mismatch** | Critical contradiction with no exact model or board match |

Missing information never adds points and is listed ("board revision: the document does not
state one", "board revision: not reported by firmware"). A revision can never be confirmed when
firmware reports none, so such boards get at most "high-confidence". Every candidate except an
exact match recommends manual verification, and every explanation says that Sentinel compared
names and metadata, not the document's contents.

**Source trust is a separate dimension** (`trust.rs`), from the host only:

| Trust | Meaning |
|---|---|
| Manufacturer site | A domain of the detected manufacturer (e.g. `dl.dell.com`). Says who published it, not which board it is for |
| Other manufacturer's site | An official domain of a different vendor |
| Community upload | `archive.org`, `ifixit.com`, `github.com`: anyone can upload; rights unverified |
| Your library | A file you added; origin unknown to Sentinel |
| Unverified source | Any other host |

An official source can carry a mismatch; a community upload can be an exact match. Sentinel
never calls a document manufacturer-approved.

## 7. Downloads and stored files

Downloads happen only when you press Download (or, if you enable it, automatically for exact or
high-confidence matches from the manufacturer's own site, at most 3 per search). They are allowed
only for documents whose access status is *downloadable*: HTTPS addresses on a manufacturer
domain, not web pages, not mismatches. Community uploads and unknown hosts are references only:
the source page can be opened in your browser.

Protections (tests in `net.rs`, `download.rs`, `classify.rs`):

- HTTPS only, port 443 only, no credentials in URLs, at most 2048 characters, no spaces or
  control characters.
- No request to loopback, private, link-local (cloud metadata), carrier-NAT, multicast,
  documentation, reserved or IPv4-mapped/NAT64-embedded private addresses, or to local names
  (`localhost`, `*.local`, `*.lan`, single-label hosts). The resolver drops non-public addresses
  at connection time, so DNS rebinding cannot reach the local network.
- Redirects are never followed automatically; each hop is re-validated, at most 4.
- robots.txt is fetched once per host per session and honoured (a group naming Sentinel, else
  `*`; longest rule wins); an unreachable robots.txt means do not crawl.
- At least the configured interval (default 1.5 s) between two requests to the same host; HTTP
  429 stops that provider for the session. No proxy, cookies, compression or authentication.
- Bodies are streamed to a private temporary file (mode 0600) in the download folder and capped
  at the configured size (default 100 MB); a larger, interrupted or cancelled download leaves
  nothing behind.
- The format is decided from the bytes: web pages (login or paywall pages), executables (ELF,
  PE, Mach-O, scripts) and unrecognised content are rejected whatever the name says.
- ZIP archives are inspected (central directory only) and refused if an entry could escape a
  folder (`..`, absolute or drive paths, backslashes), is a link, or is not a document, image,
  text or boardview type. **Archives are never extracted.**
- The stored name is `<sha256>.<ext>`; the remote filename is metadata only. An existing file is
  never overwritten (hard-link creation fails if it exists); an identical file is reused.
- Nothing is opened, extracted or executed automatically. "Open local copy" re-checks the stored
  name, that it is a regular file directly in Sentinel's folder (no link), that its hash still
  matches and that it is a PDF, then hands it to `gio open`. "Open source page" re-validates the
  address from the daemon's record (never from the webview) and opens it in the default browser.
- Removal deletes only a file with Sentinel's naming, regular, directly in its folder, and only
  when no other result refers to it.

## 8. Storage and caching

Schema migration 3 in `sentinel.db` (the existing database; no second one):

| Table | Holds |
|---|---|
| `doc_identities` | identity hash (key), identity JSON (original and normalised values), first seen, last search |
| `doc_sessions` | session ID, identity, start/finish, status, JSON with queries, provider outcomes, counts, error; 50 per identity |
| `doc_candidates` | ID, identity, de-duplication key (unique per identity), candidate JSON (type, source, trust, match class, evidence, revision), discovered/verified times, stored file and SHA-256 |

Documents themselves are files under `~/.local/share/sentinel/documents/downloads`, never in the
database. Behaviour: candidates are de-duplicated by normalised address (or library file name);
the first discovery time and an existing download survive re-searches; results are reused for
the configured number of days (default 14) unless you press Refresh; a cancelled or failed
search does not count as a cached result; a search running when the daemon stops is recorded
as *interrupted* at the next start; Clear results forgets results and sessions but keeps
downloaded copies. Without a working database discovery still works, keeping the last results in
memory, and downloads are refused.

## 9. Configuration (`[documents]` in `config.toml`)

| Setting | Default | Range |
|---|---|---|
| `online_discovery` | `false` | — |
| `internet_archive` | `true` (used only when online) | — |
| `catalogue_urls` | `[]` | ≤ 8 `https://` addresses of named hosts |
| `request_timeout_s` | 20 | 3–120 |
| `min_request_interval_ms` | 1500 | 250–60 000 |
| `cache_days` | 14 | 1–365 |
| `max_download_mb` | 100 | 1–1024 |
| `auto_download` | `false` | — |

The storage location follows the existing XDG layout and is not configurable. All settings are
editable in Settings → Documentation discovery and through `UpdateConfiguration`.

## 10. Privacy

Offline (the default): nothing leaves the computer. Online: a provider receives only the query
text (manufacturer, model, board identifier and revision words), plus the standard request
headers with the user agent `Sentinel-DocumentDiscovery/0.1 (Linux desktop application;
user-initiated)`. No serial number, UUID, SKU, BIOS version, inventory, account or contact
information is sent. Searches are logged locally without their query text.

## 11. Using it

- Desktop: **Schematic Library** (sidebar), or Hardware → Motherboard → "Open the Schematic
  Library".
- CLI: `sentinel docs` (results; `--all` includes other hardware, `--json`), `sentinel docs
  search [--refresh] [--no-wait]`, `sentinel docs cancel`, `sentinel docs download <id>`,
  `sentinel docs remove <id>`, `sentinel docs clear`.

## 12. Validation on real sources

- Internet Archive search: live, generic identity (ThinkPad T480): the archive's "ThinkPad T480
  Hardware Maintenance Manual" was found and classed high-confidence, community, reference only.
- Manufacturer document: live, a catalogue entry for Lenovo's official PSREF specification PDF
  was classed high-confidence and "manufacturer site", then downloaded through every check above
  (robots.txt, public-only resolver, content sniffing, hashing, verification, de-duplication,
  size cap).
- Real window and daemon: offline library search on this computer's own identity; online search
  with a fictitious identity against the real Internet Archive; a download clicked in the window
  for Lenovo's official PSREF PDF (listed in a local catalogue, fixture identity ThinkPad T480),
  fetched by the daemon from psref.lenovo.com, stored, verified and removed; a live fetch of a
  configured HTTPS catalogue address, correctly reported as not a catalogue; and a **valid
  catalogue served over real HTTPS** (a public echo endpoint), read by the daemon, its official
  entry downloaded and removed from the window.

## 13. Limitations

- Coverage depends entirely on the sources: with online discovery off, only your library; with
  it on, the Internet Archive (community uploads, references only) and the catalogues you
  configure. Sentinel ships no catalogue, so out of the box no manufacturer document is found.
  **Not finding a document is not evidence that none exists.**
- Manufacturer portals are not searched (they need JavaScript or accounts, and their terms do
  not allow automated searching). No authorised repair providers are integrated (they need
  accounts or paid access).
- Matching compares names and metadata, not contents. A mislabelled document can be classed
  wrongly; an unusual naming scheme can hide a correct one. Exact matches still deserve a check.
- Boards without a reported revision (common on laptops) can never be an exact match.
- Vendors outside the table in `vendors.rs` have no domain-based trust and no naming variations.
- Under the optional systemd unit (`RestrictAddressFamilies=AF_UNIX`) the daemon cannot open
  network sockets, so online discovery reports every source as unreachable. To use online
  discovery under systemd, install the drop-in `resources/systemd/sentinel-online-discovery.conf`
  (it adds only `AF_INET AF_INET6`); the unit itself stays hardened. Both cases were checked with
  a daemon started by systemd (PHASE_6.md, 18).
- A request that is already waiting for a slow server finishes or times out before cancellation
  takes effect (at most the request timeout).
