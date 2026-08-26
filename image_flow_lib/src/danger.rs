/// TODO
///
/// [`DangerLevel`] describes how dangerouse a given context is. This is a general description with
/// Levels similar to logging libraries. Concrete examples of Dangers are for example Danger
/// depending on the location. Danger is in general very subjective and really dependant on an
/// situation. This is in no way a perfect solution but rather a heurestic which that can help
/// assessing the real danger of the situation by the user.
///
/// Currently we have the following dangers context defined:
/// - [`LocationDangerLevel`] TODO
/// - [`ImageDetectionDangerLevel`] TODO
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, uniffi::Enum)]
pub enum DangerLevel {
    /// Debug only server for testing and development purposes
    Debug,

    /// Harmless is the less urgent level of danger. Harmless is used when no danger whats so ever
    /// can be inferred.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// These are locations where other traffic participants simply do not exists, are strongly
    /// reduced or simply is constrained to other pedestriants. In these situations we consider the
    /// location as harmless.
    Harmless,

    /// We define Moderate as situations which can potentially lead to a dangerouse situation.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// The Moderate locations are those that are usually safe but can get well travelled by
    /// pedestrians or other traffic participants such as in intersections where all type of
    /// traffic participants can meet, making it potentially dangerouse.
    ///
    /// Other Situations can include when the user approaches an escalator or in genral stairs
    /// which can lead to falling.
    Moderate,

    /// As Dangerouse we define situations where it is per default dangerouse and requires the most
    /// amount of attention.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// These are locations which should always be treated with care. If the usere is positioned on
    /// the street it should be marked dangerouse.
    Dangerouse,
}
