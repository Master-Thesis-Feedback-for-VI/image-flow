use std::{
    collections::{HashSet, hash_set},
    sync::LazyLock,
};

use crate::context::image_based_context::ImageDetections;

pub mod image_based_context;
pub mod location_based_context;

static INSIDE_CATEGORIES: LazyLock<HashSet<ContextCategory>> =
    LazyLock::new(|| HashSet::from([ContextCategory::Inside, ContextCategory::Elevator]));

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Clone, Debug, Default)]
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

    pub fn extend<I>(&mut self, iter: I)
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
    pub fn categories(self) -> HashSet<ContextCategory> {
        self.categories
    }

    pub(crate) fn merge(uniq1: Self, uniq2: Self) -> Self {
        let is_inside = uniq1.is_inside || uniq2.is_inside;

        let mut categories = uniq1.categories;
        categories.extend(uniq2.categories);

        let mut detections = uniq1
            .detections
            .unwrap_or_default();
        detections.extend(
            uniq2
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

impl FromIterator<ContextCategory> for UniqueGeographicCategories {
    fn from_iter<I: IntoIterator<Item = ContextCategory>>(iter: I) -> Self {
        let collection: HashSet<ContextCategory> = iter
            .into_iter()
            .collect();

        let is_inside = collection.contains(&ContextCategory::Inside);
        Self {
            categories: collection,
            is_inside,
            detections: None,
        }
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
