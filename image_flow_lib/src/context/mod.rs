use std::{
    collections::{HashSet, hash_set},
    mem,
    sync::LazyLock,
};

use serde::{Deserialize, Serialize};

use crate::context::image_based_context::ImageDetections;

pub mod image_based_context;
pub mod location_based_context;

static INSIDE_CATEGORIES: LazyLock<HashSet<ContextCategory>> =
    LazyLock::new(|| HashSet::from([ContextCategory::Inside, ContextCategory::Elevator]));

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(
    uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum ContextCategory {
    Forest,

    Outside,
    Inside,

    Street,
    Pedestrian,
    Intersection,

    Elevator,
}

impl ContextCategory {
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UniqueGeographicCategories {
    pub detections: Option<ImageDetections>,
    categories: HashSet<ContextCategory>,
    is_inside: bool,
}

impl UniqueGeographicCategories {
    #[must_use]
    pub fn new() -> Self {
        Self {
            categories: HashSet::new(),
            detections: None,
            is_inside: false,
        }
    }

    #[must_use]
    pub fn iter(&self) -> hash_set::Iter<'_, ContextCategory> {
        self.categories
            .iter()
    }

    pub fn insert(&mut self, context_category: ContextCategory) {
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

    fn contains(&self, context_category: ContextCategory) -> bool {
        self.categories
            .contains(&context_category)
    }

    #[must_use]
    pub fn categories(&mut self) -> HashSet<ContextCategory> {
        mem::take(&mut self.categories)
    }

    pub(crate) fn merge(mut self, mut other: Self) -> Self {
        let is_inside = self.is_inside || other.is_inside;

        let other_cats = other.categories();
        self.extend(other_cats);
        let categories = self.categories();

        let mut detections = self
            .detections
            .unwrap_or_default();
        detections.extend(
            other
                .detections
                .unwrap_or_default(),
        );
        let detections = (!detections.is_empty()).then_some(detections);

        Self {
            detections,
            categories,
            is_inside,
        }
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

impl Extend<ContextCategory> for UniqueGeographicCategories {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = ContextCategory>,
    {
        let items: Vec<ContextCategory> = iter
            .into_iter()
            .collect();

        let just_became_inside = !self.is_inside && items.contains(&ContextCategory::Inside);

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

impl FromIterator<ContextCategory> for UniqueGeographicCategories {
    fn from_iter<I: IntoIterator<Item = ContextCategory>>(iter: I) -> Self {
        let mut categories = Self::new();
        categories.extend(iter);
        categories
    }
}

impl IntoIterator for UniqueGeographicCategories {
    type Item = ContextCategory;
    type IntoIter = hash_set::IntoIter<ContextCategory>;

    fn into_iter(self) -> Self::IntoIter {
        self.categories
            .into_iter()
    }
}

impl<'a> IntoIterator for &'a UniqueGeographicCategories {
    type Item = &'a ContextCategory;
    type IntoIter = hash_set::Iter<'a, ContextCategory>;

    fn into_iter(self) -> Self::IntoIter {
        self.categories
            .iter()
    }
}

#[cfg(test)]
mod test {
    use rand::prelude::*;
    use std::collections::HashSet;

    use crate::context::{ContextCategory, INSIDE_CATEGORIES, UniqueGeographicCategories};

    const ALL_CATS: &[ContextCategory] = &[
        ContextCategory::Forest,
        ContextCategory::Outside,
        ContextCategory::Inside,
        ContextCategory::Street,
        ContextCategory::Pedestrian,
        ContextCategory::Intersection,
        ContextCategory::Elevator,
    ];

    fn randomize_context_categories() -> HashSet<ContextCategory> {
        let mut rng = rand::rng();
        let amount = rand::random_range(0..ALL_CATS.len());
        ALL_CATS
            .sample(&mut rng, amount)
            .copied()
            .collect()
    }

    fn valid_unique_cat(set: &UniqueGeographicCategories) -> bool {
        let both = set.contains(ContextCategory::Inside) && set.contains(ContextCategory::Outside);
        if both {
            return false;
        }

        if set.contains(ContextCategory::Inside) {
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
        let all: HashSet<ContextCategory> = ALL_CATS
            .iter()
            .copied()
            .collect();

        let mut unique = UniqueGeographicCategories::new();
        for cat in all.clone() {
            unique.insert(cat);
        }
        assert!(valid_unique_cat(&unique));

        let mut unique = UniqueGeographicCategories::new();
        unique.extend(all);
        assert!(valid_unique_cat(&unique));

        let inside: UniqueGeographicCategories = INSIDE_CATEGORIES
            .iter()
            .copied()
            .collect();
        let outside: UniqueGeographicCategories =
            vec![ContextCategory::Forest, ContextCategory::Street]
                .into_iter()
                .collect();

        let merged1 = UniqueGeographicCategories::merge(inside.clone(), outside.clone());
        assert!(valid_unique_cat(&merged1), "merged data: {merged1:#?}");

        let merged2 = UniqueGeographicCategories::merge(outside, inside);
        assert!(valid_unique_cat(&merged2), "merged data: {merged2:#?}");
    }

    #[test]
    fn randomized_picks() {
        let amount = 10usize;

        for _ in 0..amount {
            let sample = randomize_context_categories()
                .into_iter()
                .collect::<UniqueGeographicCategories>();
            assert!(
                valid_unique_cat(&sample),
                "Non valid here is data: {sample:#?}"
            );
        }

        for _ in 0..amount {
            let sample1: UniqueGeographicCategories = randomize_context_categories()
                .into_iter()
                .collect();
            let sample2: UniqueGeographicCategories = randomize_context_categories()
                .into_iter()
                .collect();
            let sample3 = sample1.merge(sample2);
            assert!(
                valid_unique_cat(&sample3),
                "Non valid here is data: {sample3:#?}"
            );
        }
    }
}
