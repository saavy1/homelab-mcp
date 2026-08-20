# Media Season Availability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `homelab media library availability --media-id <tmdb-id> --season <number>` so agents can compare Jellyseerr's announced season against episodes actually present in Jellyfin.

**Architecture:** Extend the versioned public model with normalized availability DTOs, add typed Jellyseerr and Jellyfin reads, and keep all matching/completeness rules in a pure `availability` module. `MediaService` orchestrates those reads; Axum, the typed client, and the CLI remain thin adapters over one fixed read-only operation.

**Tech Stack:** Rust 2024, Tokio, Reqwest, Axum 0.8, Clap, Serde, Schemars, Chrono, Cargo test/Clippy/rustfmt, GitHub Actions, ArgoCD/Flux GitOps, Tailscale.

**Spec:** `docs/superpowers/specs/2026-08-19-media-season-availability-design.md`

## Global Constraints

- The fixed public operation is `media.library.availability` at `GET /api/v1/media/library/availability?media_id={positive_i64}&season={u32}`.
- `media_id` is the TMDB/Jellyseerr catalog ID returned by `homelab media search`; title search and backend selection are not accepted by this operation.
- Season `0` is valid; `media_id <= 0` and a season that cannot decode as `u32` are validation failures before backend dispatch.
- Jellyseerr is authoritative for expected episodes, title, episode numbers, TMDB episode IDs, and air dates. Jellyfin is authoritative for library presence.
- Match by TMDB episode ID first and exact `(season_number, episode_number)` only when either side lacks a TMDB ID. Never match by title, date, array position, or fuzzy similarity.
- Matching must be linear-time with maps and sets; do not introduce a nested expected-by-actual scan.
- Duplicate identities, ambiguous series, or reuse of one actual episode are conflicts. Checked integer conversions must reject overflow rather than truncate.
- `CompletenessStatus::Unknown` is valid only for `aired.status`; announced completeness is always `complete` or `incomplete`.
- A missing Jellyfin series is successful data with `in_library: false`, zero available counts, `jellyfin_id: null`, and `episodes: null`.
- Public DTOs contain normalized values only. Never return or log API keys, URLs, file paths, raw response fragments, or opaque Jellyfin episode IDs.
- Jellyfin pages use `Limit=200` and stop on `TotalRecordCount`, a short page, or an empty page. More than 10,000 records is an internal failure, never silent truncation.
- The server does not retry; existing read timeout, request-ID, redaction, completion-event, and error-envelope behavior applies.
- API major remains `1`; API minor becomes `1`; existing operations and v1.0 clients remain compatible.
- The release tag is `homelab-v1.1.0`; deploy the server by immutable digest before installing the new CLI.

---

### Task 1: Define Versioned Availability API Models

**Files:**
- Modify: `Cargo.toml:20-45`
- Modify: `crates/homelab-api-model/Cargo.toml`
- Modify: `crates/homelab-api-model/src/lib.rs:6-216`

**Interfaces:**
- Consumes: Existing `OperationEnvelope<T>`, `JsonSchema`, Serde, and API version constants.
- Produces: `SeasonAvailabilityQuery`, `CompletenessStatus`, `EpisodeReleaseStatus`, `EpisodePresence`, `AvailabilitySeries`, `CompletenessSummary`, `AvailabilityEpisode`, `SeasonAvailability`; `API_MINOR == 1`.

- [ ] **Step 1: Add failing model contract tests**

Append tests to the existing `#[cfg(test)] mod tests` in `crates/homelab-api-model/src/lib.rs`. Exercise exact field names, date encoding, nullability, and enum spelling:

```rust
#[test]
fn season_availability_contract_is_normalized_snake_case() {
    let value = serde_json::to_value(SeasonAvailability {
        series: AvailabilitySeries {
            media_id: "60625".into(),
            jellyfin_id: Some("series-1".into()),
            title: "Rick and Morty".into(),
        },
        season: 3,
        as_of: NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
        in_library: true,
        aired: CompletenessSummary {
            status: CompletenessStatus::Incomplete,
            expected_count: 2,
            available_count: 1,
            missing_count: 1,
        },
        announced: CompletenessSummary {
            status: CompletenessStatus::Incomplete,
            expected_count: 3,
            available_count: 2,
            missing_count: 1,
        },
        unknown_air_date_count: 1,
        next_airing: Some(AvailabilityEpisode {
            episode_id: "303".into(),
            episode_number: 3,
            title: "Future".into(),
            air_date: Some(NaiveDate::from_ymd_opt(2026, 8, 21).unwrap()),
            release_status: EpisodeReleaseStatus::Future,
            presence: EpisodePresence::Available,
        }),
        episodes: Some(vec![]),
    })
    .unwrap();

    assert_eq!(value["as_of"], "2026-08-20");
    assert_eq!(value["aired"]["status"], "incomplete");
    assert_eq!(value["next_airing"]["release_status"], "future");
    assert_eq!(value["next_airing"]["presence"], "available");
    assert!(value.get("source").is_none());
}

#[test]
fn season_availability_query_rejects_unknown_fields() {
    let error = serde_json::from_value::<SeasonAvailabilityQuery>(serde_json::json!({
        "media_id": 60625,
        "season": 3,
        "backend": "jellyfin"
    }))
    .unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}
```

