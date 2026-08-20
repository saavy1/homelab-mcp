use crate::MediaError;
use chrono::NaiveDate;
use homelab_api_model::{
    AvailabilityEpisode, AvailabilitySeries, CompletenessStatus, CompletenessSummary,
    EpisodePresence, EpisodeReleaseStatus, SeasonAvailability,
};
use std::collections::{HashMap, HashSet};

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

pub(crate) fn compare_season_availability(
    expected: ExpectedSeason,
    actual: Option<LibrarySeason>,
    as_of: NaiveDate,
) -> Result<SeasonAvailability, MediaError> {
    let ExpectedSeason {
        media_id,
        title,
        season,
        episodes: expected_episodes,
    } = expected;

    let mut expected_tmdb_ids = HashSet::with_capacity(expected_episodes.len());
    let mut expected_numbers = HashSet::with_capacity(expected_episodes.len());
    for episode in &expected_episodes {
        if let Some(tmdb_id) = nonempty_tmdb_id(&episode.tmdb_id)
            && !expected_tmdb_ids.insert(tmdb_id.to_owned())
        {
            return Err(MediaError::Conflict);
        }
        if !expected_numbers.insert((season, episode.episode_number)) {
            return Err(MediaError::Conflict);
        }
    }

    let mut actual_tmdb_ids = HashMap::new();
    let mut actual_numbers = HashMap::<(u32, u32), Vec<usize>>::new();
    if let Some(library) = &actual {
        actual_tmdb_ids.reserve(library.episodes.len());
        actual_numbers.reserve(library.episodes.len());
        for (index, episode) in library.episodes.iter().enumerate() {
            if let Some(tmdb_id) = episode.tmdb_id.as_deref().and_then(nonempty_tmdb_id)
                && actual_tmdb_ids.insert(tmdb_id.to_owned(), index).is_some()
            {
                return Err(MediaError::Conflict);
            }
            actual_numbers
                .entry((episode.season_number, episode.episode_number))
                .or_default()
                .push(index);
        }
    }

    let mut consumed = HashSet::with_capacity(expected_episodes.len());
    let mut aired_expected = 0_usize;
    let mut aired_available = 0_usize;
    let mut unknown_count = 0_usize;
    let mut unknown_missing = 0_usize;
    let mut announced_available = 0_usize;
    let mut next_airing = None;
    let mut episodes = actual
        .as_ref()
        .map(|_| Vec::with_capacity(expected_episodes.len()));

    for episode in expected_episodes {
        let matched = match &actual {
            Some(library) => match_actual_episode(
                &episode,
                season,
                &library.episodes,
                &actual_tmdb_ids,
                &actual_numbers,
                &mut consumed,
            )?,
            None => false,
        };
        let release_status = match episode.air_date {
            Some(air_date) if air_date <= as_of => EpisodeReleaseStatus::Aired,
            Some(_) => EpisodeReleaseStatus::Future,
            None => EpisodeReleaseStatus::Unknown,
        };
        let presence = if matched {
            announced_available += 1;
            EpisodePresence::Available
        } else {
            EpisodePresence::Missing
        };

        match release_status {
            EpisodeReleaseStatus::Aired => {
                aired_expected += 1;
                if matched {
                    aired_available += 1;
                }
            }
            EpisodeReleaseStatus::Future => {}
            EpisodeReleaseStatus::Unknown => {
                unknown_count += 1;
                if !matched {
                    unknown_missing += 1;
                }
            }
        }

        let availability_episode = AvailabilityEpisode {
            episode_id: episode.tmdb_id,
            episode_number: episode.episode_number,
            title: episode.title,
            air_date: episode.air_date,
            release_status,
            presence,
        };
        if release_status == EpisodeReleaseStatus::Future
            && next_airing.as_ref().is_none_or(|next: &AvailabilityEpisode| {
                (availability_episode.air_date, availability_episode.episode_number)
                    < (next.air_date, next.episode_number)
            })
        {
            next_airing = Some(availability_episode.clone());
        }
        if let Some(episodes) = &mut episodes {
            episodes.push(availability_episode);
        }
    }

    if let Some(episodes) = &mut episodes {
        episodes.sort_by_key(|episode| episode.episode_number);
    }

    let announced_expected = expected_numbers.len();
    let aired_missing = aired_expected
        .checked_sub(aired_available)
        .ok_or(MediaError::Internal)?;
    let announced_missing = announced_expected
        .checked_sub(announced_available)
        .ok_or(MediaError::Internal)?;
    let aired_status = if aired_missing > 0 {
        CompletenessStatus::Incomplete
    } else if unknown_missing > 0 {
        CompletenessStatus::Unknown
    } else {
        CompletenessStatus::Complete
    };
    let announced_status = if announced_missing > 0 {
        CompletenessStatus::Incomplete
    } else {
        CompletenessStatus::Complete
    };

    Ok(SeasonAvailability {
        series: AvailabilitySeries {
            media_id,
            jellyfin_id: actual.as_ref().map(|library| library.series_id.clone()),
            title,
        },
        season,
        as_of,
        in_library: actual.is_some(),
        aired: summary(aired_status, aired_expected, aired_available)?,
        announced: summary(
            announced_status,
            announced_expected,
            announced_available,
        )?,
        unknown_air_date_count: checked_count(unknown_count)?,
        next_airing,
        episodes,
    })
}

