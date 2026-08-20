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
