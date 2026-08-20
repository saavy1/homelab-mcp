# Media Season Availability Design

**Date:** 2026-08-19
**Status:** Approved

## Summary

Add one read-only, agent-friendly operation that answers whether a specific announced television season is present in Jellyfin. Jellyseerr supplies the expected TMDB season and episode schedule; Jellyfin supplies episodes actually present in the library. `homelab-media` joins those typed results and reports both aired completeness and entire-announced-season completeness, including future air dates and honest handling of unknown dates.

The operation is identified by the TMDB/Jellyseerr catalog ID returned by `homelab media search`. It does not accept a fuzzy title, select among ambiguous shows, expose raw backend responses, or add Sonarr credentials.

## Goals

- Answer questions such as “Do we have season 3 of this show?” with one typed CLI call.
- Distinguish episodes already aired from announced future episodes.
- Report the next scheduled episode when one is known.
- Compare expected catalog episodes against media actually visible in Jellyfin.
- Return normal absence as data, not as an error.
- Preserve the curated API boundary, backend credential isolation, redaction, request IDs, provenance, and stable exit behavior.

## Non-goals

- Adding Sonarr or Radarr clients, credentials, monitoring state, queue state, or file-management operations.
- Searching by title inside the availability operation.
- Choosing automatically among ambiguous catalog results.
- Exposing Jellyfin paths, backend URLs, API keys, raw JSON, or generic query methods.
- Repairing missing media, requesting a show, refreshing Jellyfin, or triggering downloads.
- Determining file quality, codec, language, edition, or disk location.
- Treating a missing air date as proof that an episode is future or already aired.
- Supporting movies in this operation.

## Decisions

1. The command requires a positive TMDB/Jellyseerr media ID and an explicit nonnegative season number.
2. Season `0` is valid and represents specials.
3. Jellyseerr is authoritative for announced episodes, TMDB episode IDs, titles, numbers, and air dates.
4. Jellyfin is authoritative for whether an episode is present in the media library.
5. Series and episodes match by TMDB provider ID first, then by exact season and episode number when Jellyfin lacks a TMDB episode ID.
6. Duplicate provider-ID or fallback matches fail as a conflict rather than selecting arbitrarily.
7. A show absent from Jellyfin is a successful compact result with `in_library: false`; episode detail is omitted.
8. Aired completeness is tri-state: `complete`, `incomplete`, or `unknown`.
9. Announced completeness is `complete` or `incomplete`; it includes future and undated announced episodes.
10. The current UTC date is recorded as `as_of` and supplied explicitly to the pure comparison function for deterministic tests.
11. This backward-compatible addition increments API minor version from `1.0` to `1.1`; major remains `1`.

## CLI Contract

```bash
homelab media library availability \
  --media-id 60625 \
  --season 3
```

Arguments:

- `--media-id <positive-i64>`: required TMDB/Jellyseerr TV catalog ID.
- `--season <nonnegative-u32>`: required season number; `0` permits specials.
- Existing global `--request-id` and `--output json|table` options remain available.

JSON remains the default. A missing argument or invalid number is a local structured validation failure with exit code `2`; no HTTP request is sent.

Table output is a single summary row with columns:

```text
TITLE  SEASON  IN_LIBRARY  AIRED  ANNOUNCED  AVAILABLE  EXPECTED  NEXT_AIRING
```

Episode-level detail remains available in JSON and is not expanded into multiple table rows.

## HTTP Contract

```http
GET /api/v1/media/library/availability?media_id=60625&season=3
```

- Operation: `media.library.availability`
- Risk: `read`
- Request body: none
- Query fields: exactly `media_id` and `season`
- Response: `OperationEnvelope<SeasonAvailability>`
- Capability list: append `media.library.availability`

The route accepts the existing request-ID header, uses the existing read timeout, response envelope, completion event, and redaction behavior, and never retries inside the server.

## Public Models

All models use snake-case JSON and `JsonSchema`. Public models contain normalized fields only.

