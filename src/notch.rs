use crate::place::Place;

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
    fn empty_catalog() {
        let (leading, trailing) = split(&[], 200.0, |_| 50.0, 2.0);
        assert!(leading.is_empty());
        assert!(trailing.is_empty());
    }
}
