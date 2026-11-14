//! Geographic/context labels and their insertion rules.
//!
//! Inserting `Forest` or `Intersection` implies `Outside`; inserting `Parking`
//! implies `Street`. `Outside` takes precedence over `Inside`. These rules apply
//! to insertion/collection, not to direct Serde deserialization or removal.

use std::{
    collections::{HashSet, hash_set},
    fmt::Display,
    mem,
};

use serde::{Deserialize, Serialize};

/// Supported geographic/context labels, ordered by declaration for sorting.
#[allow(dead_code)]
#[derive(
    uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum LocationTag {
    /// Railway context.
    Railway,
    /// Parking context; implies [`Self::Street`].
    Parking,
    /// Street context; does not itself imply [`Self::Outside`].
    Street,
    /// Pedestrian route or area.
    Pedestrian,
    /// Intersection context; implies [`Self::Outside`].
    Intersection,
    /// Forest or foliage-like image context; implies [`Self::Outside`].
    Forest,
    /// Outdoor evidence; removes [`Self::Inside`] on insertion.
    Outside,
    /// Indoor evidence, retained only while [`Self::Outside`] is absent.
    Inside,
    /// Elevator context; does not itself imply [`Self::Inside`].
    Elevator,
}

/// Return a copy sorted in enum declaration order, preserving duplicate entries.
#[uniffi::export]
#[must_use]
pub fn sort_location_tags(categories: &[LocationTag]) -> Vec<LocationTag> {
    let mut copied_categories = categories.to_vec();
    copied_categories.sort_unstable();
    copied_categories
}

impl LocationTag {
    /// Return the tags inserted for this label, including the label itself.
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

/// Unordered set of context tags with implication and indoor/outdoor insertion rules.
///
/// Insertion and `Extend` expand [`LocationTag::implied_additional_categories`]
/// and remove `Inside` whenever `Outside` is present. Iteration and serialized
/// array order are unspecified. Serde stores `{"categories": [...]}` and does
/// not normalize tags on deserialization. Removal does not undo implications.
///
/// ```
/// use image_flow::location::tag::{LocationTag, UniqueLocationTags};
///
/// let mut tags = UniqueLocationTags::new_singular(LocationTag::Parking);
/// assert!(tags.contains(LocationTag::Street));
/// tags.remove(&LocationTag::Parking);
/// assert!(tags.contains(LocationTag::Street));
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize, uniffi::Record)]
pub struct UniqueLocationTags {
    categories: HashSet<LocationTag>,
}

