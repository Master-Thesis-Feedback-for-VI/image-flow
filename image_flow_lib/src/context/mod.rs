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
