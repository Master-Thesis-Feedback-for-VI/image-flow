//! Timestamped GPS rows and searches relative to the first CSV timestamp.
//!
//! CSV headers are `timestamp,latitude,longitude`. Timestamps are Unix seconds,
//! including fractions. Parsing preserves row order and computes each offset
//! relative to the first row; supply chronologically ordered input.

use std::{io, ops::Deref};

use chrono::{DateTime, Duration, TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

use crate::location::coordinate_system::Coordinate;

/// One timestamped GPS sample, including an offset relative to its recording.
///
/// [`DataPointsGPS::from_csv`] computes the offset; direct deserialization
/// defaults it to zero if omitted. Coordinates are retained without validation.
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct TabularGPS {
    #[serde(deserialize_with = "de_ts")]
    timestamp: DateTime<Utc>,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    offset: TimeDelta,
}

impl TabularGPS {
    /// Return the sample's latitude and longitude.
    #[must_use]
    pub fn coordinates(&self) -> Coordinate {
        (*self).into()
    }
}

impl From<TabularGPS> for Coordinate {
    fn from(value: TabularGPS) -> Self {
        Self {
            lat: value.latitude,
            lon: value.longitude,
        }
    }
}

/// Ordered GPS samples with relative offsets computed by [`Self::from_csv`].
///
/// Read-only vector access is available through `Deref`. Direct Serde
/// deserialization does not compute recording-relative offsets.
#[derive(Deserialize, Debug, Clone)]
pub struct DataPointsGPS(Vec<TabularGPS>);

impl Deref for DataPointsGPS {
    type Target = Vec<TabularGPS>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DataPointsGPS {
    /// Parse GPS CSV and calculate offsets relative to the first row.
    ///
    /// Requires `timestamp`, `latitude`, and `longitude` headers. Timestamps are
    /// finite Unix seconds, optionally fractional. Empty input yields no samples;
    /// input order is preserved rather than sorted. Any supplied `offset` column
    /// is replaced by the calculated value.
    ///
    /// # Errors
    /// Returns a serialization error for CSV/read failures, missing or malformed
    /// fields, or invalid/out-of-range timestamps.
    pub fn from_csv(rdr: impl io::Read) -> crate::Result<Self> {
        let mut rdr = csv::Reader::from_reader(rdr);
        let mut points: Vec<TabularGPS> = rdr
            .deserialize()
            .collect::<Result<Vec<TabularGPS>, _>>()
            .map_err(crate::error::serialization)?;

        if let Some(first) = points
            .first()
            .map(|p| p.timestamp)
        {
            for p in &mut points {
                p.offset = p.timestamp - first;
            }
        }

        Ok(Self(points))
    }

    /// Iterate over samples within an inclusive window around `offset`.
    ///
    /// Uses the absolute value of `range_around_offset`, preserving input order.
    pub fn fetch(
        &self,
        offset: Duration,
        range_around_offset: Duration,
    ) -> impl Iterator<Item = &TabularGPS> + '_ {
        let range = range_around_offset.abs();
        self.0
            .iter()
            .filter(move |gps| {
                let gap = (gps.offset - offset).abs();
                gap <= range
            })
    }

    /// Return the nearest sample in an inclusive tolerance window, if any.
    ///
    /// Ties keep the first sample in input order. Negative tolerance is treated
    /// as its absolute value.
    #[must_use]
    pub fn fetch_best_match(
        &self,
        offset: Duration,
        range_around_offset: Duration,
    ) -> Option<&TabularGPS> {
        self.fetch(offset, range_around_offset)
            .best_match_by_distance(offset)
    }

    /// Iterate over samples between inclusive offset bounds in input order.
    ///
    /// # Panics
    /// This function panics if `lower_bound > upper_bound`.
    pub fn fetch_within_bounds(
        &self,
        lower_bound: Duration,
        upper_bound: Duration,
    ) -> impl Iterator<Item = &TabularGPS> + '_ {
        assert!(lower_bound <= upper_bound);

        self.0
            .iter()
            .filter(move |gps| {
                let offset = gps.offset;
                lower_bound <= offset && offset <= upper_bound
            })
    }

    /// Return all samples near a regular offset grid, preserving input order.
    ///
    /// Grid offsets start at zero and stop before the last row's offset. A
    /// sample matches when its distance from any grid point is strictly less
    /// than the absolute tolerance. Multiple samples can match one grid point;
    /// this does not select only a single best sample per interval. Zero
    /// tolerance returns no samples.
    ///
    /// # Panics
    /// Panics if `interval` is zero or negative.
    pub fn fetch_every(
        &self,
        interval: Duration,
        range_around_offset: Duration,
    ) -> impl Iterator<Item = &TabularGPS> + '_ {
        assert!(interval > Duration::zero(), "interval must be positive");
        let last_ref = self
            .0
            .last()
            .map(|gps| gps.offset);
        let range = range_around_offset.abs();

        self.0
            .iter()
            .filter(move |gps| {
                let mut acc_duration = Duration::zero();

                if let Some(last_gps) = last_ref {
                    while acc_duration < last_gps {
                        let gap = (gps.offset - acc_duration).abs();
                        if gap < range {
                            return true;
                        }

                        acc_duration += interval;
                    }
                }

                false
            })
    }
}

