//! Visual solar lighting, shared by the navigator and the static-site preview.
//! NOAA/Meeus solar coordinates; geometric elevation (no weather/refraction).
use glam::DVec3;

/// Lighting coverage is independent of the free camera's one-unit aiming ray.
/// Retain the orbit footprint on entry, then follow actual camera translation.
#[derive(Clone, Copy, Debug)]
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct ShadowView {
    pub target: DVec3,
    pub distance: f64,
    pub reach: f64,
}

#[derive(Default)]
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct ShadowFocus(Option<(DVec3, ShadowView)>);

#[cfg(any(target_arch = "wasm32", test))]
impl ShadowFocus {
    pub fn update(&mut self, orbit: bool, eye: DVec3, target: DVec3, distance: f64) -> ShadowView {
        let view = match self.0 {
            Some((previous_eye, view)) if !orbit => ShadowView {
                target: view.target + (eye - previous_eye),
                ..view
            },
            _ => ShadowView {
                target,
                distance,
                reach: ((target - eye) * crate::scene::DISPLAY_SCALE).length(),
            },
        };
        self.0 = Some((eye, view));
        view
    }

    /// A mode transition can lower the eye to a walking surface. Keep the
    /// existing world-space lighting footprint instead of moving it underground.
    pub fn reanchor(&mut self, eye: DVec3) {
        if let Some((previous_eye, _)) = &mut self.0 {
            *previous_eye = eye;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SolarPosition {
    /// Unit direction toward the sun, in the renderer's east/up/south axes.
    pub direction: DVec3,
    pub elevation: f64,
    /// Clockwise radians from geographic north.
    pub azimuth: f64,
}

impl SolarPosition {
    pub fn daylight(self) -> bool {
        self.elevation > 0.0
    }

    pub fn sky_amount(self) -> f32 {
        ((self.elevation.to_degrees() + 8.0) / 18.0).clamp(0.0, 1.0) as f32
    }
}

/// Solar position from latitude/longitude in degrees and Unix UTC seconds.
/// Based on NOAA's Julian-century equation-of-time and apparent declination
/// formulas: https://gml.noaa.gov/grad/solcalc/calcdetails.html.
pub fn solar_position(latitude: f64, longitude: f64, unix_seconds: f64) -> SolarPosition {
    let t = (unix_seconds / 86400.0 + 2440587.5 - 2451545.0) / 36525.0;
    let longitude_mean = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let anomaly = (357.52911 + t * (35999.05029 - 0.0001537 * t)).to_radians();
    let eccentricity = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let center = anomaly.sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + (2.0 * anomaly).sin() * (0.019993 - 0.000101 * t)
        + (3.0 * anomaly).sin() * 0.000289;
    let omega = (125.04 - 1934.136 * t).to_radians();
    let apparent = (longitude_mean + center - 0.00569 - 0.00478 * omega.sin()).to_radians();
    let seconds = 21.448 - t * (46.815 + t * (0.00059 - t * 0.001813));
    let obliquity = (23.0 + (26.0 + seconds / 60.0) / 60.0 + 0.00256 * omega.cos()).to_radians();
    let declination = (obliquity.sin() * apparent.sin()).asin();
    let y = (obliquity / 2.0).tan().powi(2);
    let mean = longitude_mean.to_radians();
    let equation_minutes = 4.0
        * (y * (2.0 * mean).sin() - 2.0 * eccentricity * anomaly.sin()
            + 4.0 * eccentricity * y * anomaly.sin() * (2.0 * mean).cos()
            - 0.5 * y * y * (4.0 * mean).sin()
            - 1.25 * eccentricity * eccentricity * (2.0 * anomaly).sin())
        .to_degrees();
    let solar_minutes =
        (unix_seconds.rem_euclid(86400.0) / 60.0 + equation_minutes + 4.0 * longitude)
            .rem_euclid(1440.0);
    let hour = (solar_minutes / 4.0 - 180.0).to_radians();
    let latitude = latitude.clamp(-90.0, 90.0).to_radians();
    let east = -declination.cos() * hour.sin();
    let north =
        latitude.cos() * declination.sin() - latitude.sin() * declination.cos() * hour.cos();
    let up = latitude.sin() * declination.sin() + latitude.cos() * declination.cos() * hour.cos();
    SolarPosition {
        direction: DVec3::new(east, up, -north).normalize(),
        elevation: up.clamp(-1.0, 1.0).asin(),
        azimuth: east.atan2(north).rem_euclid(std::f64::consts::TAU),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_camera_aim_and_turning_do_not_shrink_or_recenter_orbit_shadows() {
        let mut focus = ShadowFocus::default();
        let eye = DVec3::new(100., 80., 120.);
        let target = DVec3::new(10., 30., 20.);
        focus.update(true, eye, target, 160.);
        let reach = ((target - eye) * crate::scene::DISPLAY_SCALE).length();
        for aim in [DVec3::X, DVec3::NEG_Z, DVec3::NEG_Y] {
            let view = focus.update(false, eye, eye + aim, 1.);
            assert_eq!(view.target, target);
            assert_eq!(view.distance, 160.);
            assert_eq!(view.reach, reach);
        }
    }

    #[test]
    fn walking_entry_keeps_the_world_footprint_and_movement_then_follows() {
        let mut focus = ShadowFocus::default();
        let target = DVec3::new(19968., 50., 64.);
        let original = focus.update(true, DVec3::new(20080., 180., 140.), target, 200.);
        let eye = DVec3::new(20080., 3.5, 140.);
        focus.reanchor(eye);
        assert_eq!(focus.update(false, eye, eye + DVec3::X, 1.).target, target);
        let movement = DVec3::new(4., 1., -8.);
        let view = focus.update(false, eye + movement, eye + movement + DVec3::X, 1.);
        assert_eq!(view.target, target + movement);
        assert_eq!(view.distance, 200.);
        assert_eq!(view.reach, original.reach);
        let orbit = focus.update(true, eye + movement, target, 40.);
        assert_eq!(orbit.target, target);
        assert_eq!(orbit.distance, 40.);
    }

    #[test]
    fn matches_the_nrel_spa_reference_observation() {
        // NREL SPA reference: Colorado, 2003-10-17 19:30:30 UTC.
        // Its apparent elevation includes refraction; our geometric result
        // differs by a few hundredths of a degree at this elevation.
        let sun = solar_position(39.742476, -105.1786, 1066419030.0);
        assert!((sun.azimuth.to_degrees() - 194.340241).abs() < 0.1);
        assert!((sun.elevation.to_degrees() - 39.888378).abs() < 0.1);
        assert!(sun.direction.x < 0.0 && sun.direction.z > 0.0);
    }

    #[test]
    fn equinox_noon_and_midnight_have_opposite_lighting() {
        // 2026-03-20 12:00:00 UTC at Greenwich's equator.
        let noon = solar_position(0.0, 0.0, 1774008000.0);
        let midnight = solar_position(0.0, 0.0, 1773964800.0);
        assert!(noon.elevation.to_degrees() > 87.0);
        assert!(noon.daylight());
        assert!(midnight.elevation.to_degrees() < -87.0);
        assert!(!midnight.daylight());
    }

    #[test]
    fn sunlight_respects_east_west_and_longitude() {
        let morning = solar_position(0.0, 0.0, 1773990000.0);
        let afternoon = solar_position(0.0, 0.0, 1774026000.0);
        assert!(morning.direction.x > 0.8);
        assert!(afternoon.direction.x < -0.8);
        let east_midnight = solar_position(0.0, 180.0, 1773964800.0);
        assert!(east_midnight.daylight());
        assert!((east_midnight.direction.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn solstice_polar_day_and_night_are_finite() {
        // 2026-06-21 12:00:00 UTC.
        let north = solar_position(89.9, 0.0, 1782043200.0);
        let south = solar_position(-89.9, 0.0, 1782043200.0);
        assert!(north.daylight());
        assert!(!south.daylight());
        assert!(north.direction.is_finite() && south.direction.is_finite());
        assert!((north.elevation.to_degrees() - 23.4).abs() < 0.2);
    }
}
