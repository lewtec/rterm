use crate::place::Place;

pub const TRAFFIC_WIDTH: f64 = 78.0;

/// One side of the menu-bar band beside the camera housing.
#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub min_x: f64,
    pub max_x: f64,
    pub height: f64,
}

/// Where the tab row sits while the window covers the menu bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillSplit {
    pub left_cap: f64,
    pub right_cap: f64,
    pub notch_width: f64,
    pub row_height: f64,
}

/// Split the menu-bar band around the notch. `right_reserve` is the width of
/// the buttons that stay at the right edge.
pub fn fill_split(
    window_min_x: f64,
    window_max_x: f64,
    left: Option<Band>,
    right: Option<Band>,
    right_reserve: f64,
) -> FillSplit {
    if let (Some(left), Some(right)) = (left, right)
        && left.max_x - left.min_x > 1.0
        && right.max_x - right.min_x > 1.0
    {
        return FillSplit {
            left_cap: (left.max_x - window_min_x - TRAFFIC_WIDTH).max(0.0),
            right_cap: (window_max_x - right.min_x - right_reserve).max(0.0),
            notch_width: (right.min_x - left.max_x).max(0.0),
            row_height: left.height.max(right.height).max(28.0),
        };
    }
    let width = (window_max_x - window_min_x).max(0.0);
    FillSplit {
        left_cap: (width / 2.0 - TRAFFIC_WIDTH).max(120.0),
        right_cap: (width / 2.0 - right_reserve).max(120.0),
        notch_width: 0.0,
        row_height: 32.0,
    }
}

pub fn split(
    places: &[Place],
    left_cap: f64,
    width_of: impl Fn(&Place) -> f64,
    spacing: f64,
) -> (Vec<Place>, Vec<Place>) {
    if places.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if !left_cap.is_finite() {
        return (places.to_vec(), Vec::new());
    }
    let mut leading = Vec::new();
    let mut used = 0.0;
    for (index, place) in places.iter().enumerate() {
        let width = width_of(place);
        let extra = if leading.is_empty() {
            width
        } else {
            spacing + width
        };
        if leading.is_empty() || used + extra <= left_cap {
            leading.push(place.clone());
            used += extra;
        } else {
            return (leading, places[index..].to_vec());
        }
    }
    (leading, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;

    fn place(host: &str) -> Place {
        Place::new("ada", host, Backend::Herdr, None)
    }

    #[test]
    fn infinite_cap_keeps_every_place_on_the_left() {
        let places: Vec<_> = (0..4).map(|index| place(&format!("h{index}"))).collect();
        let (leading, trailing) = split(&places, f64::INFINITY, |_| 80.0, 2.0);
        assert_eq!(
            leading
                .iter()
                .map(|place| place.host.as_str())
                .collect::<Vec<_>>(),
            ["h0", "h1", "h2", "h3"]
        );
        assert!(trailing.is_empty());
    }

    #[test]
    fn overflow_moves_to_the_right() {
        let places: Vec<_> = (0..4).map(|index| place(&format!("h{index}"))).collect();
        let (leading, trailing) = split(&places, 110.0, |_| 50.0, 2.0);
        assert_eq!(
            leading
                .iter()
                .map(|place| place.host.as_str())
                .collect::<Vec<_>>(),
            ["h0", "h1"]
        );
        assert_eq!(
            trailing
                .iter()
                .map(|place| place.host.as_str())
                .collect::<Vec<_>>(),
            ["h2", "h3"]
        );
    }

    #[test]
    fn first_place_stays_left_when_wider_than_the_cap() {
        let places = vec![place("wide"), place("next")];
        let (leading, trailing) = split(&places, 10.0, |_| 50.0, 2.0);
        assert_eq!(leading[0].host, "wide");
        assert_eq!(trailing[0].host, "next");
    }

    #[test]
    fn notch_band_leaves_a_gap_for_the_housing() {
        let split = fill_split(
            0.0,
            1400.0,
            Some(Band {
                min_x: 0.0,
                max_x: 500.0,
                height: 32.0,
            }),
            Some(Band {
                min_x: 900.0,
                max_x: 1400.0,
                height: 30.0,
            }),
            28.0,
        );
        assert_eq!(split.left_cap, 422.0);
        assert_eq!(split.right_cap, 472.0);
        assert_eq!(split.notch_width, 400.0);
        assert_eq!(split.row_height, 32.0);
    }

    #[test]
    fn missing_notch_splits_the_menu_bar_in_half() {
        let split = fill_split(0.0, 1000.0, None, None, 28.0);
        assert_eq!(split.left_cap, 500.0 - TRAFFIC_WIDTH);
        assert_eq!(split.right_cap, 500.0 - 28.0);
        assert_eq!(split.notch_width, 0.0);
        assert_eq!(split.row_height, 32.0);
    }

    #[test]
    fn empty_catalog() {
        let (leading, trailing) = split(&[], 200.0, |_| 50.0, 2.0);
        assert!(leading.is_empty());
        assert!(trailing.is_empty());
    }
}