/// Select the sample closest in time from an iterator of borrowed GPS rows.
pub trait BestMatchByDistance<'a> {
    /// Return the row with the smallest absolute offset difference.
    ///
    /// Empty iterators return `None`; ties retain the first matching row.
    fn best_match_by_distance(self, offset: Duration) -> Option<&'a TabularGPS>;
}

impl<'a, I> BestMatchByDistance<'a> for I
where
    I: Iterator<Item = &'a TabularGPS>,
{
    fn best_match_by_distance(self, offset: Duration) -> Option<&'a TabularGPS> {
        self.fold(None, move |acc, singular_match| {
            acc.map_or(Some(singular_match), |best_tabular_acc| {
                let diff_to_offset_acc = (best_tabular_acc.offset - offset).abs();
                let diff = (singular_match.offset - offset).abs();
                if diff < diff_to_offset_acc {
                    Some(singular_match)
                } else {
                    acc
                }
            })
        })
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn de_ts<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let secs: f64 = Deserialize::deserialize(deserializer)?;
    if !secs.is_finite() {
        return Err(serde::de::Error::custom("timestamp must be finite"));
    }
    let mut whole = secs.floor() as i64;
    let mut nanos = ((secs - secs.floor()) * 1_000_000_000.).round() as u32;
    if nanos == 1_000_000_000 {
        whole = whole
            .checked_add(1)
            .ok_or_else(|| serde::de::Error::custom("timestamp out of range"))?;
        nanos = 0;
    }
    Utc.timestamp_opt(whole, nanos)
        .single()
        .ok_or_else(|| serde::de::Error::custom("timestamp out of range"))
}

#[cfg(test)]
mod test {
    use chrono::{Duration, TimeDelta, TimeZone, Utc};

    use crate::{
        location::tabular_gps::{BestMatchByDistance, DataPointsGPS, TabularGPS},
        tests::video_data::SAMPLE_VIDEO_SOURCES,
    };

    fn gps_with_offset(offset: Duration) -> TabularGPS {
        TabularGPS {
            timestamp: Utc
                .timestamp_opt(0, 0)
                .single()
                .unwrap(),
            latitude: 0.0,
            longitude: 0.0,
            offset,
        }
    }

    fn read_csv_file(amount: Option<usize>) -> DataPointsGPS {
        let csv_data = SAMPLE_VIDEO_SOURCES
            .first()
            .unwrap()
            .csv_embed
            .as_bytes();
        let data = DataPointsGPS::from_csv(csv_data).unwrap();

        if let Some(amount) = amount {
            DataPointsGPS(data.0[..amount].to_vec())
        } else {
            data
        }
    }