```rust
pub struct SeasonAvailabilityQuery {
    pub media_id: i64,
    pub season: u32,
}

pub enum CompletenessStatus {
    Complete,
    Incomplete,
    Unknown,
}

pub enum EpisodeReleaseStatus {
    Aired,
    Future,
    Unknown,
}

pub enum EpisodePresence {
    Available,
    Missing,
}

pub struct AvailabilitySeries {
    pub media_id: String,
    pub jellyfin_id: Option<String>,
    pub title: String,
}

pub struct CompletenessSummary {
    pub status: CompletenessStatus,
    pub expected_count: u32,
    pub available_count: u32,
    pub missing_count: u32,
}

pub struct AvailabilityEpisode {
    pub episode_id: String,
    pub episode_number: u32,
    pub title: String,
    pub air_date: Option<chrono::NaiveDate>,
    pub release_status: EpisodeReleaseStatus,
    pub presence: EpisodePresence,
}

pub struct SeasonAvailability {
    pub series: AvailabilitySeries,
    pub season: u32,
    pub as_of: chrono::NaiveDate,
    pub in_library: bool,
    pub aired: CompletenessSummary,
    pub announced: CompletenessSummary,
    pub unknown_air_date_count: u32,
    pub next_airing: Option<AvailabilityEpisode>,
    pub episodes: Option<Vec<AvailabilityEpisode>>,
}
```

`CompletenessStatus::Unknown` is valid for `aired.status` only. `announced.status` must never be `unknown`.

Counts use checked conversions. An upstream result too large for `u32`, duplicate identity, or contradictory normalized record returns an internal/conflict error rather than truncating or guessing.

## Representative Response

```json
{
  "ok": true,
  "operation": "media.library.availability",
  "request_id": "req-...",
  "risk": "read",
  "summary": { "text": "season availability compared" },
  "data": {
    "series": {
      "media_id": "60625",
      "jellyfin_id": "opaque-jellyfin-id",
      "title": "Rick and Morty"
    },
    "season": 3,
    "as_of": "2026-08-20",
    "in_library": true,
    "aired": {
      "status": "incomplete",
      "expected_count": 10,
      "available_count": 9,
      "missing_count": 1
    },
    "announced": {
      "status": "incomplete",
      "expected_count": 10,
      "available_count": 9,
      "missing_count": 1
    },
    "unknown_air_date_count": 0,
    "next_airing": null,
    "episodes": [
      {
        "episode_id": "123456",
        "episode_number": 7,
        "title": "The Ricklantis Mixup",
        "air_date": "2017-09-10",
        "release_status": "aired",
        "presence": "missing"
      }
    ]
  },
  "provenance": {
    "service": "homelab-media",
    "timestamp": "2026-08-20T00:00:00Z"
  }
}
```
The episode list in this representative response is abridged.


The `episodes` array contains every expected episode, ordered by episode number, when `in_library` is true. This makes the result directly auditable and lets an agent answer follow-up questions without another call.

When `in_library` is false:

- `jellyfin_id` is `null`;
- `episodes` is `null`;
- `available_count` is zero;
- announced and aired counts are still computed from Jellyseerr;
- `next_airing` is still returned when known.

## Backend Data Acquisition

### Jellyseerr expected season

Call:

```http
GET /api/v1/tv/{media_id}/season/{season}
X-Api-Key: <in-cluster credential>
```

Normalize only:

- show/catalog identity already supplied by the caller;
- season number;
- TMDB episode ID;
- episode number;
- title;
- nullable `airDate`.

The season response does not reliably carry the series title, so obtain the normalized title from the existing TV-details endpoint:

```http
GET /api/v1/tv/{media_id}
```

Execute these two independent reads concurrently. A show or season `404` maps to `not_found`. Invalid JSON or missing required episode identity/number/title maps to the existing redacted invalid-response/internal path. An invalid air date is normalized to `None` and classified as unknown rather than failing the operation.

