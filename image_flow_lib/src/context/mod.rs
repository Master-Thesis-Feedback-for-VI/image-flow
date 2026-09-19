use std::{
    collections::{HashSet, hash_set},
    mem,
    sync::LazyLock,
};

use serde::{Deserialize, Serialize};

pub mod image_based_context;
pub mod location_based_context;

static INSIDE_CATEGORIES: LazyLock<HashSet<LocationTag>> =
    LazyLock::new(|| HashSet::from([LocationTag::Inside, LocationTag::Elevator]));

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(
    uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum LocationTag {
    Forest,

    Outside,
    Inside,

    Street,
    Pedestrian,
    Intersection,

    Elevator,
}

impl LocationTag {
    #[must_use]
    pub const fn implied_additional_categories(self) -> &'static [Self] {
        match self {
            Self::Forest => &[Self::Outside, Self::Forest],
            Self::Outside => &[Self::Outside],
            Self::Inside => &[Self::Inside],
            Self::Street => &[Self::Outside, Self::Street],
            Self::Pedestrian => &[Self::Outside, Self::Pedestrian],
            Self::Intersection => &[Self::Outside, Self::Intersection],
            Self::Elevator => &[Self::Elevator],
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, uniffi::Record)]
pub struct UniqueLocationTags {
    categories: HashSet<LocationTag>,
    is_inside: bool,
}

impl UniqueLocationTags {
    #[must_use]
    pub fn new() -> Self {
        Self {
            categories: HashSet::new(),
            is_inside: false,
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
        let is_inside_category = INSIDE_CATEGORIES.contains(&context_category);

        if self.is_inside && !is_inside_category {
            return;
        }

        self.categories
            .insert(context_category);

        if is_inside_category && !self.is_inside {
            self.is_inside = true;
            self.remove_outside_categories();
        }
    }

    fn remove_outside_categories(&mut self) {
        let cat = self
            .categories
            .iter()
            .filter(move |pred| INSIDE_CATEGORIES.contains(pred))
            .copied()
            .collect();

        self.categories = cat;
    }

    #[cfg(test)]
    fn contains(&self, context_category: LocationTag) -> bool {
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

    pub(crate) fn extend_by_implied(self) -> Self {
        let categories = self
            .categories
            .into_iter()
            .flat_map(|category| {
                category
                    .implied_additional_categories()
                    .to_vec()
            })
            .collect();

        Self { categories, ..self }
    }
}

impl Extend<LocationTag> for UniqueLocationTags {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = LocationTag>,
    {
        let items: Vec<LocationTag> = iter
            .into_iter()
            .collect();

        let just_became_inside = !self.is_inside && items.contains(&LocationTag::Inside);

        if self.is_inside || just_became_inside {
            self.categories
                .extend(
                    items
                        .into_iter()
                        .filter(|c| INSIDE_CATEGORIES.contains(c)),
                );
        } else {
            self.categories
                .extend(items);
        }

        if just_became_inside {
            self.is_inside = true;
            self.remove_outside_categories();
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

    use crate::context::{INSIDE_CATEGORIES, LocationTag, UniqueLocationTags};

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
        let both = set.contains(LocationTag::Inside) && set.contains(LocationTag::Outside);
        if both {
            return false;
        }

        if set.contains(LocationTag::Inside) {
            if !set.is_inside {
                return false;
            }

            for cat in &set.categories {
                if !INSIDE_CATEGORIES.contains(cat) {
                    return false;
                }
            }
        }

        true
    }

    #[test]
    fn cannot_conatain_both() {
        let all: HashSet<LocationTag> = ALL_CATS
            .iter()
            .copied()
            .collect();

        let mut unique = UniqueLocationTags::new();
        for cat in all.clone() {
            unique.insert(cat);
        }
        assert!(valid_unique_cat(&unique));

        let mut unique = UniqueLocationTags::new();
        unique.extend(all);
        assert!(valid_unique_cat(&unique));

        let inside: UniqueLocationTags = INSIDE_CATEGORIES
            .iter()
            .copied()
            .collect();
        let outside: UniqueLocationTags = vec![LocationTag::Forest, LocationTag::Street]
            .into_iter()
            .collect();

        {
            let mut inside = inside.clone();
            let outside = outside.clone();

            inside.extend(outside);
            assert!(valid_unique_cat(&inside), "merged data: {inside:#?}");
        }

        {
            let mut outside = outside;

            outside.extend(inside);
            assert!(valid_unique_cat(&outside), "merged data: {outside:#?}");
        }
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