    #[test]
    fn parse_gps_file() {
        const MAX_DELTA: Duration = Duration::seconds(2);

        let deltas = read_csv_file(None)
            .0
            .iter()
            .map(|gps| &gps.offset)
            .scan(None, |prev, &x| {
                let d = prev.map(|p| x - p);
                *prev = Some(x);
                Some(d)
            })
            .flatten()
            .filter(|delta| *delta > MAX_DELTA)
            .collect::<Vec<TimeDelta>>();

        assert_eq!(deltas.len(), 0, "content: {deltas:?}");
    }

    #[test]
    fn fetch_every_second() {
        let table = DataPointsGPS(
            (0..10)
                .map(|second| gps_with_offset(Duration::seconds(second)))
                .collect(),
        );

        let fetched_gps = table
            .fetch_every(Duration::seconds(2), Duration::milliseconds(400))
            .collect::<Vec<&TabularGPS>>();

        assert_eq!(
            fetched_gps.len(),
            table
                .0
                .len()
                / 2,
            "Tabular Data: {:#?}\nFetched Data: {fetched_gps:#?}",
            table.0
        );
    }

    #[test]
    fn best_match_by_distance_returns_none_for_empty_iterator() {
        let points = Vec::<TabularGPS>::new();

        assert!(
            points
                .iter()
                .best_match_by_distance(Duration::seconds(1))
                .is_none()
        );
    }

    #[test]
    fn best_match_by_distance_picks_closest_offset() {
        let points = [
            gps_with_offset(Duration::seconds(-5)),
            gps_with_offset(Duration::seconds(2)),
            gps_with_offset(Duration::seconds(9)),
        ];

        let best = points
            .iter()
            .best_match_by_distance(Duration::seconds(3))
            .unwrap();

        assert_eq!(best.offset, Duration::seconds(2));
    }

    #[test]
    fn best_match_by_distance_keeps_first_match_on_tie() {
        let points = [
            gps_with_offset(Duration::seconds(-1)),
            gps_with_offset(Duration::seconds(1)),
        ];

        let best = points
            .iter()
            .best_match_by_distance(Duration::seconds(0))
            .unwrap();

        assert_eq!(best.offset, Duration::seconds(-1));
    }

    #[test]
    fn csv_computes_fractional_offsets_relative_to_first_row() {
        let points = DataPointsGPS::from_csv(
            b"timestamp,latitude,longitude\n1000.25,52.5,-9.25\n1001.75,53,10\n".as_slice(),
        )
        .expect("parse fractional timestamps");
        assert_eq!(points.len(), 2);
        assert_eq!(
            points[0]
                .timestamp
                .timestamp_subsec_nanos(),
            250_000_000
        );
        assert_eq!(points[0].offset, Duration::zero());
        assert_eq!(points[1].offset, Duration::milliseconds(1500));
        assert_eq!(
            points[0]
                .coordinates()
                .lat,
            52.5
        );
        assert_eq!(
            points[0]
                .coordinates()
                .lon,
            -9.25
        );
    }

    #[test]
    fn csv_empty_and_invalid_rows() {
        let empty = DataPointsGPS::from_csv(b"timestamp,latitude,longitude\n".as_slice())
            .expect("empty CSV");
        assert!(empty.is_empty());
        assert!(
            empty
                .fetch_best_match(Duration::zero(), Duration::seconds(1))
                .is_none()
        );
        assert_eq!(
            empty
                .fetch_every(Duration::seconds(1), Duration::seconds(1))
                .count(),
            0
        );
        for data in [
            "timestamp,latitude,longitude\nwrong,1,2\n",
            "timestamp,latitude,longitude\n0,wrong,2\n",
            "timestamp,latitude\n0,1\n",
        ] {
            assert!(DataPointsGPS::from_csv(data.as_bytes()).is_err(), "{data}");
        }
    }

    fn table() -> DataPointsGPS {
        DataPointsGPS(
            [0, 1, 2, 3, 4]
                .map(|s| gps_with_offset(Duration::seconds(s)))
                .to_vec(),
        )
    }

