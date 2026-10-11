//! Vehicle-model selection and dispatch (enum dispatch, no `dyn`, WASM-friendly).
//!
//! `Provisional` is the existing project-owned model. `Arcade` is the generic
//! gearbox model, `Classic` the tuned arcade-racer feel model. `Custom` steps a provider-supplied model installed from a
//! `model_ext::VehicleModelRegistry` (this is how private compatibility models
//! plug in).

use crate::arcade_model::{self, ArcadeConfig};
use crate::classic_model::{self, ClassicConfig};
use crate::drivetrain::DriveState;
use crate::provisional_model::{self, KinematicUpdate, ProvisionalPolicy};
use crate::scripted_model::{self, ScriptedControl};
use crate::{DriverInput, Vehicle};

/// Which vehicle model a `RaceWorld` steps with. Defaults to `Provisional`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VehicleModel {
    #[default]
    Provisional,
    /// Servo-driven motion for computer-controlled cars: the driver module
    /// supplies accelerations (`RaceWorld::set_scripted_control`); there is no
    /// engine, gearbox or tyre model. See `scripted_model`.
    Scripted,
    /// Generic project-owned arcade model (torque-curve engine, automatic
    /// gearbox, grip-limited tyres, speed-sensitive steering with a visible
    /// body-yaw lead). The default of the public engine; needs parameters
    /// (`RaceWorld::install_arcade_parameters`). See `arcade_model`.
    Arcade,
    /// Tuned classic arcade-racer handling: response curves for launch, a
    /// speed-driven gearbox with a rev hold, a very strong brake, a steering
    /// law with a knee, reverse at any speed and a handbrake slide. The default
    /// of project cars; needs parameters (`RaceWorld::install_classic_parameters`).
    /// See `classic_model`.
    Classic,
    /// Model supplied by an external provider (see `model_ext`); needs
    /// `RaceWorld::install_custom_model`.
    Custom,
}

impl VehicleModel {
    /// Whether this model may be selected in the current build. Models that
    /// need installed parameters are checked by `RaceWorld`.
    pub fn is_selectable(self) -> bool {
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance(
        self,
        vehicle: Vehicle,
        input: DriverInput,
        dt: f64,
        policy: ProvisionalPolicy,
        drive: &mut DriveState,
        scripted: ScriptedControl,
        arcade: Option<&ArcadeConfig>,
        classic: Option<&ClassicConfig>,
        custom: Option<&mut crate::model_ext::CustomModel>,
        road_slope: f64,
    ) -> KinematicUpdate {
        match self {
            Self::Arcade => match arcade {
                Some(cfg) => arcade_model::advance(cfg, vehicle, input, dt, drive),
                // RaceWorld never selects Arcade without parameters.
                None => provisional_model::advance(vehicle, input, dt, policy),
            },
            Self::Classic => match classic {
                Some(cfg) => classic_model::advance(cfg, vehicle, input, dt, drive, road_slope),
                // RaceWorld never selects Classic without parameters.
                None => provisional_model::advance(vehicle, input, dt, policy),
            },
            Self::Custom => match custom {
                Some(model) => {
                    model.dynamics.set_road_slope(road_slope);
                    let step = model.dynamics.step(vehicle, input, dt);
                    KinematicUpdate {
                        heading: step.heading,
                        angular_velocity: step.angular_velocity,
                        velocity: step.velocity,
                        proposed_position: step.proposed_position,
                    }
                }
                // RaceWorld never selects Custom without an installed model.
                None => provisional_model::advance(vehicle, input, dt, policy),
            },
            Self::Scripted => scripted_model::advance(vehicle, scripted, dt),
            Self::Provisional => provisional_model::advance(vehicle, input, dt, policy),
        }
    }
}
