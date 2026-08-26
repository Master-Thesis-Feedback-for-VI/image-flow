use std::io;

use chrono::{DateTime, Duration, TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct TabularGPS {
    #[serde(deserialize_with = "de_ts")]
    timestamp: DateTime<Utc>,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    offset: TimeDelta,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DataPointsGPS(Vec<TabularGPS>);

impl DataPointsGPS {
    /// # Errors
    /// TODO
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

    pub fn fetch_every(
        &self,
        interval: Duration,
        range_around_offset: Duration,
    ) -> impl Iterator<Item = &TabularGPS> + '_ {
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

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn de_ts<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let secs: f64 = Deserialize::deserialize(deserializer)?;
    let whole = secs.trunc() as i64;
    let nanos = ((secs - secs.trunc()) * 1_000_000_000.).round() as u32;
    Ok(Utc
        .timestamp_opt(whole, nanos)
        .single()
        .expect("Could not convert time"))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeDelta};

    use crate::{
        location::tabular_gps::{DataPointsGPS, TabularGPS},
        tests::data::GPS_FILE,
    };

    fn read_csv_file(amount: Option<usize>) -> DataPointsGPS {
        let csv_data = GPS_FILE.as_bytes();

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
        let table = read_csv_file(Some(10));

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
}