    #[test]
    fn fetch_includes_both_boundaries_and_normalizes_negative_range() {
        let points = table();
        for range in [Duration::seconds(1), Duration::seconds(-1)] {
            let offsets: Vec<_> = points
                .fetch(Duration::seconds(2), range)
                .map(|p| {
                    p.offset
                        .num_seconds()
                })
                .collect();
            assert_eq!(offsets, [1, 2, 3]);
        }
        assert_eq!(
            points
                .fetch(Duration::seconds(2), Duration::zero())
                .count(),
            1
        );
        assert!(
            points
                .fetch_best_match(Duration::seconds(10), Duration::seconds(1))
                .is_none()
        );
        assert_eq!(
            points
                .fetch_best_match(Duration::milliseconds(2600), Duration::seconds(1))
                .expect("nearest match")
                .offset,
            Duration::seconds(3)
        );
    }

    #[test]
    fn bounds_are_inclusive_and_allow_a_single_instant() {
        let points = table();
        let offsets: Vec<_> = points
            .fetch_within_bounds(Duration::seconds(1), Duration::seconds(3))
            .map(|p| {
                p.offset
                    .num_seconds()
            })
            .collect();
        assert_eq!(offsets, [1, 2, 3]);
        assert_eq!(
            points
                .fetch_within_bounds(Duration::seconds(2), Duration::seconds(2))
                .count(),
            1
        );
    }

    #[test]
    #[should_panic(expected = "assertion failed: lower_bound <= upper_bound")]
    fn inverted_bounds_are_rejected() {
        let _ = table().fetch_within_bounds(Duration::seconds(2), Duration::seconds(1));
    }

    #[test]
    fn periodic_fetch_uses_strict_tolerance_and_excludes_final_tick() {
        let points = table();
        let offsets: Vec<_> = points
            .fetch_every(Duration::seconds(2), Duration::milliseconds(100))
            .map(|p| {
                p.offset
                    .num_seconds()
            })
            .collect();
        assert_eq!(offsets, [0, 2]);
        assert_eq!(
            points
                .fetch_every(Duration::seconds(2), Duration::zero())
                .count(),
            0
        );
        assert_eq!(
            points
                .fetch_every(Duration::seconds(2), Duration::seconds(-1))
                .count(),
            2
        );
    }

    #[test]
    fn csv_handles_negative_fractional_timestamps_and_nanosecond_carry() {
        let points = DataPointsGPS::from_csv(
            b"timestamp,latitude,longitude\n-0.25,1,2\n1.9999999996,3,4\n".as_slice(),
        )
        .expect("parse fractional timestamps");
        assert_eq!(
            points[0]
                .timestamp
                .timestamp(),
            -1
        );
        assert_eq!(
            points[0]
                .timestamp
                .timestamp_subsec_nanos(),
            750_000_000
        );
        assert_eq!(
            points[1]
                .timestamp
                .timestamp(),
            2
        );
        assert_eq!(
            points[1]
                .timestamp
                .timestamp_subsec_nanos(),
            0
        );
        assert_eq!(points[1].offset, Duration::milliseconds(2250));
    }

    #[test]
    fn csv_rejects_nonfinite_and_out_of_range_timestamps_without_panicking() {
        for timestamp in ["NaN", "inf", "-inf", "1e30", "-1e30"] {
            let csv = format!("timestamp,latitude,longitude\n{timestamp},1,2\n");
            assert!(
                DataPointsGPS::from_csv(csv.as_bytes()).is_err(),
                "{timestamp}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "interval must be positive")]
    fn periodic_fetch_rejects_zero_interval_instead_of_looping_forever() {
        let _ = table().fetch_every(Duration::zero(), Duration::milliseconds(100));
    }

    #[test]
    #[should_panic(expected = "interval must be positive")]
    fn periodic_fetch_rejects_negative_interval_instead_of_looping_forever() {
        let _ = table().fetch_every(Duration::seconds(-1), Duration::milliseconds(100));
    }
}