fn match_actual_episode(
    expected: &ExpectedEpisode,
    season: u32,
    actual: &[LibraryEpisode],
    actual_tmdb_ids: &HashMap<String, usize>,
    actual_numbers: &HashMap<(u32, u32), Vec<usize>>,
    consumed: &mut HashSet<usize>,
) -> Result<bool, MediaError> {
    let expected_tmdb_id = nonempty_tmdb_id(&expected.tmdb_id);
    if let Some(index) = expected_tmdb_id.and_then(|tmdb_id| actual_tmdb_ids.get(tmdb_id).copied())
    {
        if !consumed.insert(index) {
            return Err(MediaError::Conflict);
        }
        return Ok(true);
    }

    let Some(candidates) = actual_numbers.get(&(season, expected.episode_number)) else {
        return Ok(false);
    };
    let mut candidates = candidates.iter().copied().filter(|index| {
        expected_tmdb_id.is_none()
            || actual[*index]
                .tmdb_id
                .as_deref()
                .and_then(nonempty_tmdb_id)
                .is_none()
    });
    let Some(index) = candidates.next() else {
        return Ok(false);
    };
    if candidates.next().is_some() || !consumed.insert(index) {
        return Err(MediaError::Conflict);
    }
    Ok(true)
}

fn nonempty_tmdb_id(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

fn summary(
    status: CompletenessStatus,
    expected_count: usize,
    available_count: usize,
) -> Result<CompletenessSummary, MediaError> {
    let missing_count = expected_count
        .checked_sub(available_count)
        .ok_or(MediaError::Internal)?;
    Ok(CompletenessSummary {
        status,
        expected_count: checked_count(expected_count)?,
        available_count: checked_count(available_count)?,
        missing_count: checked_count(missing_count)?,
    })
}

fn checked_count(count: usize) -> Result<u32, MediaError> {
    u32::try_from(count).map_err(|_| MediaError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaError;
    use homelab_api_model::{
        CompletenessStatus, EpisodePresence, EpisodeReleaseStatus, SeasonAvailability,
    };

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn as_of() -> NaiveDate {
        date(2026, 8, 19)
    }

    fn episode(tmdb_id: &str, episode_number: u32, air_date: Option<NaiveDate>) -> ExpectedEpisode {
        ExpectedEpisode {
            tmdb_id: tmdb_id.into(),
            episode_number,
            title: format!("Episode {episode_number}"),
            air_date,
        }
    }

    fn expected(season: u32, episodes: Vec<ExpectedEpisode>) -> ExpectedSeason {
        ExpectedSeason {
            media_id: "60625".into(),
            title: "Series".into(),
            season,
            episodes,
        }
    }

    fn library_episode(
        tmdb_id: Option<&str>,
        season_number: u32,
        episode_number: u32,
    ) -> LibraryEpisode {
        LibraryEpisode {
            jellyfin_id: format!("opaque-{season_number}-{episode_number}"),
            tmdb_id: tmdb_id.map(str::to_owned),
            season_number,
            episode_number,
        }
    }

    fn actual(episodes: Vec<LibraryEpisode>) -> Option<LibrarySeason> {
        Some(LibrarySeason {
            series_id: "series-1".into(),
            episodes,
        })
    }

    fn compare(
        expected: ExpectedSeason,
        actual: Option<LibrarySeason>,
    ) -> SeasonAvailability {
        compare_season_availability(expected, actual, as_of()).unwrap()
    }

    #[test]
    fn all_aired_and_announced_episodes_are_available() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("302", 2, Some(date(2026, 9, 1))),
                    episode("301", 1, Some(date(2026, 8, 1))),
                ],
            ),
            actual(vec![
                library_episode(Some("302"), 3, 2),
                library_episode(Some("301"), 3, 1),
            ]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Complete);
        assert_eq!(result.aired.expected_count, 1);
        assert_eq!(result.aired.available_count, 1);
        assert_eq!(result.aired.missing_count, 0);
        assert_eq!(result.announced.status, CompletenessStatus::Complete);
        assert_eq!(result.announced.expected_count, 2);
        assert_eq!(result.announced.available_count, 2);
        assert_eq!(result.announced.missing_count, 0);
        assert_eq!(
            result.next_airing.as_ref().map(|episode| (
                episode.episode_number,
                episode.release_status,
                episode.presence,
            )),
            Some((
                2,
                EpisodeReleaseStatus::Future,
                EpisodePresence::Available,
            ))
        );
        assert_eq!(
            result
                .episodes
                .unwrap()
                .iter()
                .map(|episode| episode.episode_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn missing_aired_episode_is_incomplete() {
        let result = compare(
            expected(3, vec![episode("301", 1, Some(date(2026, 8, 1)))]),
            actual(vec![]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Incomplete);
        assert_eq!(result.aired.expected_count, 1);
        assert_eq!(result.aired.available_count, 0);
        assert_eq!(result.aired.missing_count, 1);
        assert_eq!(
            result.episodes.unwrap()[0].presence,
            EpisodePresence::Missing
        );
    }

    #[test]
    fn only_a_future_episode_missing_keeps_aired_complete() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("302", 2, Some(date(2026, 9, 1))),
                ],
            ),
            actual(vec![library_episode(Some("301"), 3, 1)]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Complete);
        assert_eq!(result.aired.missing_count, 0);
        assert_eq!(result.announced.status, CompletenessStatus::Incomplete);
        assert_eq!(result.announced.expected_count, 2);
        assert_eq!(result.announced.available_count, 1);
        assert_eq!(result.announced.missing_count, 1);
    }

    #[test]
    fn missing_unknown_date_episode_makes_aired_status_unknown() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("302", 2, None),
                ],
            ),
            actual(vec![library_episode(Some("301"), 3, 1)]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Unknown);
        assert_eq!(result.aired.expected_count, 1);
        assert_eq!(result.aired.available_count, 1);
        assert_eq!(result.aired.missing_count, 0);
        assert_eq!(result.announced.status, CompletenessStatus::Incomplete);
        assert_eq!(result.unknown_air_date_count, 1);
    }

    #[test]
    fn missing_aired_episode_takes_precedence_over_missing_unknown_date_episode() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("302", 2, None),
                ],
            ),
            actual(vec![]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Incomplete);
        assert_eq!(result.aired.expected_count, 1);
        assert_eq!(result.aired.available_count, 0);
        assert_eq!(result.aired.missing_count, 1);
        assert_eq!(result.unknown_air_date_count, 1);
        assert_eq!(result.announced.missing_count, 2);
    }

    #[test]
    fn available_unknown_date_episode_does_not_make_aired_status_unknown() {
        let result = compare(
            expected(3, vec![episode("301", 1, None)]),
            actual(vec![library_episode(Some("301"), 3, 1)]),
        );

        assert_eq!(result.aired.status, CompletenessStatus::Complete);
        assert_eq!(result.aired.expected_count, 0);
        assert_eq!(result.aired.available_count, 0);
        assert_eq!(result.aired.missing_count, 0);
        assert_eq!(result.announced.status, CompletenessStatus::Complete);
        assert_eq!(result.unknown_air_date_count, 1);
    }

    #[test]
    fn absent_series_is_compact_and_preserves_next_airing() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("302", 2, Some(date(2026, 9, 1))),
                ],
            ),
            None,
        );

        assert!(!result.in_library);
        assert_eq!(result.series.media_id, "60625");
        assert_eq!(result.series.jellyfin_id, None);
        assert_eq!(result.episodes, None);
        assert_eq!(result.aired.available_count, 0);
        assert_eq!(result.announced.available_count, 0);
        assert_eq!(
            result.next_airing.as_ref().map(|episode| (
                episode.episode_number,
                episode.presence,
                episode.release_status
            )),
            Some((2, EpisodePresence::Missing, EpisodeReleaseStatus::Future))
        );
    }

    #[test]
    fn season_zero_is_retained() {
        let result = compare(expected(0, vec![]), actual(vec![]));

        assert_eq!(result.season, 0);
    }

    #[test]
    fn exact_tmdb_provider_match_wins_before_an_eligible_number_fallback() {
        let error = compare_season_availability(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("", 2, Some(date(2026, 8, 2))),
                ],
            ),
            actual(vec![
                library_episode(Some("301"), 3, 2),
                library_episode(None, 3, 1),
            ]),
            as_of(),
        )
        .unwrap_err();

        assert!(matches!(error, MediaError::Conflict));
    }

    #[test]
    fn number_fallback_works_when_either_side_lacks_a_tmdb_id() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("", 1, Some(date(2026, 8, 1))),
                    episode("302", 2, Some(date(2026, 8, 2))),
                ],
            ),
            actual(vec![
                library_episode(Some("different"), 3, 1),
                library_episode(None, 3, 2),
            ]),
        );

        assert!(result
            .episodes
            .unwrap()
            .iter()
            .all(|episode| episode.presence == EpisodePresence::Available));
    }

    #[test]
    fn duplicate_expected_tmdb_ids_conflict() {
        let error = compare_season_availability(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("301", 2, Some(date(2026, 8, 2))),
                ],
            ),
            actual(vec![]),
            as_of(),
        )
        .unwrap_err();

        assert!(matches!(error, MediaError::Conflict));
    }

    #[test]
    fn duplicate_actual_tmdb_ids_conflict() {
        let error = compare_season_availability(
            expected(3, vec![episode("301", 1, Some(date(2026, 8, 1)))]),
            actual(vec![
                library_episode(Some("301"), 3, 1),
                library_episode(Some("301"), 3, 2),
            ]),
            as_of(),
        )
        .unwrap_err();

        assert!(matches!(error, MediaError::Conflict));
    }

    #[test]
    fn duplicate_number_fallback_candidates_conflict() {
        let error = compare_season_availability(
            expected(3, vec![episode("301", 1, Some(date(2026, 8, 1)))]),
            actual(vec![
                library_episode(None, 3, 1),
                library_episode(None, 3, 1),
            ]),
            as_of(),
        )
        .unwrap_err();

        assert!(matches!(error, MediaError::Conflict));
    }

    #[test]
    fn one_actual_record_cannot_satisfy_two_expected_records() {
        let error = compare_season_availability(
            expected(
                3,
                vec![
                    episode("301", 1, Some(date(2026, 8, 1))),
                    episode("", 2, Some(date(2026, 8, 2))),
                ],
            ),
            actual(vec![library_episode(Some("301"), 3, 2)]),
            as_of(),
        )
        .unwrap_err();

        assert!(matches!(error, MediaError::Conflict));
    }

    #[test]
    fn extra_jellyfin_episodes_are_ignored() {
        let result = compare(
            expected(3, vec![episode("301", 1, Some(date(2026, 8, 1)))]),
            actual(vec![
                library_episode(Some("301"), 3, 1),
                library_episode(Some("999"), 3, 99),
            ]),
        );

        let episodes = result.episodes.unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].episode_id, "301");
    }

    #[test]
    fn equal_date_next_airings_use_episode_number_as_tie_break() {
        let result = compare(
            expected(
                3,
                vec![
                    episode("302", 2, Some(date(2026, 9, 1))),
                    episode("301", 1, Some(date(2026, 9, 1))),
                ],
            ),
            actual(vec![]),
        );

        assert_eq!(result.next_airing.unwrap().episode_number, 1);
    }

    #[test]
    fn air_date_equal_to_as_of_is_aired() {
        let result = compare(
            expected(3, vec![episode("301", 1, Some(as_of()))]),
            actual(vec![]),
        );

        let episode = &result.episodes.unwrap()[0];
        assert_eq!(episode.release_status, EpisodeReleaseStatus::Aired);
        assert_eq!(result.aired.expected_count, 1);
        assert_eq!(result.aired.missing_count, 1);
    }

    #[test]
    fn empty_expected_season_has_complete_zero_count_summaries() {
        let result = compare(expected(3, vec![]), actual(vec![]));

        assert!(result.in_library);
        assert_eq!(result.episodes, Some(vec![]));
        assert_eq!(result.aired.status, CompletenessStatus::Complete);
        assert_eq!(result.aired.expected_count, 0);
        assert_eq!(result.aired.available_count, 0);
        assert_eq!(result.aired.missing_count, 0);
        assert_eq!(result.announced.status, CompletenessStatus::Complete);
        assert_eq!(result.announced.expected_count, 0);
        assert_eq!(result.announced.available_count, 0);
        assert_eq!(result.announced.missing_count, 0);
    }

    #[test]
    fn oversized_count_takes_the_internal_error_path() {
        assert!(matches!(checked_count(usize::MAX), Err(MediaError::Internal)));
    }
}