- [ ] **Step 2: Run the focused model test and confirm RED**

Run:

```bash
cargo test -p homelab-api-model season_availability -- --nocapture
```

Expected: compile failure because the availability DTOs and `NaiveDate` import do not exist.

- [ ] **Step 3: Add Chrono and the exact public DTOs**

Add `chrono.workspace = true` to `crates/homelab-api-model/Cargo.toml`. In `lib.rs`, import `chrono::NaiveDate`, change `API_MINOR` to `1`, and define:

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonAvailabilityQuery {
    pub media_id: i64,
    pub season: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessStatus {
    Complete,
    Incomplete,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeReleaseStatus {
    Aired,
    Future,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodePresence {
    Available,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AvailabilitySeries {
    pub media_id: String,
    pub jellyfin_id: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CompletenessSummary {
    pub status: CompletenessStatus,
    pub expected_count: u32,
    pub available_count: u32,
    pub missing_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AvailabilityEpisode {
    pub episode_id: String,
    pub episode_number: u32,
    pub title: String,
    pub air_date: Option<NaiveDate>,
    pub release_status: EpisodeReleaseStatus,
    pub presence: EpisodePresence,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SeasonAvailability {
    pub series: AvailabilitySeries,
    pub season: u32,
    pub as_of: NaiveDate,
    pub in_library: bool,
    pub aired: CompletenessSummary,
    pub announced: CompletenessSummary,
    pub unknown_air_date_count: u32,
    pub next_airing: Option<AvailabilityEpisode>,
    pub episodes: Option<Vec<AvailabilityEpisode>>,
}
```

- [ ] **Step 4: Run all model contract tests**

Run:

```bash
cargo test -p homelab-api-model
```

Expected: all tests pass, including the exact snake-case/date/null contract and existing v1 DTO contracts.

- [ ] **Step 5: Commit the model contract**

```bash
git add Cargo.toml Cargo.lock crates/homelab-api-model/Cargo.toml crates/homelab-api-model/src/lib.rs
git commit -m "feat: define season availability API contract"
```

---

### Task 2: Read and Normalize Jellyseerr Expected Seasons

**Files:**
- Create: `crates/homelab-media/src/availability.rs`
- Modify: `crates/homelab-media/src/lib.rs`
- Modify: `crates/homelab-media/src/clients/jellyseerr.rs:1-324`

**Interfaces:**
- Consumes: `MediaError`, `JellyseerrClient::send`, Chrono, and caller-supplied positive `media_id`/`season`.
- Produces: `pub(crate) struct ExpectedSeason`, `pub(crate) struct ExpectedEpisode`, and `JellyseerrClient::expected_season(media_id: i64, season: u32) -> Result<ExpectedSeason, MediaError>`.

- [ ] **Step 1: Add failing Jellyseerr normalization tests**

Add focused `#[cfg(test)] mod tests` mock-server tests inside `crates/homelab-media/src/clients/jellyseerr.rs`, where the crate-private method and normalized types remain testable without expanding the library's public API. Define a local `spawn(app: Router) -> String` with `TcpListener::bind("127.0.0.1:0")`, `tokio::spawn(axum::serve(listener, app))`, and a local `client(base_url, key)` matching the existing integration-test helper. Gate both success handlers on a shared two-party `tokio::sync::Barrier`; the test deadlocks under the existing read timeout if the requests become sequential, proving concurrency without wall-clock assertions. The success case must also assert both exact paths, the API-key header, stable episode order from the source list, and invalid-date normalization:

```rust
#[tokio::test]
async fn expected_season_reads_tv_and_season_concurrently_and_normalizes_dates() {
    let app = Router::new()
        .route("/api/v1/tv/60625", get(|headers: HeaderMap| async move {
            assert_eq!(headers["x-api-key"], "key");
            Json(json!({"id": 60625, "name": "Rick and Morty"}))
        }))
        .route("/api/v1/tv/60625/season/3", get(|| async {
            Json(json!({"seasonNumber": 3, "episodes": [
                {"id": 301, "episodeNumber": 1, "name": "A", "airDate": "2017-04-01"},
                {"id": 302, "episodeNumber": 2, "name": "B", "airDate": "not-a-date"},
                {"id": 303, "episodeNumber": 3, "name": "C", "airDate": null}
            ]}))
        }));

    let season = client(spawn(app).await, "key")
        .expected_season(60625, 3)
        .await
        .unwrap();
    assert_eq!(season.media_id, "60625");
    assert_eq!(season.title, "Rick and Morty");
    assert_eq!(season.season, 3);
    assert_eq!(season.episodes[0].tmdb_id, "301");
    assert_eq!(season.episodes[0].air_date.unwrap().to_string(), "2017-04-01");
    assert_eq!(season.episodes[1].air_date, None);
    assert_eq!(season.episodes[2].air_date, None);
}
```

Also add tests that a missing title, episode ID, episode number, or title returns `ErrorCode::Internal`; either endpoint's `404` returns `ErrorCode::NotFound`; and season `0` is sent unchanged.

- [ ] **Step 2: Run the Jellyseerr tests and confirm RED**

```bash
cargo test -p homelab-media clients::jellyseerr::tests::expected_season -- --nocapture
```

Expected: compile failure because `expected_season`, `ExpectedSeason`, and `ExpectedEpisode` are absent.

- [ ] **Step 3: Introduce normalized expected-season types**

Create `crates/homelab-media/src/availability.rs` with only normalized internal inputs at this stage:

```rust
use chrono::NaiveDate;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExpectedSeason {
    pub(crate) media_id: String,
    pub(crate) title: String,
    pub(crate) season: u32,
    pub(crate) episodes: Vec<ExpectedEpisode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExpectedEpisode {
    pub(crate) tmdb_id: String,
    pub(crate) episode_number: u32,
    pub(crate) title: String,
    pub(crate) air_date: Option<NaiveDate>,
}
```

Register `mod availability;` in `crates/homelab-media/src/lib.rs`; keep these types crate-private.

- [ ] **Step 4: Implement two concurrent Jellyseerr reads and strict normalization**

Add:

```rust
pub(crate) async fn expected_season(
    &self,
    media_id: i64,
    season: u32,
) -> Result<ExpectedSeason, MediaError> {
    let details_path = format!("/api/v1/tv/{media_id}");
    let season_path = format!("/api/v1/tv/{media_id}/season/{season}");
    let (details, season_value) = tokio::try_join!(
        self.send(Method::GET, "get_tv_details", &details_path, None, false),
        self.send(Method::GET, "get_tv_season", &season_path, None, false),
    )?;
    normalize_expected_season(media_id, season, &details, &season_value)
}
```

Implement `normalize_expected_season` using checked `u32::try_from` conversions. Require `details.name`, a season payload whose `seasonNumber` equals the requested season, and every episode's `id`, `episodeNumber`, and `name`; reject contradictory or duplicate episode identities/numbers. Parse `airDate` with `NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()`. A malformed or missing `episodes` array returns `MediaError::serialization("jellyseerr", "get_tv_season")`; never include the offending value in the error.

- [ ] **Step 5: Run the complete Jellyseerr suites**

```bash
cargo test -p homelab-media clients::jellyseerr::tests
cargo test -p homelab-media --test jellyseerr
```

Expected: the new expected-season unit tests and all existing request/search integration tests pass.

- [ ] **Step 6: Commit the Jellyseerr reader**

```bash
git add crates/homelab-media/src/availability.rs crates/homelab-media/src/lib.rs crates/homelab-media/src/clients/jellyseerr.rs
git commit -m "feat: read expected seasons from Jellyseerr"
```

---

### Task 3: Page Jellyfin Series and Real Episodes

**Files:**
- Modify: `crates/homelab-media/src/availability.rs`
- Modify: `crates/homelab-media/src/clients/jellyfin.rs:1-100`

**Interfaces:**
- Consumes: Caller-supplied TMDB media ID string and season number; existing authenticated Jellyfin `send` method.
- Produces: `LibrarySeason`, `LibraryEpisode`, and `JellyfinClient::library_season(media_id: &str, season: u32) -> Result<Option<LibrarySeason>, MediaError>`.

- [ ] **Step 1: Add failing pagination and ambiguity tests**

Add a `#[cfg(test)] mod tests` in `crates/homelab-media/src/clients/jellyfin.rs` with the same local `spawn` and `client` helpers described in Task 2. Record each URI in shared test state. The success fixture returns two series pages and two episode pages, and asserts these exact query requirements:

```text
/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=0&Limit=200
/Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex=200&Limit=200
/Shows/series-1/Episodes?Season=3&IsMissing=false&Fields=ProviderIds&StartIndex=0&Limit=200
/Shows/series-1/Episodes?Season=3&IsMissing=false&Fields=ProviderIds&StartIndex=200&Limit=200
```

Assert `ProviderIds.Tmdb == "60625"` selects `series-1`; mismatched series are ignored; valid episode records normalize TMDB ID, parent season, and episode number; a record from another season or without `IndexNumber` is ignored.

Add separate tests for:

```rust
assert_eq!(client.library_season("999", 3).await.unwrap(), None);
assert_eq!(duplicate_series.unwrap_err().error_code(), ErrorCode::Conflict);
assert_eq!(over_ten_thousand.unwrap_err().error_code(), ErrorCode::Internal);
```

Also verify unsupported `AnyProviderIdEquals` is absent from every observed URI and malformed page JSON is redacted.

- [ ] **Step 2: Run the Jellyfin availability tests and confirm RED**

```bash
cargo test -p homelab-media clients::jellyfin::tests::library_season -- --nocapture
```

Expected: compile failure because `library_season`, `LibrarySeason`, and `LibraryEpisode` do not exist.

- [ ] **Step 3: Add normalized Jellyfin input types and conflict errors**

Extend `availability.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LibrarySeason {
    pub(crate) series_id: String,
    pub(crate) episodes: Vec<LibraryEpisode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LibraryEpisode {
    pub(crate) jellyfin_id: String,
    pub(crate) tmdb_id: Option<String>,
    pub(crate) season_number: u32,
    pub(crate) episode_number: u32,
}
```

Add `#[error("media records conflict")] Conflict` and `#[error("media normalized data is invalid")] Internal` to `MediaError` in `crates/homelab-media/src/error.rs`. Map them to `ErrorCode::Conflict` and `ErrorCode::Internal`; return exactly `media records conflict` and `media backend returned invalid normalized data` from `public_message`, respectively. Never store or echo offending upstream values in either variant.

- [ ] **Step 4: Implement bounded generic Jellyfin pagination**

Inside `jellyfin.rs`, define:

```rust
const PAGE_SIZE: usize = 200;
const MAX_RECORDS: usize = 10_000;

async fn paged_items(
    &self,
    operation: &'static str,
    path: impl Fn(usize) -> String,
) -> Result<Vec<Value>, MediaError>
```

For each page, require `Items` to be an array. Read `TotalRecordCount` with a checked integer conversion when present. Stop after empty/short page or when accumulated length reaches the total. Before extending, use `checked_add`; reject any result over `MAX_RECORDS`. Detect a server that returns full duplicate pages without advancing by enforcing the cap.

- [ ] **Step 5: Implement exact series and real-episode reads**

Add:

```rust
pub(crate) async fn library_season(
    &self,
    media_id: &str,
    season: u32,
) -> Result<Option<LibrarySeason>, MediaError>
```

Page all server-visible `Series`, locally filter `ProviderIds.Tmdb == media_id`, return `None` for no match, and return conflict for multiple matches. For one match, page `/Shows/{encoded_series_id}/Episodes` with `Season`, `IsMissing=false`, and `Fields=ProviderIds`. Retain only requested-season records with valid nonnegative `IndexNumber`; normalize the opaque episode ID for internal conflict diagnostics only.

- [ ] **Step 6: Run the complete Jellyfin suites**

```bash
cargo test -p homelab-media clients::jellyfin::tests
cargo test -p homelab-media --test jellyfin
```

Expected: the new pagination/absence/conflict/cap unit tests and all existing library/session/refresh integration tests pass.

- [ ] **Step 7: Commit the Jellyfin reader**

```bash
git add crates/homelab-media/src/availability.rs crates/homelab-media/src/clients/jellyfin.rs crates/homelab-media/src/error.rs
git commit -m "feat: read real Jellyfin season episodes"
```

---

### Task 4: Calculate and Orchestrate Season Availability
**Files:**
- Modify: `crates/homelab-media/src/availability.rs`
- Modify: `crates/homelab-media/src/service.rs:1-324`
- Modify: `crates/homelab-media/tests/service.rs`

**Interfaces:**
- Consumes: `ExpectedSeason`, `Option<LibrarySeason>`, explicit `chrono::NaiveDate`, and typed backend readers from Tasks 2–3.
- Produces: `compare_season_availability(expected, actual, as_of) -> Result<SeasonAvailability, MediaError>` and `MediaService::season_availability(request_id, media_id, season) -> Result<OperationEnvelope<SeasonAvailability>, MediaError>`.

- [ ] **Step 1: Add exhaustive pure comparison tests**

Place `#[cfg(test)] mod tests` beside the pure function in `availability.rs`. Use compact builders and assert these observable cases independently:

1. all aired and announced episodes available;
2. one aired episode missing;
3. only a future episode missing;
4. missing unknown-date episode makes aired status `unknown`;
5. available unknown-date episode does not make aired status unknown;
6. absent Jellyfin series returns `in_library: false`, null episode list, zero available counts, and a preserved next airing;
7. season `0` is retained;
8. TMDB provider ID wins over a conflicting number candidate;
9. number fallback works when either side lacks a TMDB ID;
10. duplicate expected TMDB IDs conflict;
11. duplicate actual TMDB IDs conflict;
12. duplicate number fallback candidates conflict;
13. one actual record cannot satisfy two expected records;
14. extra Jellyfin episodes are ignored;
15. equal-date next airings use episode number as tie-break;
16. `air_date == as_of` is classified as aired;
17. empty expected season yields zero-count complete summaries and an empty episode list when the series exists;
18. `checked_count(usize::MAX)` takes the internal error path without allocating an oversized vector.

A representative assertion:

```rust
assert_eq!(result.aired.status, CompletenessStatus::Unknown);
assert_eq!(result.aired.expected_count, 1);
assert_eq!(result.aired.available_count, 1);
assert_eq!(result.aired.missing_count, 0);
assert_eq!(result.announced.status, CompletenessStatus::Incomplete);
assert_eq!(result.unknown_air_date_count, 1);
assert_eq!(result.episodes.as_ref().unwrap()[0].episode_number, 1);
```

- [ ] **Step 2: Run pure tests and confirm RED**

```bash
cargo test -p homelab-media availability::tests -- --nocapture
```

Expected: compile failure because the comparison function is absent.

- [ ] **Step 3: Implement linear-time identity matching**

Build these indexes once:

```rust
HashMap<String, usize>              // actual TMDB ID -> actual index
HashMap<(u32, u32), Vec<usize>>     // (season, episode) -> actual candidates
HashSet<usize>                      // consumed actual indices
HashSet<String>                     // expected TMDB identities
HashSet<(u32, u32)>                 // expected numbering identities
```

For each expected episode, first use the actual TMDB map when the expected TMDB ID has an exact match. If no exact provider match exists, the number index may select only a candidate where the expected or candidate actual record lacks a TMDB ID; a different nonempty TMDB ID is never a fallback match. Reject duplicate candidates and consumed-index reuse. Do not scan the actual vector inside the expected loop.
- [ ] **Step 4: Implement partitioning, counts, ordering, and next airing**

Produce one `AvailabilityEpisode` per expected episode when `actual.is_some()`, sorted by `episode_number`. Derive release state from `air_date` against `as_of`; derive presence from the match result. Use `u32::try_from` for every public count and checked subtraction for `missing_count`.

Compute aired status exactly:

```rust
let aired_status = if aired_missing > 0 {
    CompletenessStatus::Incomplete
} else if unknown_missing > 0 {
    CompletenessStatus::Unknown
} else {
    CompletenessStatus::Complete
};
```

Compute announced status only as complete/incomplete. Select `next_airing` from future episodes using `(air_date, episode_number)` ordering, regardless of presence.

- [ ] **Step 5: Add failing service orchestration tests**

In `crates/homelab-media/tests/service.rs`, mock both Jellyseerr endpoints plus Jellyfin series/episode pages. Assert:

```rust
let envelope = service(config).season_availability("req-availability", 60625, 3).await.unwrap();
assert_eq!(envelope.operation, "media.library.availability");
assert_eq!(envelope.risk, RiskLevel::Read);
assert_eq!(envelope.request_id, "req-availability");
assert_eq!(envelope.data.unwrap().series.media_id, "60625");
```

Add a counter-based test proving `media_id <= 0` fails with `ErrorCode::Validation` before any mock route is reached, and a no-series test proving Jellyfin's episode endpoint is never called.

- [ ] **Step 6: Implement the service method**

Add:

```rust
pub async fn season_availability(
    &self,
    request_id: &str,
    media_id: i64,
    season: u32,
) -> Result<OperationEnvelope<SeasonAvailability>, MediaError> {
    if media_id <= 0 {
        return Err(MediaError::Validation("media_id must be positive".into()));
    }
    let expected = self.jellyseerr.expected_season(media_id, season).await?;
    let actual = self
        .jellyfin
        .library_season(&media_id.to_string(), season)
        .await?;
    let data = compare_season_availability(expected, actual, Utc::now().date_naive())?;
    Ok(success(
        "media.library.availability",
        request_id,
        RiskLevel::Read,
        "season availability compared",
        data,
    ))
}
```

- [ ] **Step 7: Run all media application tests**

```bash
cargo test -p homelab-media
```

Expected: pure comparison, backend, orchestration, and all existing media tests pass.

- [ ] **Step 8: Commit the application behavior**

```bash
git add crates/homelab-media/src/availability.rs crates/homelab-media/src/service.rs crates/homelab-media/tests/service.rs
git commit -m "feat: compare announced and available episodes"
```

---

### Task 5: Expose the Curated Axum Read Route

**Files:**
- Modify: `servers/homelab-api/src/routes/mod.rs:14-123`
- Modify: `servers/homelab-api/src/routes/media.rs:1-508`
- Modify: `servers/homelab-api/src/lib.rs:128-184`
- Modify: `servers/homelab-api/tests/api.rs`

**Interfaces:**
- Consumes: `SeasonAvailabilityQuery` and `MediaService::season_availability`.
- Produces: `GET /api/v1/media/library/availability`, capability `media.library.availability`, structured validation/error/success responses, and read-only route metadata.

- [ ] **Step 1: Add failing route and capability tests**

Extend `servers/homelab-api/tests/api.rs` with a fixture that serves the required Jellyseerr/Jellyfin reads, then assert:

```rust
let response = app
    .oneshot(request(Method::GET, "/api/v1/media/library/availability?media_id=60625&season=3")
        .header(REQUEST_ID_HEADER, "req-season")
        .body(Body::empty())
        .unwrap())
    .await
    .unwrap();
assert_eq!(response.status(), StatusCode::OK);
let body = json_body(response).await;
assert_eq!(body["operation"], "media.library.availability");
assert_eq!(body["request_id"], "req-season");
assert_eq!(body["risk"], "read");
assert_eq!(body["data"]["season"], 3);
```

Update the capabilities assertion to require API `{major: 1, minor: 1}` and the appended operation. Add cases for missing `media_id`, `media_id=0`, negative IDs, negative/overflowing season, duplicate/unknown query keys, and backend `404`; assert invalid queries reach no backend.

- [ ] **Step 2: Run focused API tests and confirm RED**

```bash
cargo test -p homelab-api --test api season_availability -- --nocapture
```

Expected: route is `404` and capability is absent.

- [ ] **Step 3: Register the read route and capability**

Append `media.library.availability` to `OPERATIONS` and register:

```rust
.route(
    "/api/v1/media/library/availability",
    on(MethodFilter::GET, media::season_availability),
)
```

before the existing library status route. Add route metadata in `route_metadata`:

```rust
if path == "/api/v1/media/library/availability" {
    return (
        "media.library.availability",
        RiskLevel::Read,
        "homelab-media",
        None,
    );
}
```

- [ ] **Step 4: Implement strict query validation and dispatch**

In `routes/media.rs`, parse only `media_id` and `season`:

```rust
pub(crate) async fn season_availability(
    State(state): State<ApiState>,
    Extension(context): Extension<RequestContext>,
    RawQuery(raw): RawQuery,
) -> Response {
    let meta = OperationMeta::new(
        "media.library.availability",
        RiskLevel::Read,
        "homelab-media",
        None,
    );
    let query = match parse_query::<SeasonAvailabilityQuery>(
        raw.as_deref(),
        &["media_id", "season"],
    ) {
        Ok(query) if query.media_id > 0 => query,
        Ok(_) => return validation_response(
            &context.request_id,
            meta,
            "media_id must be positive",
        ),
        Err(message) => return validation_response(&context.request_id, meta, message),
    };
    let result = state
        .media
        .season_availability(&context.request_id, query.media_id, query.season)
        .await;
    service_response(&context.request_id, meta, result)
}
```

Reuse `validation_response` and `service_response` exactly as shown; do not duplicate response mapping.

- [ ] **Step 5: Run the complete server contract suite**

```bash
cargo test -p homelab-api
```

Expected: all curated-route, validation, redaction, timeout, request-ID, and availability tests pass; `/mcp` remains `404`.

- [ ] **Step 6: Commit the server adapter**

```bash
git add servers/homelab-api/src/lib.rs servers/homelab-api/src/routes/mod.rs servers/homelab-api/src/routes/media.rs servers/homelab-api/tests/api.rs
git commit -m "feat: expose season availability API"
```

---

### Task 6: Add the Typed Rust Client Method

**Files:**
- Modify: `crates/homelab-client/src/media.rs:1-219`
- Modify: `crates/homelab-client/tests/client.rs`

**Interfaces:**
- Consumes: `SeasonAvailabilityQuery`, `SeasonAvailability`, and existing `HomelabClient::execute` transport.
- Produces: `MediaClient::season_availability(request_id: &str, query: &SeasonAvailabilityQuery) -> Result<OperationEnvelope<SeasonAvailability>, ClientError>`.

- [ ] **Step 1: Add a failing exact-request client test**

Extend the fixed API handler and `every_operation_uses_a_typed_fixed_route` test:

```rust
let query = SeasonAvailabilityQuery { media_id: 60625, season: 3 };
let envelope = client
    .media()
    .season_availability("req-season", &query)
    .await
    .unwrap();
assert_eq!(envelope.operation, "media.library.availability");

let request = seen.lock().iter()
    .find(|request| request.path == "/api/v1/media/library/availability")
    .unwrap()
    .clone();
assert_eq!(request.method, Method::GET);
assert_eq!(request.query.as_deref(), Some("media_id=60625&season=3"));
assert_eq!(request.request_id.as_deref(), Some("req-season"));
```

Have the mock return a typed `SeasonAvailability`; assert the client rejects a wrong envelope shape through the existing redacted decode-error path.

- [ ] **Step 2: Run the focused client test and confirm RED**

```bash
cargo test -p homelab-client --test client every_operation_uses_a_typed_fixed_route -- --nocapture
```

Expected: compile failure because `season_availability` is absent.

- [ ] **Step 3: Implement the fixed typed method**

Add:

```rust
pub async fn season_availability(
    &self,
    request_id: &str,
    query: &SeasonAvailabilityQuery,
) -> Result<OperationEnvelope<SeasonAvailability>, ClientError> {
    let mut url = self.client.route(&["media", "library", "availability"])?;
    url.query_pairs_mut()
        .append_pair("media_id", &query.media_id.to_string())
        .append_pair("season", &query.season.to_string());
    self.client
        .execute(self.client.http.request(Method::GET, url), request_id)
        .await
}
```

This follows the existing `request_id`-first media-client convention; no raw URL, backend argument, retry loop, or `serde_json::Value` may enter this public method.

- [ ] **Step 4: Run all typed-client tests**

```bash
cargo test -p homelab-client
```

Expected: every fixed route, typed envelope, compatibility gate, mutation single-send, and decode-redaction test passes.

- [ ] **Step 5: Commit the typed client**

```bash
git add crates/homelab-client/src/media.rs crates/homelab-client/tests/client.rs
git commit -m "feat: add season availability client"
```

---

### Task 7: Ship the Agent-Friendly CLI Command

**Files:**
- Modify: `crates/homelab-cli/src/args.rs:1-213`
- Modify: `crates/homelab-cli/src/main.rs:1-470`
- Modify: `crates/homelab-cli/src/render.rs:1-170` only if the existing bounded generic table loses required season fields
- Modify: `crates/homelab-cli/tests/cli.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `MediaClient::season_availability`, stable CLI exit classes, JSON/table renderers.
- Produces: `homelab media library availability --media-id <positive-i64> --season <u32> [--output json|table]` with JSON default and exact operation/risk metadata.

- [ ] **Step 1: Add failing CLI parser and subprocess tests**

Extend the command-tree test to require:

```text
homelab media library availability --help
  --media-id <MEDIA_ID>
  --season <SEASON>
```

Extend the representative command test:

```rust
let output = run(&api, &[
    "media", "library", "availability",
    "--media-id", "60625",
    "--season", "3",
]);
let body = assert_json_success(&output);
assert_eq!(body["operation"], "media.library.availability");
assert_eq!(body["data"]["series"]["media_id"], "60625");
assert_eq!(body["data"]["season"], 3);
```

Assert the observed request is one GET to `/api/v1/media/library/availability?media_id=60625&season=3`. Add no-HTTP exit-2 cases for omitted flags, `--media-id 0`, negative/overflowing media IDs, negative/overflowing seasons, and movie-only flags. Add a table-mode fixture with more than 20 episodes and assert output remains bounded and excludes keys/URLs/raw response data.

- [ ] **Step 2: Run focused CLI tests and confirm RED**

```bash
cargo test -p homelab-cli --test cli availability -- --nocapture
```

Expected: parser rejects the unknown `availability` subcommand.

- [ ] **Step 3: Add strict argument types and dispatch**

Add a positive parser:

```rust
fn parse_positive_media_id(value: &str) -> Result<i64, String> {
    let value = value.parse::<i64>().map_err(|_| "media ID must be a positive integer".to_owned())?;
    if value <= 0 {
        return Err("media ID must be a positive integer".into());
    }
    Ok(value)
}
```

Extend `LibraryCommand`:

```rust
Availability {
    #[arg(long, value_parser = parse_positive_media_id)]
    media_id: i64,
    #[arg(long)]
    season: u32,
}
```

Map the command to operation `media.library.availability` and risk `read`. Construct `SeasonAvailabilityQuery { media_id, season }`, then call `client.media().season_availability(request_id, &query)`. Preserve JSON as the default and existing error-to-exit mapping (`validation` 2, `not_found`/`conflict` 4, `timeout`/`unavailable` 5, internal 1).

- [ ] **Step 4: Render the approved one-row availability table**

In `write_table`, route only operation `media.library.availability` to a dedicated `write_availability_table`; keep every existing operation on the generic renderer. Emit exactly one header and one data row:

```text
TITLE | SEASON | IN_LIBRARY | AIRED | ANNOUNCED | AVAILABLE | EXPECTED | NEXT_AIRING
```

Read `AVAILABLE` and `EXPECTED` from `announced.available_count` and `announced.expected_count`. Render `NEXT_AIRING` as `E{episode_number} {air_date}` or `-`. Use the existing `cell` function for every value, so `MAX_CELL_CHARS` and control-character normalization remain enforced. Never iterate `episodes` in table mode. The table test must assert one data row, all eight headings, announced counts, and absence of episode titles, credentials, URLs, and raw backend fields.

- [ ] **Step 5: Document the exact two-command agent flow**

Add to `README.md` beside the current CLI examples:

```bash
homelab media search --query "Rick and Morty"
homelab media library availability --media-id 60625 --season 3
```

State that `media-id` is the TV catalog ID from search, completeness compares Jellyseerr announcements with Jellyfin presence, season `0` means specials, and the operation is read-only.

- [ ] **Step 6: Run all CLI tests**

```bash
cargo test -p homelab-cli
```

Expected: exact command tree, JSON/table behavior, local validation, request IDs, stable exits, and existing destructive-command protections pass.

- [ ] **Step 7: Commit the CLI contract**

```bash
git add crates/homelab-cli/src/args.rs crates/homelab-cli/src/main.rs crates/homelab-cli/src/render.rs crates/homelab-cli/tests/cli.rs README.md
git commit -m "feat: add season availability CLI"
```

---

### Task 8: Run Repository Gates and Independent Review

**Files:**
- Modify only files identified by failing gates or evidence-backed review findings.
- Verify: all workspace crates and `docs/superpowers/specs/2026-08-19-media-season-availability-design.md`.

**Interfaces:**
- Consumes: Tasks 1–7 as one complete feature branch.
- Produces: formatted, warning-free, workspace-tested implementation with no stale API `1.0` assumptions and a clean review verdict.

- [ ] **Step 1: Run formatting and inspect any formatter diff**

```bash
cargo fmt --all
cargo fmt --all -- --check
```

Expected: check exits `0` after the formatter pass.

- [ ] **Step 2: Run the complete workspace suite**

```bash
cargo test --workspace
```

Expected: every workspace test passes; record the exact test/suite counts in the task report.

- [ ] **Step 3: Run warnings-as-errors Clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: exit `0` with no warnings.

- [ ] **Step 4: Scan for stale and forbidden interface residue**

Use repository search to verify:

```text
API_MINOR is defined once as 1
media.library.availability appears in model/server/client/CLI tests and capability registration
no availability path accepts query/title/backend/source/raw URL inputs
no AnyProviderIdEquals usage exists
no public availability DTO contains serde_json::Value
no homelab media MCP command or endpoint is reintroduced
```

Any match must be either the approved spec/plan, exact implementation, or a focused test fixture.

- [ ] **Step 5: Request a spec-compliance review**

Give the reviewer the approved spec, all commits since `ace65fa`, and the exact gate outputs. Require file:line findings ranked Critical/Important/Minor, explicit checks of matching ambiguity, unknown dates, absent-series behavior, pagination caps, redaction, and CLI validation. Fix every correctness finding at the source, rerun the affected package tests, and repeat review until approved.

- [ ] **Step 6: Commit gate or review corrections when needed**

If verification changed tracked files:

```bash
git add -u
git commit -m "fix: harden season availability contracts"
```

If no tracked file changed, retain the clean HEAD and attach the command evidence to the implementation report.

---

### Task 9: Publish, Deploy, and Smoke-Test v1.1

**Files:**
- Modify in the `sb` repository: `argocd/clusters/superbloom/infra/homelab-api/resources/deployment.yaml`
- Verify in this repository: `.github/workflows/build-homelab-api.yml`
- Verify in this repository: `.github/workflows/release-homelab-cli.yml`

**Interfaces:**
- Consumes: Reviewed Task 8 commit, GitHub Actions, immutable GHCR digests, existing `homelab-api` ArgoCD workload, Mac release installer path, and Tailscale endpoint.
- Produces: deployed API `1.1`, checksum-verified `homelab-v1.1.0` Mac CLI, and a read-only production result for TMDB ID `60625`, season `3`.

- [ ] **Step 1: Merge the reviewed application branch and publish it**

From the primary `homelab-mcp` checkout, fast-forward only:

```bash
git merge --ff-only feat/media-season-availability
git push origin main
```

Watch the `Build homelab-api` workflow triggered by that exact commit and require a successful conclusion.

- [ ] **Step 2: Resolve and record the immutable API digest**

Use the successful workflow's published `ghcr.io/saavy1/homelab-api` metadata and `docker buildx imagetools inspect` to obtain the 64-hex `sha256` digest. Reject a mutable tag, a digest from another commit, or a digest that is not 64 hexadecimal characters.

- [ ] **Step 3: Update and validate the GitOps workload**

In the dedicated `sb` worktree, replace only the existing `image:` digest in `argocd/clusters/superbloom/infra/homelab-api/resources/deployment.yaml`. Preserve namespace, resources, probes, secrets, ports, Tailscale annotations, and parent kustomizations. Validate:

```bash
kubectl kustomize argocd/clusters/superbloom/infra/homelab-api/resources >/dev/null
git diff --check
git diff -- argocd/clusters/superbloom/infra/homelab-api/resources/deployment.yaml
```

The diff must contain exactly one old digest and one new digest.

- [ ] **Step 4: Publish the GitOps change and prove rollout**

```bash
git add argocd/clusters/superbloom/infra/homelab-api/resources/deployment.yaml
git commit -m "feat: deploy media season availability"
git merge --ff-only feat/media-season-availability-deploy
git push origin main
```

Wait for ArgoCD `infra-homelab-api` to report `Synced Healthy`, then require the Deployment to report `1/1` ready and its pod image ID to equal the selected immutable digest.

- [ ] **Step 5: Verify the deployed API before releasing the CLI**

From the Mac over the existing Tailscale endpoint:

```bash
curl --fail --silent http://homelab-api.tailc2db57.ts.net:8080/livez
curl --fail --silent http://homelab-api.tailc2db57.ts.net:8080/readyz
curl --fail --silent http://homelab-api.tailc2db57.ts.net:8080/api/v1/capabilities
```

Require `livez` and `readyz` to return `ok`; capabilities must report `api.major == 1`, `api.minor == 1`, and contain `media.library.availability`.

- [ ] **Step 6: Tag and verify the macOS ARM64 release**

```bash
git tag -a homelab-v1.1.0 -m "homelab CLI v1.1.0"
git push origin homelab-v1.1.0
```

Watch `Release homelab CLI` to success. Download `homelab-aarch64-apple-darwin.tar.gz` and its `.sha256`, run `shasum -a 256 -c`, extract it, and install mode `0755` at `~/.local/bin/homelab` on `saavys-mac-mini-3`.

- [ ] **Step 7: Run the production CLI smoke**

On the Mac, with `HOMELAB_API_URL=http://homelab-api.tailc2db57.ts.net:8080/api/v1`:

```bash
homelab media search --query "Rick and Morty"
homelab media library availability --media-id 60625 --season 3
```

Require one JSON document per invocation, exit `0`, operation `media.library.availability`, risk `read`, series media ID `60625`, season `3`, internally consistent counts (`available_count + missing_count == expected_count` for each summary), no raw backend/credential fields, and a corresponding redacted request-completion event with the same request ID.

- [ ] **Step 8: Record rollback anchors and final evidence**

Record the previous API digest, new API digest, application commit, GitOps commit, release workflow URL, CLI archive checksum, capability response, production response, and completion-event correlation ID. If smoke fails, restore the prior immutable digest and reinstall checksum-verified `homelab-v1.0.1`; this feature has no persisted state or schema migration.
