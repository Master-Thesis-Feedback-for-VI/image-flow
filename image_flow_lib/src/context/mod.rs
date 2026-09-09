pub mod image_based_context;
pub mod location_based_context;

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(uniffi::Enum, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextCategory {
    Forest,

    Outside,
    Building,

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
            Self::Building => &[Self::Building],
            Self::Street => &[Self::Outside, Self::Street],
            Self::Pedestrian => &[Self::Outside, Self::Pedestrian],
            Self::Intersection => &[Self::Outside, Self::Intersection],
            Self::Elevator => &[Self::Elevator],
        }
    }
}