### Jellyfin series identity

Page through server-visible series items:

```http
GET /Items?Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&StartIndex={n}&Limit=200
X-Emby-Token: <in-cluster credential>
```

Continue until the returned item count reaches `TotalRecordCount` or a short/empty page proves completion. Match `ProviderIds.Tmdb` exactly against the requested `media_id` string.

- No match: normal absence.
- One match: use its opaque Jellyfin `Id`.
- More than one match: conflict.

Do not use unsupported Emby-style provider-ID query parameters and do not fuzzy-match titles.

### Jellyfin actual episodes

For a matched series, page through real episodes:

```http
GET /Shows/{series_id}/Episodes?Season={season}&IsMissing=false&Fields=ProviderIds&StartIndex={n}&Limit=200
X-Emby-Token: <in-cluster credential>
```

Normalize only:

- opaque Jellyfin item ID for internal diagnostics, never the public response;
- `ProviderIds.Tmdb` when present;
- `ParentIndexNumber` as season number;
- `IndexNumber` as episode number.

Only records with the requested season and a valid episode number participate. `IsMissing=false` is mandatory so virtual/missing metadata entries are not mistaken for downloaded media.

## Matching Rules

For each expected Jellyseerr episode:

1. Match an actual Jellyfin episode with the same TMDB episode provider ID.
2. If the expected or actual record lacks that provider ID, fall back to exact `(season_number, episode_number)` equality.
3. Never fall back to title, air date, array position, or approximate matching.
4. Each expected and actual episode may match at most once.
5. Duplicate candidates or one actual record matching multiple expected records is a conflict.
6. Extra Jellyfin episodes not present in Jellyseerr do not affect completeness and are not returned.

The implementation computes matching in linear time using provider-ID and number maps; it must not perform an avoidable nested scan.

## Completeness Semantics

Partition expected episodes by Jellyseerr `air_date` relative to `as_of`:

- `aired`: date is on or before `as_of`;
- `future`: date is after `as_of`;
- `unknown`: date is absent or invalid.

Aired summary counts only known-aired expected episodes:

- `incomplete` if one or more known-aired episodes are missing;
- otherwise `unknown` if one or more unknown-date episodes are missing;
- otherwise `complete`.

Announced summary counts every expected episode, including future and unknown-date episodes:

- `complete` when every expected episode is available;
- `incomplete` otherwise.

`unknown_air_date_count` counts every expected episode without a usable date, whether available or missing.

`next_airing` is the future expected episode with the earliest air date; episode number breaks equal-date ties. It is returned whether that episode is already available or missing.

An empty announced season has zero counts, `complete` summaries, no next airing, and an empty episode list when the series exists. `in_library` remains the authoritative answer about whether Jellyfin contains the series.

## Application Layer

`MediaService::season_availability` orchestrates three typed backend reads:

1. Jellyseerr TV details and season details, concurrently where independent.
2. Jellyfin paginated series lookup.
3. Jellyfin paginated real-episode lookup only when a series match exists.
4. A pure comparison function receives normalized expected episodes, optional actual episodes, and explicit `as_of` date.

The pure function owns matching, partitioning, counts, ordering, next-airing selection, and invariants. Backend clients own HTTP and normalization only. API routes and CLI rendering contain no business logic.

## Error and Status Behavior

| Condition | API error code | Retryable | CLI exit |
|---|---|---:|---:|
| Missing/invalid CLI argument | `validation` locally | false | 2 |
| `media_id <= 0` | `validation` | false | 2 |
| Unknown Jellyseerr TV/season | `not_found` | false | 4 |
| Jellyfin series absent | success (`in_library: false`) | n/a | 0 |
| Duplicate identity/match | `conflict` | false | 4 |
| Backend timeout | `timeout` | true for this read | 5 |
| Backend connection/5xx/429 | `unavailable` | existing read policy | 5 |
| Invalid required upstream fields | `internal` | false | 1 |
| Missing/invalid air date only | success (`unknown`) | n/a | 0 |

