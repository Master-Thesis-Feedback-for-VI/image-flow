use crate::location::coordinate_system::Coordinate;

type NameLocation = (&'static str, Coordinate);

pub const DANGEROUSE_LOCATIONS: &[NameLocation] = &[
    (
        "Karl-Wiechert-Allee",
        Coordinate {
            lat: 52.38602,
            lon: 9.80881,
        },
    ),
    (
        "Hannoversche Strasse",
        Coordinate {
            lat: 52.38704,
            lon: 9.83569,
        },
    ),
    (
        "Limmerstrasse (Am Kuechengarten)",
        Coordinate {
            lat: 52.37166,
            lon: 9.71264,
        },
    ),
    (
        "Im Maschsee",
        Coordinate {
            lat: 52.36241,
            lon: 9.73834,
        },
    ),
];

pub const MODERATE_LOCATIONS: &[NameLocation] = &[
    (
        "Berckhusenstrasse",
        Coordinate {
            lat: 52.37423,
            lon: 9.79119,
        },
    ),
    (
        "Marienstrasse Rolltreppe",
        Coordinate {
            lat: 52.37032,
            lon: 9.75328,
        },
    ),
    (
        "Steintor",
        Coordinate {
            lat: 52.37592,
            lon: 9.73218,
        },
    ),
];

pub const HARMLESS_LOCATIONS: &[NameLocation] = &[(
    "Eilenriede",
    Coordinate {
        lat: 52.38305,
        lon: 9.78945,
    },
)];

pub const INTERSECTION_LOCATIONS: &[NameLocation] = &[
    (
        "Cafe Safran",
        Coordinate {
            lat: 52.373_945,
            lon: 9.718_928,
        },
    ),
    (
        "Lister Doener",
        Coordinate {
            lat: 52.388_617,
            lon: 9.750_264,
        },
    ),
    (
        "Spannhagengarten",
        Coordinate {
            lat: 52.400_251,
            lon: 9.780_959,
        },
    ),
];
