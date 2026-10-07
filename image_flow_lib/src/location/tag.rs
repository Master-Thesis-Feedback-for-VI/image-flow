use std::{
    collections::{HashSet, hash_set},
    fmt::Display,
    mem,
};

use serde::{Deserialize, Serialize};

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(
    uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum LocationTag {
    Railway,
    Street,
    Parking,
    Intersection,
    Pedestrian,
    Forest,
    Outside,
    Inside,
    Elevator,
}

#[uniffi::export]
#[must_use]
pub fn sort_location_tags(categories: &[LocationTag]) -> Vec<LocationTag> {
    let mut copied_categories = categories.to_vec();
    copied_categories.sort_unstable();
    copied_categories
}

impl LocationTag {
    #[must_use]
    pub const fn implied_additional_categories(self) -> &'static [Self] {
        match self {
            Self::Forest => &[Self::Outside, Self::Forest],
            Self::Outside => &[Self::Outside],
            Self::Inside => &[Self::Inside],
            Self::Street => &[Self::Street],
            Self::Pedestrian => &[Self::Pedestrian],
            Self::Intersection => &[Self::Outside, Self::Intersection],
            Self::Elevator => &[Self::Elevator],
            Self::Parking => &[Self::Parking, Self::Street],
            Self::Railway => &[Self::Railway],
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, uniffi::Record)]
pub struct UniqueLocationTags {
    categories: HashSet<LocationTag>,
}

impl UniqueLocationTags {
    #[must_use]
    pub fn new() -> Self {
        Self {
            categories: HashSet::new(),
        }
    }

    #[must_use]
    pub fn new_singular(category: LocationTag) -> Self {
        let mut uniq = Self::new();
        uniq.insert(category);
        uniq
    }

    #[must_use]
    pub fn iter(&self) -> hash_set::Iter<'_, LocationTag> {
        self.categories
            .iter()
    }

    pub fn insert(&mut self, context_category: LocationTag) {
        self.extend([context_category]);
    }

    #[must_use]
    pub fn contains(&self, context_category: LocationTag) -> bool {
        self.categories
            .contains(&context_category)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.categories
            .is_empty()
    }

    #[must_use]
    pub fn categories(&mut self) -> HashSet<LocationTag> {
        mem::take(&mut self.categories)
    }

    pub fn remove(&mut self, tag: &LocationTag) {
        self.categories
            .remove(tag);
    }
}

impl Display for UniqueLocationTags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let categories = self
            .categories
            .clone()
            .into_iter()
            .collect::<Vec<LocationTag>>();

        write!(f, "UniqueLocationTags:\n\t{categories:#?}")
    }
}

impl Extend<LocationTag> for UniqueLocationTags {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = LocationTag>,
    {
        let iter = iter
            .into_iter()
            .flat_map(|category| {
                category
                    .implied_additional_categories()
                    .to_vec()
            });
        self.categories
            .extend(iter);
        if self
            .categories
            .contains(&LocationTag::Outside)
        {
            self.categories
                .remove(&LocationTag::Inside);
        }
    }
}

impl FromIterator<LocationTag> for UniqueLocationTags {
    fn from_iter<I: IntoIterator<Item = LocationTag>>(iter: I) -> Self {
        let mut categories = Self::new();
        categories.extend(iter);
        categories
    }
}

impl IntoIterator for UniqueLocationTags {
    type Item = LocationTag;
    type IntoIter = hash_set::IntoIter<LocationTag>;

    fn into_iter(self) -> Self::IntoIter {
        self.categories
            .into_iter()
    }
}

impl<'a> IntoIterator for &'a UniqueLocationTags {
    type Item = &'a LocationTag;
    type IntoIter = hash_set::Iter<'a, LocationTag>;

    fn into_iter(self) -> Self::IntoIter {
        self.categories
            .iter()
    }
}

#[cfg(test)]
mod test {
    use rand::prelude::*;
    use std::collections::HashSet;

    use crate::location::tag::{LocationTag, UniqueLocationTags};

    const ALL_CATS: &[LocationTag] = &[
        LocationTag::Forest,
        LocationTag::Outside,
        LocationTag::Inside,
        LocationTag::Street,
        LocationTag::Pedestrian,
        LocationTag::Intersection,
        LocationTag::Elevator,
    ];

    fn randomize_context_categories() -> HashSet<LocationTag> {
        let mut rng = rand::rng();
        let amount = rand::random_range(0..ALL_CATS.len());
        ALL_CATS
            .sample(&mut rng, amount)
            .copied()
            .collect()
    }

    fn valid_unique_cat(set: &UniqueLocationTags) -> bool {
        let either = set.contains(LocationTag::Inside) ^ set.contains(LocationTag::Outside);
        let neither = !set.contains(LocationTag::Inside) && !set.contains(LocationTag::Outside);

        either || neither || set.is_empty()
    }

    #[test]
    fn cannot_conatain_both() {
        let mut tags = UniqueLocationTags::new();
        tags.extend([LocationTag::Outside, LocationTag::Inside]);
        assert!(valid_unique_cat(&tags), "Invalid: {tags}");

        tags = UniqueLocationTags::new();
        tags.extend([LocationTag::Outside, LocationTag::Intersection]);
        assert!(valid_unique_cat(&tags), "Invalid: {tags}");

        tags = UniqueLocationTags::new();
        tags.extend([LocationTag::Inside, LocationTag::Intersection]);
        assert!(valid_unique_cat(&tags), "Invalid: {tags}");
    }

    #[test]
    fn randomized_picks() {
        let amount = 10usize;

        for _ in 0..amount {
            let sample = randomize_context_categories()
                .into_iter()
                .collect::<UniqueLocationTags>();
            assert!(
                valid_unique_cat(&sample),
                "Non valid here is data: {sample:#?}"
            );
        }

        for _ in 0..amount {
            let sample1: UniqueLocationTags = randomize_context_categories()
                .into_iter()
                .collect();
            let sample2: UniqueLocationTags = randomize_context_categories()
                .into_iter()
                .collect();

            let mut sample3 = UniqueLocationTags::new();
            sample3.extend(sample1);
            sample3.extend(sample2);

            assert!(
                valid_unique_cat(&sample3),
                "Non valid here is data: {sample3:#?}"
            );
        }
    }
}
