//! Optional lossy physical stylus experiment.
//!
//! This module is never called by the default lossless codec path.

#[derive(Debug, Clone, Copy)]
pub struct StylusParameters {
    pub mass: f64,
    pub compliance: f64,
    pub damping: f64,
    pub max_displacement: f64,
    pub max_velocity: f64,
    pub tracking_error: f64,
}

impl Default for StylusParameters {
    fn default() -> Self {
        Self {
            mass: 0.02,
            compliance: 0.8,
            damping: 0.15,
            max_displacement: 1.0,
            max_velocity: 50.0,
            tracking_error: 0.0,
        }
    }
}

/// Simulate a damped mass/spring stylus following a normalized trajectory.
pub fn simulate_axis(input: &[f64], sample_rate: u32, params: StylusParameters) -> Vec<f64> {
    let dt = 1.0 / f64::from(sample_rate.max(1));
    let mass = params.mass.max(1.0e-9);
    let compliance = params.compliance.max(1.0e-9);
    let stiffness = 1.0 / compliance;
    let mut position = 0.0;
    let mut velocity = 0.0;
    let mut output = Vec::with_capacity(input.len());
    for &target in input {
        let drive = (target - position) * stiffness;
        let acceleration = (drive - params.damping * velocity) / mass;
        velocity = (velocity + acceleration * dt).clamp(-params.max_velocity, params.max_velocity);
        position =
            (position + velocity * dt).clamp(-params.max_displacement, params.max_displacement);
        let tracking_loss = params.tracking_error.clamp(0.0, 1.0);
        output.push(position * (1.0 - tracking_loss));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_is_bounded_and_tracking_error_degrades_output() {
        let input = vec![1.0; 1_000];
        let normal = simulate_axis(&input, 48_000, StylusParameters::default());
        assert!(normal.iter().all(|value| value.abs() <= 1.0));
        let muted = simulate_axis(
            &input,
            48_000,
            StylusParameters {
                tracking_error: 1.0,
                ..StylusParameters::default()
            },
        );
        assert!(muted.iter().all(|value| *value == 0.0));
    }
}
