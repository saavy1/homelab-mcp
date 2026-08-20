# Catalog item fix report

## Scope and base

- Base commit: `f12e2d3` (`style: format replacement API workspace`).
- Affected packages only: `homelab-api-model`, `homelab-media`, `homelab-api`, `homelab-client`, and `homelab-cli`.
- No broad workspace tests or workspace formatter were run.

## RED evidence

All regression tests were written before production changes.

1. `cargo test -p homelab-api-model item_details_query_requires_a_catalog_media_type`
   - Exit `101`.
   - Expected failure: `ItemDetailsQuery` did not exist (`E0425`/`E0422`).
2. `cargo test -p homelab-media --test jellyseerr item_details_selects_movie_or_tv_catalog_endpoint`
   - Exit `101`.
   - Expected failure: `JellyseerrClient` had no `item_details` method (`E0599`).
3. `cargo test -p homelab-media --test service item_details_uses_jellyseerr_catalog_and_never_jellyfin`
   - Exit `101`.
   - Expected failure: `MediaService::item_details` accepted no media type (`E0061`).
4. `cargo test -p homelab-api --test api item_details_requires_exact_media_type_query_and_selects_catalog_endpoint -- --exact`
   - Exit `101`; one focused test failed.
   - Expected failure: TV lookup returned normalized `movie`, demonstrating that the API ignored `media_type` and used Jellyfin.
5. `cargo test -p homelab-client --test client every_operation_uses_a_typed_fixed_route -- --exact`
   - Exit `101`.
   - Expected failures: missing `ItemDetailsQuery` import and old two-argument `item_details` method (`E0432`/`E0061`).
6. `cargo test -p homelab-cli --test cli search_result_id_and_type_are_accepted_by_item_show_with_exact_query -- --exact`
   - Exit `101`; one focused test failed.
   - Expected failure: `media item show --media-type tv` exited `2` because the required flag was not implemented.

## GREEN evidence

Focused regression tests:

1. `cargo test -p homelab-api-model item_details_query_requires_a_catalog_media_type -- --exact`
   - Passed: 1 test.
2. `cargo test -p homelab-media --test jellyseerr item_details_selects_movie_or_tv_catalog_endpoint -- --exact`
   - Passed: 1 test.
3. `cargo test -p homelab-media --test service item_details_uses_jellyseerr_catalog_and_never_jellyfin -- --exact`
   - Passed: 1 test.
4. `cargo test -p homelab-api --test api item_details_requires_exact_media_type_query_and_selects_catalog_endpoint -- --exact`
   - Passed: 1 test.
5. `cargo test -p homelab-client --test client every_operation_uses_a_typed_fixed_route -- --exact`
   - Passed: 1 test.
6. `cargo test -p homelab-cli --test cli search_result_id_and_type_are_accepted_by_item_show_with_exact_query -- --exact`
   - Passed: 1 test.
7. `cargo test -p homelab-cli --test cli invalid_arguments_exit_two_without_http_and_missing_config_is_structured -- --exact`
   - Passed: 1 test.

Affected-package tests:

```text
cargo test -p homelab-api-model  -> 7 passed (2 suites)
cargo test -p homelab-media      -> 25 passed (7 suites)
cargo test -p homelab-api        -> 16 passed (4 suites)
cargo test -p homelab-client     -> 6 passed (3 suites)
cargo test -p homelab-cli        -> 9 passed (2 suites)
```

Warnings-as-errors Clippy:

```text
cargo clippy -p homelab-api-model -p homelab-media -p homelab-api -p homelab-client -p homelab-cli --all-targets -- -D warnings
OK
```

## Design compliance

- `ItemDetailsQuery` is typed and requires exactly `media_type: movie|tv`; the API rejects missing, invalid, or unknown query fields with the existing structured validation envelope.
- The public route remains `GET /api/v1/media/items/{id}` and the operation remains `media.items.show`.
- The typed client emits the exact query contract: `?media_type=movie` or `?media_type=tv`.
- CLI `media item show` requires `--media-type movie|tv`; missing/invalid type and non-numeric item IDs exit `2` before HTTP.
- Catalog IDs are validated as non-empty ASCII-numeric values at the CLI, API, and Jellyseerr client boundaries.
- `60625` with type `tv` targets Jellyseerr `GET /api/v1/tv/60625`; movie selects `GET /api/v1/movie/{id}`.
- Search output (`id` plus `media_type`) passes directly into `media item show`; the subprocess test proves the exact resulting API URI.
- `MediaService::item_details` calls Jellyseerr and the focused service test proves the separately configured Jellyfin backend receives zero calls.
- Obsolete `JellyfinClient::get_item_details`, its normalizer/validation helpers, its focused test, and old Jellyfin item-detail API fixtures/callers were removed.
- Existing operation envelopes, redaction behavior, `API_MAJOR`/`API_MINOR`, and capability operation naming were unchanged.

## Concerns

None observed in the requested scope.


## Formatting cleanup verification

After `cargo fmt --all`, the final formatted tree was verified with:

```text
cargo test -p homelab-api-model -> 7 passed (2 suites)
cargo test -p homelab-media     -> 25 passed (7 suites)
cargo test -p homelab-api       -> 16 passed (4 suites)
cargo test -p homelab-client    -> 6 passed (3 suites)
cargo test -p homelab-cli       -> 9 passed (2 suites)
cargo clippy -p homelab-api-model -p homelab-media -p homelab-api -p homelab-client -p homelab-cli --all-targets -- -D warnings -> OK
cargo fmt --all -- --check -> passed
```