Messages identify only the backend and normalized condition. They never include URLs, keys, raw bodies, file paths, or response fragments.

## Versioning and Compatibility

- `API_MAJOR` remains `1`.
- `API_MINOR` becomes `1`.
- Capabilities append `media.library.availability`.
- Existing routes, DTOs, CLI commands, and mutations are unchanged.
- Server deployment precedes CLI installation so the new read command never targets an older production server.
- Existing v1.0.x CLIs remain compatible with the v1.1 server.
- Release the new CLI as `homelab-v1.1.0`.

## Security and Resource Limits

- The operation is read-only and never triggers requests, downloads, refreshes, or mutations.
- Backend credentials remain in Kubernetes and are never returned or logged.
- Existing request/body limits, timeouts, correlation IDs, completion events, and redaction apply.
- Pagination uses a fixed page size of 200 and a defensive maximum of 10,000 series or episodes per backend traversal. Exceeding the cap returns an internal error rather than silently truncating.
- Query validation occurs before backend calls.
- No backend response uses `serde_json::Value` outside private client normalization code.

## Testing

### Model contracts

- Exact snake-case serialization for all enums and DTOs.
- `NaiveDate` uses `YYYY-MM-DD`.
- Unknown query fields remain consistent with existing query compatibility policy.
- API version is `1.1` and capability is present exactly once.

### Jellyseerr client

- TV title and season detail normalization.
- Nullable/invalid air dates become unknown.
- TV and season `404` handling.
- Required-field decode failures are redacted.

### Jellyfin client

- Paginated series traversal and exact TMDB provider match.
- Absent and duplicate series behavior.
- Paginated `IsMissing=false` episode traversal.
- Provider IDs and numeric fallback fields normalize correctly.
- Defensive pagination cap and repeated/empty page termination.

### Comparison function

- Complete aired and announced season.
- Partial aired season.
- Complete aired but incomplete announced season with future episodes.
- Missing unknown-date episode produces aired `unknown`.
- Available unknown-date episode does not falsely make known-aired completeness incomplete.
- Absent Jellyfin series returns compact success.
- Season `0` specials.
- Provider-ID primary matching and number fallback.
- Duplicate matching conflicts.
- Extra Jellyfin episodes are ignored.
- Stable episode ordering and next-airing tie break.
- Deterministic boundary where `air_date == as_of` counts as aired.

### API, client, and CLI

- Exact route and query encoding.
- Invalid media ID/season produces structured validation before backend calls.
- Typed response and error envelopes.
- CLI subprocess JSON and table output.
- Missing arguments exit `2` without HTTP.
- Existing commands remain unchanged.

### Production smoke

After review and all workspace gates:

1. Publish and deploy the v1.1 API image by immutable digest.
2. Publish and checksum `homelab-v1.1.0` for `aarch64-apple-darwin`.
3. Install the CLI on `saavys-mac-mini-3`.
4. Verify capabilities report API `1.1` and the new operation.
5. Run:

```bash
homelab media library availability --media-id 60625 --season 3
```

6. Confirm one structured envelope, no credential/raw backend data, internally consistent counts, and a matching request-completion correlation ID.
7. This smoke test is read-only; do not trigger a request, refresh, download, or deletion.

## Rollback

- CLI rollback: reinstall the checksum-verified `homelab-v1.0.1` release.
- API rollback: restore the previous immutable image digest in the GitOps Deployment and reconcile ArgoCD.
- The addition has no persisted state or schema migration.
- Existing v1 operations remain available throughout rollback.

## Follow-up Possibilities

These are explicitly outside this implementation:

- Sonarr/Radarr operational availability, monitoring, and file-quality views.
- Multi-season or whole-series aggregation.
- Movie availability.
- Quality profile, codec, language, or disk-path reporting.
- A high-level fuzzy-title command; agents should continue using `media search` first.