impl UniqueLocationTags {
    /// Create an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            categories: HashSet::new(),
        }
    }

    /// Create a set with one label and any tags implied by it.
    #[must_use]
    pub fn new_singular(category: LocationTag) -> Self {
        let mut uniq = Self::new();
        uniq.insert(category);
        uniq
    }

    /// Borrow tags in unspecified order without removing them.
    #[must_use]
    pub fn iter(&self) -> hash_set::Iter<'_, LocationTag> {
        self.categories
            .iter()
    }

    /// Insert a label and its implications, resolving `Inside`/`Outside` conflicts.
    pub fn insert(&mut self, context_category: LocationTag) {
        self.extend([context_category]);
    }

    /// Merge weaker contextual hints without letting nearby outdoor features or
    /// image colors override a known building-containment result.
    pub(crate) fn extend_context_tags(&mut self, hints: Self) {
        let inside = self.contains(LocationTag::Inside);
        self.extend(
            hints
                .into_iter()
                .filter(|tag| {
                    !inside
                        || !tag
                            .implied_additional_categories()
                            .contains(&LocationTag::Outside)
                }),
        );
    }

    /// Test whether a label is present.
    #[must_use]
    pub fn contains(&self, context_category: LocationTag) -> bool {
        self.categories
            .contains(&context_category)
    }

    /// Test whether no labels are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.categories
            .is_empty()
    }

    /// Remove and return all labels, leaving this set empty.
    ///
    /// Use [`Self::iter`] to inspect tags without draining the set.
    #[must_use]
    pub fn categories(&mut self) -> HashSet<LocationTag> {
        mem::take(&mut self.categories)
    }

    /// Remove only this label; previously implied labels remain present.
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
        LocationTag::Parking,
        LocationTag::Railway,
    ];

    fn valid_unique_cat(set: &UniqueLocationTags) -> bool {
        let either = set.contains(LocationTag::Inside) ^ set.contains(LocationTag::Outside);
        let neither = !set.contains(LocationTag::Inside) && !set.contains(LocationTag::Outside);

        either || neither || set.is_empty()
    }

    #[test]
    fn classification_outdoor_hints_do_not_override_inside() {
        for hint in [
            LocationTag::Forest,
            LocationTag::Intersection,
            LocationTag::Outside,
        ] {
            let mut tags = UniqueLocationTags::new_singular(LocationTag::Inside);
            tags.extend_context_tags(UniqueLocationTags::new_singular(hint));
            assert!(tags.contains(LocationTag::Inside));
            assert!(!tags.contains(LocationTag::Outside));
            assert!(!tags.contains(hint));
        }
    }

    #[test]
    fn classification_context_preserves_nonconflicting_tags() {
        let mut tags = UniqueLocationTags::new_singular(LocationTag::Inside);
        tags.extend_context_tags(UniqueLocationTags::new_singular(LocationTag::Railway));
        assert!(tags.contains(LocationTag::Inside));
        assert!(tags.contains(LocationTag::Railway));

        let mut tags = UniqueLocationTags::new_singular(LocationTag::Outside);
        tags.extend_context_tags(UniqueLocationTags::new_singular(LocationTag::Forest));
        assert!(tags.contains(LocationTag::Outside));
        assert!(tags.contains(LocationTag::Forest));
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
    fn every_subset_obeys_implications_and_outside_precedence_in_both_orders() {
        for mask in 0..(1 << ALL_CATS.len()) {
            let chosen: Vec<_> = ALL_CATS
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, tag)| *tag)
                .collect();
            let forward: UniqueLocationTags = chosen
                .iter()
                .copied()
                .collect();
            let mut backward = UniqueLocationTags::new();
            for tag in chosen
                .iter()
                .rev()
            {
                backward.insert(*tag);
                backward.insert(*tag);
            }
            let mut expected: HashSet<_> = chosen
                .iter()
                .flat_map(|tag| {
                    tag.implied_additional_categories()
                        .iter()
                        .copied()
                })
                .collect();
            if expected.contains(&LocationTag::Outside) {
                expected.remove(&LocationTag::Inside);
            }
            assert!(valid_unique_cat(&forward));
            assert_eq!(
                forward
                    .into_iter()
                    .collect::<HashSet<_>>(),
                expected
            );
            assert_eq!(backward.categories(), expected);
        }
    }

    #[test]
    fn categories_drains_and_remove_does_not_remove_implied_tags() {
        let mut tags = UniqueLocationTags::new_singular(LocationTag::Parking);
        tags.remove(&LocationTag::Parking);
        tags.remove(&LocationTag::Forest);
        assert_eq!(tags.categories(), HashSet::from([LocationTag::Street]));
        assert!(tags.is_empty());
        assert!(
            tags.categories()
                .is_empty()
        );
    }

    #[test]
    fn sorting_preserves_duplicates_and_does_not_mutate_input() {
        let input = [
            LocationTag::Inside,
            LocationTag::Railway,
            LocationTag::Inside,
        ];
        assert_eq!(
            super::sort_location_tags(&input),
            [
                LocationTag::Railway,
                LocationTag::Inside,
                LocationTag::Inside
            ]
        );
        assert_eq!(input[0], LocationTag::Inside);
        assert_eq!(super::sort_location_tags(&[]), []);
    }

    #[test]
    fn serialization_preserves_unique_tags() {
        let tags = UniqueLocationTags::new_singular(LocationTag::Forest);
        let json = serde_json::to_string(&tags).expect("serialize tags");
        let restored: UniqueLocationTags = serde_json::from_str(&json).expect("deserialize tags");
        assert_eq!(
            tags.into_iter()
                .collect::<HashSet<_>>(),
            restored
                .into_iter()
                .collect()
        );
    }
}
