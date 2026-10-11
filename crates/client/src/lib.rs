//! Browser-facing simulation facade. Rendering and browser input stay outside
//! the shared simulation crate.

use racer_content::LoadedScene;
use racer_render::{Vec3, VehicleProfileIdentity};
use racer_sim::{
    DriverInput, PreviewCollisionWorld, RaceWorld, RankEntry, SimTiming, TrackRuntime,
    Transmission, Vec2, VehicleContactBody, VehicleModel,
    model_ext::{VehicleModelProvider, VehicleModelRegistry},
    preview_route_audit::{
        ReferencePathPoint, RouteAuditConfig, RouteAuditError, RouteCoverageReport,
        audit_closed_reference_path,
    },
    rank_race, resolve_pairwise_vehicle_contacts,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientSnapshot {
    pub tick: u64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub ground_normal: [f64; 3],
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub heading: f64,
    pub speed: f64,
    pub lap: u32,
    pub checkpoint: usize,
    pub finished: bool,
    /// 1-based rank among local competitors (always 1 for solo practice).
    pub race_position: u16,
    /// Cars ranked together with this one (1 for solo practice).
    pub competitors: u16,
    /// Ticks (60 Hz race clock) in the lap in progress; 0 once finished.
    pub current_lap_ticks: u64,
    pub last_lap_ticks: Option<u64>,
    pub best_lap_ticks: Option<u64>,
    pub finish_ticks: Option<u64>,
    /// Gearbox gear 1..=5, or 0 when the active vehicle model has no gearbox.
    pub gear: u8,
    /// Engine-speed estimate in 0..=1 for HUD/audio (0 without a gearbox).
    pub rpm01: f64,
}

fn vehicle_models() -> &'static std::sync::Mutex<VehicleModelRegistry> {
    static REGISTRY: std::sync::OnceLock<std::sync::Mutex<VehicleModelRegistry>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(VehicleModelRegistry::new()))
}

/// Imported car data held by the client (see [`register_imported_car_parser`]).
/// The stock client never interprets it: a private build supplies the parser
/// and the model selection derived from the data.
pub trait ImportedCarData: Send {
    /// Registry id and parameters of the model that should drive the car, or
    /// `None` when the data is incomplete (the client then falls back to the
    /// registered default model).
    fn model_selection(&self) -> Option<(String, Vec<u8>)>;
    /// Diagnostic line for the import status UI.
    fn status_text(&self) -> String;
    /// JSON of the client message that carries the selection to the server,
    /// or `None` without complete data.
    fn message_json(&self) -> Option<String>;
    /// Downcast access for the private build that created the value.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Parses the descriptor's imported car-data JSON block.
pub type ImportedCarParser = fn(&str) -> Result<Box<dyn ImportedCarData>, String>;

static IMPORTED_CAR_PARSER: std::sync::OnceLock<ImportedCarParser> = std::sync::OnceLock::new();
static IMPORTED_DEFAULT_MODEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Install the parser used by `BrowserRace::set_imported_car_data` (once per build).
pub fn register_imported_car_parser(parser: ImportedCarParser) -> Result<(), String> {
    IMPORTED_CAR_PARSER
        .set(parser)
        .map_err(|_| "an imported car parser is already registered".to_owned())
}

/// Parse imported car-data JSON with the registered parser.
pub fn parse_imported_car_data(json: &str) -> Result<Box<dyn ImportedCarData>, String> {
    let parser = IMPORTED_CAR_PARSER
        .get()
        .ok_or_else(|| "imported car data is not supported by this build".to_owned())?;
    parser(json)
}

/// Name the registered vehicle model that imported open previews start with
/// (once per build). Without one they keep the engine's default model.
pub fn register_imported_default_model(id: &str) -> Result<(), String> {
    IMPORTED_DEFAULT_MODEL
        .set(id.to_owned())
        .map_err(|_| "an imported default model is already registered".to_owned())
}

fn imported_default_model() -> Option<&'static str> {
    IMPORTED_DEFAULT_MODEL.get().map(String::as_str)
}

/// Install the imported-preview default model into `world`, when one is registered.
fn install_default_model(world: &mut RaceWorld) -> Result<(), String> {
    match imported_default_model() {
        Some(id) => install_registered_model(world, id, b""),
        None => Ok(()),
    }
}

/// Install the registered model `id` into `world` and make it active.
fn install_registered_model(world: &mut RaceWorld, id: &str, params: &[u8]) -> Result<(), String> {
    let registry = vehicle_models();
    let registry = registry.lock().unwrap_or_else(|e| e.into_inner());
    world.install_custom_model(&registry, id, params)
}

/// Register an external vehicle model provider for this client build.
///
/// Providers are Rust code, so they are linked in at build time: a separate
/// crate depends on this one, calls this from its own start function
/// (for example a `#[wasm_bindgen(start)]` fn) and builds its own wasm. The
/// stock wasm has no providers registered.
pub fn register_vehicle_model(provider: Box<dyn VehicleModelProvider>) -> Result<(), String> {
    vehicle_models()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .register(provider)
}

/// Ids of the registered external vehicle models.
pub fn registered_vehicle_models() -> Vec<String> {
    vehicle_models()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ids()
        .map(str::to_string)
        .collect()
}

pub struct LocalRaceClient {
    world: RaceWorld,
    input: DriverInput,
    /// Player-local decode of the selected imported car. Selects the vehicle
    /// model in the imported open preview when complete. The raw data never
    /// leaves the device; only the derived selection can be sent to a server.
    imported_data: Option<Box<dyn ImportedCarData>>,
    /// True while the world is an imported open preview, the only context
    /// where the handling model follows the imported car data.
    imported_preview: bool,
    /// External vehicle model selected with `set_car_model` (id, parameters).
    car_model: Option<(String, Vec<u8>)>,
    /// Computer opponents on the project circuit (none by default).
    opponents: Option<racer_ai::Opponents>,
    /// Computer-driver road and ground of the active generated track.
    project_ai: Option<(std::sync::Arc<racer_ai::AiTrack>, PreviewCollisionWorld)>,
    /// Gates, laps and road of the imported track, when its road rows make a
    /// race course (see [`LocalRaceClient::set_imported_laps`]).
    imported_course: Option<racer_ai::ImportedCourse>,
    /// Catalogue index of the demo car the local driver chose; its classic
    /// parameters drive the human car and seed the opponents' cars.
    car: usize,
    /// Gearbox mode of the local driver, applied to every world the client
    /// builds (the worlds are replaced when the track changes).
    transmission: Transmission,
    auto_reverse: bool,
}

/// Contact radius of the human car (the project car).
const HUMAN_CONTACT_RADIUS: f64 = 1.0;
/// Local id of the human car; opponent ids start at 1000.
const LOCAL_HUMAN_ID: u32 = 1;
const LOCAL_OPPONENT_FIRST_ID: u32 = 1000;

/// One computer opponent as the browser needs it for drawing, HUD and audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpponentSnapshot {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub heading: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub speed: f64,
    pub laps: u32,
    pub checkpoint: usize,
    pub finished: bool,
    pub race_position: u16,
    /// Origin height (scene Z) on tracks with relief.
    pub z: f64,
}

/// Rotate a scene-metric support normal from the simulation's anchor-heading
/// frame into scene XY. The collision importer has already removed XY scale.
pub fn preview_normal_to_scene(normal_sim: [f64; 3], anchor_heading: f64) -> Option<[f64; 3]> {
    if normal_sim.iter().any(|value| !value.is_finite()) || !anchor_heading.is_finite() {
        return None;
    }
    let (sin, cos) = anchor_heading.sin_cos();
    let mut scene = [
        normal_sim[0] * cos - normal_sim[1] * sin,
        normal_sim[0] * sin + normal_sim[1] * cos,
        normal_sim[2],
    ];
    let length = (scene[0] * scene[0] + scene[1] * scene[1] + scene[2] * scene[2]).sqrt();
    if !length.is_finite() || length <= 1.0e-9 {
        return None;
    }
    for value in &mut scene {
        *value /= length;
    }
    Some(scene)
}

/// Parse the authoritative seven-float render pose format used by multiplayer
/// snapshots: x, y, z, heading, and the world-space support normal.
pub fn parse_profiled_render_poses(
    values: &[f32],
    profiles: &[VehicleProfileIdentity],
) -> Result<Vec<(Vec3, f32, Vec3)>, &'static str> {
    if profiles.len() > racer_render::MAX_RENDER_VEHICLES {
        return Err("render vehicle count exceeds the protocol limit");
    }
    if values.len() != profiles.len() * 7 {
        return Err("render pose and profile counts do not match");
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("render pose contains a non-finite value");
    }
    values
        .chunks_exact(7)
        .map(|pose| {
            const MAX_COORDINATE: f32 = 1_000_000.0;
            if pose[..4].iter().any(|value| value.abs() > MAX_COORDINATE) {
                return Err("render pose is outside the supported world bounds");
            }
            let normal = Vec3::new(pose[4], pose[5], pose[6]);
            if normal.x.abs() > 1.0001
                || normal.y.abs() > 1.0001
                || normal.z.abs() > 1.0001
                || normal.z <= 0.0
            {
                return Err("render pose contains an invalid ground normal");
            }
            let normal_length = normal.length();
            if !normal_length.is_finite() || normal_length <= 1.0e-6 {
                return Err("render pose contains an invalid ground normal");
            }
            Ok((
                Vec3::new(pose[0], pose[1], pose[2]),
                pose[3],
                normal.normalized(),
            ))
        })
        .collect()
}

impl Default for LocalRaceClient {
    fn default() -> Self {
        Self::new(60)
    }
}

impl LocalRaceClient {
    pub fn new(physics_hz: u32) -> Self {
        let timing = SimTiming {
            physics_hz: physics_hz.max(1),
            ..SimTiming::default()
        };
        let mut world = RaceWorld::new(timing, TrackRuntime::synthetic_circuit());
        let _ = world.use_classic_model(default_project_classic_config());
        Self {
            world,
            input: DriverInput::default(),
            imported_data: None,
            imported_preview: false,
            car_model: None,
            opponents: None,
            project_ai: None,
            imported_course: None,
            car: 0,
            transmission: Transmission::Automatic,
            auto_reverse: false,
        }
    }

    /// Choose the local driver's gearbox mode (automatic by default). Meant for
    /// the set-up screen: a change restarts the drivetrain in its start state.
    pub fn set_transmission(&mut self, transmission: Transmission) {
        self.transmission = transmission;
        self.world.set_transmission(transmission);
    }

    pub fn transmission(&self) -> Transmission {
        self.transmission
    }

    /// Auto reverse for the local driver (no shifting needed; see the sim).
    pub fn set_auto_reverse(&mut self, enabled: bool) {
        self.auto_reverse = enabled;
        self.world.set_auto_reverse(enabled);
    }

    pub fn auto_reverse(&self) -> bool {
        self.auto_reverse
    }

    /// Add computer opponents on the project circuit and line everybody up on
    /// the grid (the human takes the rearmost slot). `count` 0 removes them.
    /// The imported open preview has no race, but when its collision carries
    /// road-edge rows the opponents drive that road (free roaming, no laps).
    pub fn configure_opponents(&mut self, count: usize, skill: u8, catch_up: bool) {
        self.opponents = None;
        if count == 0 {
            return;
        }
        let track = self.world.track.clone();
        // A race on the imported track: the course's grid and road.
        let racing_course = self
            .imported_course
            .as_ref()
            .filter(|_| self.imported_preview && track.laps_to_finish > 0);
        let (ai_track, collision) = match (&self.project_ai, racing_course) {
            (Some((_, collision)), Some(course)) => (course.ai_track.clone(), Some(collision)),
            (Some((road, collision)), None) => (road.as_ref().clone(), Some(collision)),
            (None, _) if self.imported_preview => return,
            (None, _) => match racer_ai::circuit_ai_track(&track) {
                Ok(road) => (road, None),
                Err(_) => return,
            },
        };
        // Drive along the road in the direction the car is facing at the spawn
        // (the course is already oriented along the start heading).
        let direction = if racing_course.is_some() {
            1
        } else if self.imported_preview {
            let h = self.world.vehicle.heading;
            let row = &ai_track.rows[ai_track.locate(
                [self.world.vehicle.position.x, self.world.vehicle.position.y],
                None,
            )];
            if row.forward[0] * h.cos() + row.forward[1] * h.sin() < 0.0 {
                -1
            } else {
                1
            }
        } else {
            1
        };
        let settings = racer_ai::Settings {
            skill: racer_ai::Skill::from_level(skill),
            catch_up,
            laps: track.laps_to_finish,
            direction,
            seed: 0x5eed,
            tick_seconds: self.world.timing.step_seconds(),
            ..racer_ai::Settings::default()
        };
        let opponents = racer_ai::Opponents::new(
            &track,
            std::sync::Arc::new(ai_track),
            std::sync::Arc::new(racer_ai::AiConfig::default()),
            settings,
            count,
            LOCAL_OPPONENT_FIRST_ID,
            1,
            self.world.timing,
        );
        let mut opponents = opponents;
        if let Some(collision) = collision {
            opponents.install_collision(collision);
        }
        // Opponents drive different cars from the player's; each respects its
        // own car's top speed, acceleration, braking and grip.
        for k in 0..count.min(racer_ai::MAX_OPPONENTS) {
            let (car, _) = demo_cars().opponent_pick((self.car, 0), k);
            opponents.set_vehicle(LOCAL_OPPONENT_FIRST_ID + k as u32, car_driver_limits(car));
        }
        let (position, heading) = opponents.human_grid_slot(0, 1);
        self.world.place_vehicle(position, heading);
        self.opponents = Some(opponents);
    }

    /// Choose the demo car (catalogue index) whose classic parameters the
    /// local driver's car uses and from which the opponents' cars follow.
    /// Applied at once to the project circuit or generated track; the
    /// imported open preview keeps its own models. Opponents already created
    /// are re-assigned their cars.
    pub fn set_project_car(&mut self, car: usize) {
        let car = car.min(demo_cars().cars().len().saturating_sub(1));
        if car == self.car {
            return;
        }
        self.car = car;
        if !self.imported_preview {
            let _ = self.world.use_classic_model(car_classic_config(car));
        }
        self.reapply_custom_model();
        if let Some(opponents) = self.opponents.as_mut() {
            for k in 0..opponents.cars.len() {
                let (other, _) = demo_cars().opponent_pick((car, 0), k);
                opponents.set_vehicle(LOCAL_OPPONENT_FIRST_ID + k as u32, car_driver_limits(other));
            }
        }
    }

    /// Catalogue index of the chosen demo car.
    pub fn project_car(&self) -> usize {
        self.car
    }

    /// Computer opponents with their ranks, for drawing, HUD and audio.
    pub fn opponent_snapshots(&self) -> Vec<OpponentSnapshot> {
        let Some(opponents) = &self.opponents else {
            return Vec::new();
        };
        let positions = self.positions();
        opponents
            .cars
            .iter()
            .map(|car| {
                let v = car.world.vehicle;
                OpponentSnapshot {
                    id: car.id,
                    x: v.position.x,
                    y: v.position.y,
                    heading: v.heading,
                    velocity_x: v.velocity.x,
                    velocity_y: v.velocity.y,
                    speed: v.speed(),
                    laps: v.completed_laps,
                    checkpoint: v.checkpoint,
                    finished: v.finished,
                    z: v.position_z,
                    race_position: positions
                        .iter()
                        .find(|(id, _)| *id == car.id)
                        .map_or(0, |(_, p)| *p),
                }
            })
            .collect()
    }

    /// Ranks of the human (id 1) and every opponent.
    fn positions(&self) -> Vec<(u32, u16)> {
        let mut entries: Vec<(u32, RankEntry)> =
            vec![(LOCAL_HUMAN_ID, self.world.rank_entry(LOCAL_HUMAN_ID))];
        if let Some(o) = &self.opponents {
            entries.extend(o.cars.iter().map(|c| (c.id, c.world.rank_entry(c.id))));
        }
        entries.sort_by_key(|(id, _)| *id);
        let list: Vec<RankEntry> = entries.iter().map(|(_, e)| *e).collect();
        let ranks = rank_race(self.world.track.checkpoints.len(), &list);
        entries.iter().map(|(id, _)| *id).zip(ranks).collect()
    }

    pub fn set_imported_data(&mut self, data: Option<Box<dyn ImportedCarData>>) {
        self.imported_data = data;
        self.refresh_handling_model();
    }

    /// Imported previews use the model the car data selects when the data is
    /// complete and valid, otherwise the registered default model. The project
    /// circuit keeps the provisional model.
    fn refresh_handling_model(&mut self) {
        self.refresh_imported_handling_model();
        self.reapply_custom_model();
    }

    /// Install the selected external model (if any) into the current world.
    fn reapply_custom_model(&mut self) {
        let Some((id, params)) = self.car_model.clone() else {
            return;
        };
        let registry = vehicle_models();
        let registry = registry.lock().unwrap_or_else(|e| e.into_inner());
        if self.world.custom_model_id() == Some(id.as_str())
            && self.world.vehicle_model() == VehicleModel::Custom
        {
            return;
        }
        let _ = self.world.install_custom_model(&registry, &id, &params);
    }

    /// Select an external vehicle model registered with
    /// [`register_vehicle_model`] for solo driving. Unknown ids or parameters
    /// the provider rejects leave the previous model active.
    pub fn set_car_model(&mut self, model_id: &str, params: &[u8]) -> Result<(), String> {
        racer_protocol::validate_car_model(model_id, params).map_err(str::to_string)?;
        {
            let registry = vehicle_models();
            let registry = registry.lock().unwrap_or_else(|e| e.into_inner());
            self.world
                .install_custom_model(&registry, model_id, params)?;
        }
        self.car_model = Some((model_id.to_string(), params.to_vec()));
        Ok(())
    }

    /// Drop the external selection and return to the built-in model.
    pub fn clear_car_model(&mut self) {
        if self.car_model.take().is_some() {
            if self.imported_preview {
                self.refresh_imported_handling_model();
            } else {
                let _ = self.world.use_classic_model(car_classic_config(self.car));
            }
        }
    }

    /// The `SetCarModel` message for room play, if an external model is selected.
    pub fn car_model_message(&self) -> Option<racer_protocol::ClientMessage> {
        let (model_id, params) = self.car_model.clone()?;
        Some(racer_protocol::ClientMessage::SetCarModel { model_id, params })
    }

    fn refresh_imported_handling_model(&mut self) {
        if !self.imported_preview {
            return;
        }
        let selection = self
            .imported_data
            .as_ref()
            .and_then(|data| data.model_selection());
        let installed = selection.is_some_and(|(id, params)| {
            install_registered_model(&mut self.world, &id, &params).is_ok()
        });
        if !installed && !self.default_model_active() {
            let _ = install_default_model(&mut self.world);
        }
    }

    fn default_model_active(&self) -> bool {
        self.world.vehicle_model() == VehicleModel::Custom
            && self.world.custom_model_id() == imported_default_model()
    }

    /// Id of the installed provider model while it is the active model.
    fn active_custom_id(&self) -> Option<&str> {
        (self.world.vehicle_model() == VehicleModel::Custom)
            .then(|| self.world.custom_model_id())
            .flatten()
    }

    pub fn handling_model(&self) -> VehicleModel {
        self.world.vehicle_model()
    }

    /// Diagnostic line for the import status and HUD.
    pub fn handling_status_text(&self) -> String {
        match self.world.vehicle_model() {
            VehicleModel::Provisional => "handling: provisional".into(),
            VehicleModel::Scripted => "handling: scripted".into(),
            VehicleModel::Arcade => "handling: arcade".into(),
            VehicleModel::Classic => "handling: classic".into(),
            VehicleModel::Custom => match self.active_custom_id() {
                Some(id) => format!("handling: {id}"),
                None => "handling: custom".into(),
            },
        }
    }

    /// Protocol `HANDLING_*` code of the active vehicle model.
    pub fn handling_code(&self) -> u8 {
        match self.world.vehicle_model() {
            VehicleModel::Provisional => racer_protocol::HANDLING_PROVISIONAL,
            VehicleModel::Scripted => racer_protocol::HANDLING_SCRIPTED,
            VehicleModel::Arcade => racer_protocol::HANDLING_ARCADE,
            VehicleModel::Classic => racer_protocol::HANDLING_CLASSIC,
            VehicleModel::Custom => {
                let registry = vehicle_models().lock().unwrap_or_else(|e| e.into_inner());
                self.active_custom_id()
                    .map_or(racer_protocol::HANDLING_EXTERNAL, |id| {
                        registry.wire_code(id)
                    })
            }
        }
    }

    /// JSON of the client message carrying the derived model selection (never
    /// raw data). `None` without complete car data.
    pub fn imported_message_json(&self) -> Option<String> {
        self.imported_data.as_ref()?.message_json()
    }

    pub fn imported_data(&self) -> Option<&dyn ImportedCarData> {
        self.imported_data.as_deref()
    }

    pub fn set_input(&mut self, input: DriverInput) {
        self.input = input;
    }

    /// Shift requests (up, down) for the next tick(s), kept separate from the
    /// pedal and steering update so a caller can latch a short key press.
    pub fn set_shift(&mut self, shift_up: bool, shift_down: bool) {
        self.input.shift_up = shift_up;
        self.input.shift_down = shift_down;
    }

    /// Advance one simulation tick. Call from a fixed-rate accumulator, never
    /// directly once per rendered frame.
    pub fn tick(&mut self) -> ClientSnapshot {
        // A rebuilt world (track change) starts without a gearbox choice.
        self.world.set_transmission(self.transmission);
        self.world.set_auto_reverse(self.auto_reverse);
        let Some(opponents) = self.opponents.as_mut() else {
            self.world.step(self.input);
            return self.snapshot();
        };
        let human = racer_ai::CarObs::from_world(
            LOCAL_HUMAN_ID,
            &self.world,
            true,
            racer_ai::race::OPPONENT_HALF_WIDTH,
        );
        let opponent_before: Vec<(u32, Vec2)> = opponents
            .cars
            .iter()
            .map(|c| (c.id, c.world.vehicle.position))
            .collect();
        opponents.step(&[human]);
        let human_before = self.world.vehicle.position;
        self.world.step(self.input);
        let (min_z, max_z) = (-0.5, 0.5);
        let mut bodies = vec![VehicleContactBody {
            stable_id: LOCAL_HUMAN_ID,
            vehicle: self.world.vehicle,
            previous_position: human_before,
            radius: HUMAN_CONTACT_RADIUS,
            min_z,
            max_z,
        }];
        let mut after = vec![(LOCAL_HUMAN_ID, self.world.vehicle.position)];
        for car in &opponents.cars {
            let before = opponent_before
                .iter()
                .find(|(id, _)| *id == car.id)
                .map_or(car.world.vehicle.position, |(_, p)| *p);
            bodies.push(VehicleContactBody {
                stable_id: car.id,
                vehicle: car.world.vehicle,
                previous_position: before,
                radius: racer_ai::race::OPPONENT_CONTACT_RADIUS,
                min_z,
                max_z,
            });
            after.push((car.id, car.world.vehicle.position));
        }
        if resolve_pairwise_vehicle_contacts(&mut bodies).is_ok() {
            for body in bodies {
                let previous = after
                    .iter()
                    .find(|(id, _)| *id == body.stable_id)
                    .map_or(body.vehicle.position, |(_, p)| *p);
                let displaced = previous != body.vehicle.position;
                let world = if body.stable_id == LOCAL_HUMAN_ID {
                    Some(&mut self.world)
                } else {
                    opponents
                        .cars
                        .iter_mut()
                        .find(|c| c.id == body.stable_id)
                        .map(|c| &mut c.world)
                };
                if let Some(world) = world {
                    world.vehicle = body.vehicle;
                    if displaced {
                        world.constrain_after_vehicle_contact(previous);
                    }
                }
            }
        }
        self.snapshot()
    }

    pub fn reset(&mut self) {
        self.input.reset_request = true;
        self.world.step(self.input);
        self.input.reset_request = false;
    }

    /// Race the imported track for `laps` laps (a point-to-point track has a
    /// single run). Returns false and leaves free driving when `laps` is 0 or
    /// the track's road rows do not make a course. Call right after
    /// [`Self::use_open_preview_with_collision`], before adding opponents.
    pub fn set_imported_laps(&mut self, laps: u32) -> bool {
        self.opponents = None;
        if !self.imported_preview {
            return false;
        }
        let Some(course) = self.imported_course.as_ref().filter(|_| laps > 0) else {
            self.world.track = TrackRuntime::open_preview();
            self.world.vehicle.checkpoint = 0;
            return false;
        };
        self.world.track = course.runtime_for_laps(laps);
        self.world.vehicle.checkpoint = 1.min(self.world.track.checkpoints.len());
        true
    }

    /// True when the imported track has a race course (laps can be raced).
    pub fn imported_course_available(&self) -> bool {
        self.imported_preview && self.imported_course.is_some()
    }

    /// Laps of the race on the imported track (0 when free driving).
    pub fn imported_laps(&self) -> u32 {
        if self.imported_preview {
            self.world.track.laps_to_finish
        } else {
            0
        }
    }

    pub fn use_open_preview(&mut self) {
        self.imported_course = None;
        let timing = self.world.timing;
        let mut world = RaceWorld::new(timing, TrackRuntime::open_preview());
        // Imported profiles start with the registered default model, or the
        // one the imported car data selects when it is complete.
        let _ = install_default_model(&mut world);
        self.world = world;
        self.input = DriverInput::default();
        self.imported_preview = true;
        self.opponents = None;
        self.project_ai = None;
        self.refresh_handling_model();
    }

    pub fn use_project_circuit(&mut self) {
        let timing = self.world.timing;
        self.world = RaceWorld::new(timing, TrackRuntime::synthetic_circuit());
        let _ = self.world.use_classic_model(car_classic_config(self.car));
        self.input = DriverInput::default();
        self.imported_preview = false;
        self.opponents = None;
        self.project_ai = None;
        self.imported_course = None;
        self.reapply_custom_model();
    }

    pub fn use_open_preview_with_collision(
        &mut self,
        collision: PreviewCollisionWorld,
    ) -> Result<(), &'static str> {
        let timing = self.world.timing;
        let mut world = RaceWorld::new(timing, TrackRuntime::open_preview());
        install_default_model(&mut world)
            .map_err(|_| "the default imported-car vehicle model is not available")?;
        let road_collision = collision.clone();
        world.install_preview_collision(collision)?;
        self.world = world;
        self.input = DriverInput::default();
        self.imported_preview = true;
        self.opponents = None;
        self.imported_course = racer_ai::imported_course(
            &road_collision.edge_rows,
            self.world.vehicle.position,
            self.world.vehicle.heading,
        );
        self.project_ai = imported_road_ai(&self.world, road_collision);
        self.refresh_handling_model();
        Ok(())
    }

    /// Switch to a generated project track: gates, laps, road-edge walls and
    /// ground collision all come from the RCP track. The spawn is slot 0 of
    /// the start grid (the simulation origin).
    pub fn use_project_track(&mut self, track: &RcpTrackScene) -> Result<(), &'static str> {
        let timing = self.world.timing;
        let mut world = RaceWorld::new(timing, track.runtime.clone());
        let spawn = track
            .spawn_slots
            .first()
            .map_or(Vec2::new(0.0, 0.0), |s| Vec2::new(s.x, s.y));
        world.set_spawn(spawn);
        world.install_preview_collision(track.collision.clone())?;
        let _ = world.use_classic_model(car_classic_config(self.car));
        self.world = world;
        self.input = DriverInput::default();
        self.imported_preview = false;
        self.opponents = None;
        self.imported_course = None;
        let frames = racer_ai::race::road_frames_from_edge_rows(&track.collision.edge_rows, 1.0);
        self.project_ai = racer_ai::AiTrack::from_pack_track_frames(
            &track.pack_track,
            &frames,
            &racer_ai::DeriveParams::default(),
        )
        .ok()
        .map(|road| (std::sync::Arc::new(road), track.collision.clone()));
        Ok(())
    }

    pub fn has_preview_ground(&self) -> bool {
        self.world.has_preview_collision()
    }

    pub fn snapshot(&self) -> ClientSnapshot {
        let v = self.world.vehicle;
        let lap_times = self.world.lap_times();
        ClientSnapshot {
            tick: self.world.tick,
            x: v.position.x,
            y: v.position.y,
            z: v.position_z,
            ground_normal: self
                .world
                .preview_ground_normal()
                .unwrap_or([0.0, 0.0, 1.0]),
            velocity_x: v.velocity.x,
            velocity_y: v.velocity.y,
            heading: v.heading,
            speed: v.speed(),
            lap: v.completed_laps + 1,
            checkpoint: v.checkpoint,
            finished: v.finished,
            race_position: self
                .positions()
                .iter()
                .find(|(id, _)| *id == LOCAL_HUMAN_ID)
                .map_or(1, |(_, p)| *p),
            competitors: 1 + self.opponents.as_ref().map_or(0, |o| o.cars.len() as u16),
            current_lap_ticks: self.world.current_lap_ticks(),
            last_lap_ticks: lap_times.last_lap_ticks,
            best_lap_ticks: lap_times.best_lap_ticks,
            finish_ticks: lap_times.finish_ticks,
            gear: self.world.drive_telemetry().gear,
            rpm01: self.world.drive_telemetry().rpm01,
        }
    }
}

use racer_content::classic_params::{classic_config, default_project_classic_config};
use racer_content::project_cars::project_cars as demo_cars;

/// Classic handling parameters of the demo car with catalogue index `index`
/// (the default car for an index outside the catalogue).
fn car_classic_config(index: usize) -> racer_sim::ClassicConfig {
    demo_cars()
        .cars()
        .get(index)
        .and_then(|car| classic_config(&car.vehicle))
        .unwrap_or_else(default_project_classic_config)
}

/// Driver limits of the demo car with catalogue index `index`.
fn car_driver_limits(index: usize) -> racer_ai::VehicleAi {
    racer_ai::vehicle_ai_from_classic(&car_classic_config(index))
}
pub use racer_content::derive_preview_collision;
pub use racer_content::project_tracks::{
    PROJECT_TRACK_CONTENT_ID, load_project_track, project_track_catalog, project_track_info,
    project_track_profile_id,
};
pub use racer_content::rcp_track::RcpTrackScene;

/// JSON array describing the selectable generated tracks.
pub fn project_track_catalog_json() -> String {
    let entries: Vec<serde_json::Value> = project_track_catalog()
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "profile_id": project_track_profile_id(&t.id),
                "name": t.name,
                "description": t.description,
                "theme": t.theme,
                "fingerprint": t.fingerprint,
                "laps": t.laps,
            })
        })
        .collect();
    serde_json::Value::Array(entries).to_string()
}

/// Pose mapping and summary of a loaded project track, in the same shape the
/// page uses for other local scenes (`spawn_pose_mapping.anchor_scene_*`).
pub fn project_track_metadata(track: &RcpTrackScene) -> serde_json::Value {
    let p = track.anchor_scene_position;
    let f = track.anchor_scene_forward;
    serde_json::json!({
        "track_id": track.track_id,
        "name": track.name,
        "laps": track.laps,
        "checkpoints": track.runtime.checkpoints.len(),
        "ground_triangles": track.ground_triangles,
        "spawn_pose_mapping": {
            "anchor_scene_position_m": [p.x, p.y, p.z],
            "anchor_scene_forward": [f.x, f.y, f.z],
            "confidence": "generated project track",
        },
        "spawn_slots": track.spawn_slots.iter().map(|s| serde_json::json!([s.x, s.y, s.heading])).collect::<Vec<_>>(),
        "has_ai": track.pack_track.ai.is_some(),
        "map": project_track_map(track),
    })
}

/// Centerline and road edges of a generated track in scene XY (metres), thinned
/// to at most 240 points, for the client's mini map and wrong-way check.
fn project_track_map(track: &RcpTrackScene) -> serde_json::Value {
    let segments = &track.pack_track.centerline;
    let step = segments.len().div_ceil(240).max(1);
    let round = |v: f64| (v * 10.0).round() / 10.0;
    let (mut center, mut left, mut right) = (Vec::new(), Vec::new(), Vec::new());
    for seg in segments.iter().step_by(step) {
        // pack frame (x, y up, z) -> scene (x, -z)
        let (px, py) = (f64::from(seg.position[0]), -f64::from(seg.position[2]));
        let (fx, fy) = (f64::from(seg.forward[0]), -f64::from(seg.forward[2]));
        let norm = fx.hypot(fy).max(1e-9);
        let (fx, fy) = (fx / norm, fy / norm);
        let (lw, rw) = (f64::from(seg.left_width_m), f64::from(seg.right_width_m));
        center.push([round(px), round(py)]);
        left.push([round(px - fy * lw), round(py + fx * lw)]);
        right.push([round(px + fy * rw), round(py - fx * rw)]);
    }
    serde_json::json!({ "closed": true, "centerline": center, "left": left, "right": right })
}

/// Mini-map outline of the built-in test circuit: its ordered gates in
/// simulation XY and their radius (half the road width).
pub fn circuit_map_json() -> String {
    let runtime = TrackRuntime::synthetic_circuit();
    let points: Vec<[f64; 2]> = runtime
        .checkpoints
        .iter()
        .map(|c| [c.center.x, c.center.y])
        .collect();
    let half_width = runtime.checkpoints.first().map_or(5.0, |c| c.radius);
    serde_json::json!({ "closed": true, "smooth": true, "centerline": points, "half_width": half_width })
        .to_string()
}

/// Audit the optional owner-local reference path against the exact walkable
/// triangles derived for this loaded preview. Missing legacy cache metadata
/// returns `ReferencePathUnavailable`; it is never reconstructed by guesswork.
/// Road for the computer opponents of an imported open preview, derived from
/// the collision's road-edge rows (none when the scene has too few). The grid
/// is placed so the human's slot is at the spawn.
fn imported_road_ai(
    world: &RaceWorld,
    collision: PreviewCollisionWorld,
) -> Option<(std::sync::Arc<racer_ai::AiTrack>, PreviewCollisionWorld)> {
    if collision.edge_rows.len() < 3 {
        return None;
    }
    let frames = racer_ai::race::road_frames_from_edge_rows(&collision.edge_rows, 1.0);
    let first = frames.first()?.position;
    let last = frames.last()?.position;
    let gap = (first[0] - last[0]).hypot(first[1] - last[1]);
    let mut road =
        racer_ai::AiTrack::from_frames(&frames, gap < 30.0, &racer_ai::DeriveParams::default())
            .ok()?;
    let spawn = world.vehicle.position;
    let near = road.locate([spawn.x, spawn.y], None);
    let heading = world.vehicle.heading;
    let forward = road.rows[near].forward;
    road.grid_spacing = (8.0 / road.row_length).round().max(1.0) as usize;
    // Grid slot 0 sits one row past `grid_start` in the driving direction.
    let dir = if forward[0] * heading.cos() + forward[1] * heading.sin() < 0.0 {
        1
    } else {
        -1
    };
    road.grid_start = road.step(near, dir, 1);
    Some((std::sync::Arc::new(road), collision))
}

pub fn audit_imported_preview_route(
    loaded: &LoadedScene,
    collision: &PreviewCollisionWorld,
) -> Result<RouteCoverageReport, RouteAuditError> {
    let scene_path = loaded
        .preview_reference_path_scene
        .as_ref()
        .ok_or(RouteAuditError::ReferencePathUnavailable)?;
    let anchor = loaded.spawn_pose_mapping.anchor_scene_position_m;
    let forward = loaded.spawn_pose_mapping.anchor_scene_forward;
    let heading_offset = f64::from(forward.y.atan2(forward.x));
    if !heading_offset.is_finite() {
        return Err(RouteAuditError::NonFinitePath);
    }
    let (sin, cos) = heading_offset.sin_cos();
    let mut path = Vec::with_capacity(scene_path.len());
    for point in scene_path {
        let sim =
            preview_scene_point_to_sim(*point, anchor, sin, cos, collision.units_per_sim_unit)
                .map_err(|_| RouteAuditError::NonFinitePath)?;
        path.push(ReferencePathPoint {
            x: sim.x,
            y: sim.y,
            z: f64::from(point.z),
        });
    }
    audit_closed_reference_path(
        collision,
        &path,
        RouteAuditConfig {
            units_per_sim_unit: collision.units_per_sim_unit,
            max_spacing_scene: 0.5,
        },
    )
}

fn preview_scene_point_to_sim(
    point: Vec3,
    anchor: Vec3,
    sin: f64,
    cos: f64,
    units_per_sim_unit: f64,
) -> Result<Vec2, String> {
    let dx = (f64::from(point.x) - f64::from(anchor.x)) / units_per_sim_unit;
    let dy = (f64::from(point.y) - f64::from(anchor.y)) / units_per_sim_unit;
    let transformed = Vec2::new(dx * cos + dy * sin, -dx * sin + dy * cos);
    if !transformed.x.is_finite()
        || !transformed.y.is_finite()
        || transformed.x.abs() > 1.0e6
        || transformed.y.abs() > 1.0e6
    {
        return Err("preview collision coordinate exceeds its world bound".into());
    }
    Ok(transformed)
}

#[cfg(test)]
mod progression_tests {
    use super::*;

    #[test]
    fn solo_snapshot_exposes_position_and_lap_timing() {
        let mut client = LocalRaceClient::new(60);
        let first = client.snapshot();
        assert_eq!(first.race_position, 1);
        assert_eq!(first.current_lap_ticks, 0);
        assert_eq!((first.last_lap_ticks, first.best_lap_ticks), (None, None));
        for _ in 0..30 {
            client.tick();
        }
        let later = client.snapshot();
        assert_eq!(later.current_lap_ticks, 30);
        assert_eq!(later.finish_ticks, None);
        assert_eq!(later.race_position, 1);
    }
}

#[cfg(test)]
mod transmission_tests {
    use super::*;

    fn gear_after(client: &mut LocalRaceClient, up: bool, down: bool) -> u8 {
        client.set_shift(up, down);
        client.tick();
        client.set_shift(false, false);
        for _ in 0..3 {
            client.tick();
        }
        client.world.drive_telemetry().gear
    }

    #[test]
    fn solo_manual_box_starts_in_neutral_and_walks_the_gears() {
        let mut client = LocalRaceClient::new(60);
        client.tick();
        assert_eq!(
            client.world.drive_telemetry().gear,
            1,
            "automatic: first gear"
        );
        client.set_transmission(Transmission::Manual);
        client.tick();
        assert_eq!(client.world.drive_telemetry().gear, 251, "manual: neutral");
        assert_eq!(gear_after(&mut client, true, false), 1);
        assert_eq!(gear_after(&mut client, true, false), 2);
        assert_eq!(gear_after(&mut client, false, true), 1);
    }

    #[test]
    fn the_choice_survives_a_rebuilt_world() {
        let mut client = LocalRaceClient::new(60);
        client.set_transmission(Transmission::Manual);
        client.use_project_circuit();
        client.tick();
        assert!(client.world.transmission().is_manual());
        assert_eq!(client.world.drive_telemetry().gear, 251);
    }
}

#[cfg(test)]
mod preview_collision_tests {
    use super::*;
    use racer_content::{Bounds3, SourceToScene, SpawnPoseMapping};
    use racer_render::{
        IndexedMesh, Material, MeshInstance, MeshVertex, RgbaTextureArray, Scene3d, UvWrap,
    };

    /// Stand-in for the private `calibrated` provider that imported previews install.
    struct StubDynamics;
    impl racer_sim::model_ext::VehicleDynamics for StubDynamics {
        fn clone_box(&self) -> Box<dyn racer_sim::model_ext::VehicleDynamics> {
            Box::new(StubDynamics)
        }
        fn step(
            &mut self,
            vehicle: racer_sim::Vehicle,
            _input: DriverInput,
            dt: f64,
        ) -> racer_sim::model_ext::VehicleStep {
            racer_sim::model_ext::VehicleStep {
                heading: vehicle.heading,
                angular_velocity: 0.0,
                velocity: Vec2::new(0.0, 0.0),
                proposed_position: vehicle.position + Vec2::new(0.0, 0.0) * dt,
            }
        }
    }
    const STUB_DEFAULT: &str = "stub.default";
    struct StubDefault;
    impl VehicleModelProvider for StubDefault {
        fn id(&self) -> &str {
            STUB_DEFAULT
        }
        fn create(
            &self,
            _params: &[u8],
        ) -> Result<Box<dyn racer_sim::model_ext::VehicleDynamics>, String> {
            Ok(Box::new(StubDynamics))
        }
    }
    fn stub_imported_models() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = register_vehicle_model(Box::new(StubDefault));
            let _ = register_imported_default_model(STUB_DEFAULT);
        });
    }

    fn vertex(position: Vec3, material: u32) -> MeshVertex {
        MeshVertex {
            position,
            normal: Vec3::UP,
            uv: [0.0, 0.0],
            material,
        }
    }

    fn loaded_scene(wall_alpha: f32) -> LoadedScene {
        let mut track = IndexedMesh::default();
        // Two upward ground triangles around the spawn.
        track.triangle(
            vertex(Vec3::new(-2.0, -2.0, 0.0), 0),
            vertex(Vec3::new(2.0, -2.0, 0.0), 0),
            vertex(Vec3::new(2.0, 2.0, 0.0), 0),
        );
        track.triangle(
            vertex(Vec3::new(-2.0, -2.0, 0.0), 0),
            vertex(Vec3::new(2.0, 2.0, 0.0), 0),
            vertex(Vec3::new(-2.0, 2.0, 0.0), 0),
        );
        // A two-triangle vertical wall. Its shared base edge should be emitted
        // once; a fully transparent material should be excluded by metadata.
        track.triangle(
            vertex(Vec3::new(1.0, -1.0, 0.0), 1),
            vertex(Vec3::new(1.0, 1.0, 0.0), 1),
            vertex(Vec3::new(1.0, 1.0, 2.0), 1),
        );
        track.triangle(
            vertex(Vec3::new(1.0, -1.0, 0.0), 1),
            vertex(Vec3::new(1.0, 1.0, 2.0), 1),
            vertex(Vec3::new(1.0, -1.0, 2.0), 1),
        );
        let mut car = IndexedMesh::default();
        car.triangle(
            vertex(Vec3::new(-0.5, -0.5, 0.0), 0),
            vertex(Vec3::new(0.5, -0.5, 0.0), 0),
            vertex(Vec3::new(0.0, 0.5, 1.0), 0),
        );
        LoadedScene {
            scene: Scene3d {
                meshes: vec![track, car],
                background_mesh_indices: Vec::new(),
                instances: vec![
                    MeshInstance {
                        mesh_index: 0,
                        translation: Vec3::ZERO,
                        heading: 0.0,
                        ground_normal: Vec3::UP,
                        scale: 1.0,
                    },
                    MeshInstance {
                        mesh_index: 1,
                        translation: Vec3::ZERO,
                        heading: 0.0,
                        ground_normal: Vec3::UP,
                        scale: 1.0,
                    },
                ],
                materials: vec![
                    Material::whole_layer("ground", [1.0; 4], 0, UvWrap::Clamp),
                    Material::whole_layer("wall", [1.0, 1.0, 1.0, wall_alpha], 0, UvWrap::Clamp),
                ],
                textures: RgbaTextureArray {
                    width: 1,
                    height: 1,
                    layers: vec![vec![255; 4]],
                },
            },
            vehicle_instance_index: 1,
            source_identity: None,
            track_mesh_indices: vec![0],
            texture_asset_ids: vec!["synthetic".into()],
            track_bounds: Bounds3 {
                min: Vec3::new(-2.0, -2.0, 0.0),
                max: Vec3::new(2.0, 2.0, 2.0),
            },
            source_to_scene: SourceToScene {
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                source_scale_m: 1.0,
            },
            spawn_pose_mapping: SpawnPoseMapping {
                anchor_source_position_m: Vec3::ZERO,
                anchor_scene_position_m: Vec3::ZERO,
                anchor_source_forward: Vec3::new(1.0, 0.0, 0.0),
                anchor_scene_forward: Vec3::new(1.0, 0.0, 0.0),
                confidence: "synthetic".into(),
            },
            preview_reference_path_scene: None,
            track_edge_rows_scene: Vec::new(),
        }
    }

    #[test]
    fn imported_track_collision_derives_walkable_floor_and_deduplicated_wall() {
        stub_imported_models();
        let collision = derive_preview_collision(&loaded_scene(1.0), 1.0).unwrap();
        assert_eq!(collision.ground.len(), 2);
        assert_eq!(collision.walls.len(), 1);
        assert!((collision.walls[0].start.x - 1.0).abs() < 1.0e-6);
        assert!((collision.walls[0].end.x - 1.0).abs() < 1.0e-6);
        assert!((collision.vehicle_radius - std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-5);
        collision.validate().unwrap();
    }

    #[test]
    fn imported_path_audits_against_the_derived_preview_ground() {
        stub_imported_models();
        let mut loaded = loaded_scene(1.0);
        loaded.preview_reference_path_scene = Some(vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 0.0),
        ]);
        let collision = derive_preview_collision(&loaded, 1.0).unwrap();
        let report = audit_imported_preview_route(&loaded, &collision).unwrap();
        assert_eq!(report.unsupported_count, 0);
        assert!(report.seam_supported);
        assert!(report.seam_matches_initial_support);
    }

    #[test]
    fn old_cached_scene_reports_route_diagnostic_unavailable() {
        stub_imported_models();
        let loaded = loaded_scene(1.0);
        let collision = derive_preview_collision(&loaded, 1.0).unwrap();
        assert_eq!(
            audit_imported_preview_route(&loaded, &collision),
            Err(RouteAuditError::ReferencePathUnavailable)
        );
    }

    #[test]
    fn imported_collision_skips_only_explicitly_transparent_wall_materials() {
        stub_imported_models();
        let collision = derive_preview_collision(&loaded_scene(0.0), 1.0).unwrap();
        assert_eq!(collision.walls.len(), 0);
        assert_eq!(collision.ground.len(), 2);
    }

    #[test]
    fn imported_collision_accepts_reversed_ground_triangle_winding() {
        stub_imported_models();
        let mut scene = loaded_scene(1.0);
        for triangle in scene.scene.meshes[0].indices[..6].chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        let collision = derive_preview_collision(&scene, 1.0).unwrap();
        assert_eq!(collision.ground.len(), 2);
        collision.validate_initial_support().unwrap();
    }

    #[test]
    fn imported_collision_inverse_transforms_geometry_and_scales_footprint() {
        stub_imported_models();
        let mut scene = loaded_scene(1.0);
        scene.spawn_pose_mapping.anchor_scene_position_m = Vec3::ZERO;
        scene.spawn_pose_mapping.anchor_scene_forward = Vec3::new(0.0, 1.0, 0.0);
        let collision = derive_preview_collision(&scene, 2.0).unwrap();
        assert!((collision.walls[0].start.x.abs() - 0.5).abs() < 1.0e-6);
        assert!((collision.walls[0].start.y + 0.5).abs() < 1.0e-6);
        assert!((collision.walls[0].end.x.abs() - 0.5).abs() < 1.0e-6);
        assert!((collision.walls[0].end.y + 0.5).abs() < 1.0e-6);
        assert!((collision.vehicle_radius - 0.3535533).abs() < 1.0e-5);
    }

    #[test]
    fn imported_collision_classifies_ramp_after_simulation_scale_transform() {
        stub_imported_models();
        let mut scene = loaded_scene(1.0);
        for vertex in &mut scene.scene.meshes[0].vertices[..6] {
            vertex.position.z = vertex.position.x * 1.5;
        }
        let collision = derive_preview_collision(&scene, 0.5).unwrap();
        assert_eq!(collision.ground.len(), 2);
        collision.validate_initial_support().unwrap();
    }

    #[test]
    fn imported_collision_carries_road_edge_rows_in_simulation_coordinates() {
        stub_imported_models();
        let mut scene = loaded_scene(1.0);
        scene.spawn_pose_mapping.anchor_scene_forward = Vec3::new(0.0, 1.0, 0.0);
        scene.track_edge_rows_scene = vec![racer_content::SceneEdgeRow {
            position: Vec3::new(0.0, 10.0, 0.25),
            forward: [0.0, 1.0],
            right: [1.0, 0.0],
            left_distance: 2.0,
            right_distance: 3.0,
            block: 7,
        }];
        let collision = derive_preview_collision(&scene, 0.5).unwrap();
        let row = collision.edge_rows[0];
        assert!(
            (row.x - 20.0).abs() < 1.0e-5 && row.y.abs() < 1.0e-5,
            "{row:?}"
        );
        assert!((row.forward.x - 1.0).abs() < 1.0e-5 && row.forward.y.abs() < 1.0e-5);
        // Scene right (+X) becomes sim -Y when the spawn heading is +Y.
        assert!(row.right.x.abs() < 1.0e-5 && (row.right.y + 1.0).abs() < 1.0e-5);
        assert_eq!(
            (row.left_distance, row.right_distance, row.block),
            (4.0, 6.0, 7)
        );
        assert_eq!(row.z, 0.25);
        LocalRaceClient::new(60)
            .use_open_preview_with_collision(collision)
            .unwrap();
    }

    #[test]
    fn imported_open_preview_with_road_rows_gets_driving_opponents() {
        stub_imported_models();
        // A long straight road along +X with relief-free ground under it.
        let rows: Vec<racer_sim::TrackEdgeRow> = (0..300)
            .map(|i| racer_sim::TrackEdgeRow {
                x: f64::from(i) * 4.0 - 40.0,
                y: 0.0,
                z: 0.0,
                forward: Vec2::new(1.0, 0.0),
                right: Vec2::new(0.0, -1.0),
                left_distance: 7.0,
                right_distance: 7.0,
                block: 0,
            })
            .collect();
        let collision = PreviewCollisionWorld {
            ground: vec![
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-100.0, -50.0, 0.0],
                        [2000.0, -50.0, 0.0],
                        [2000.0, 50.0, 0.0],
                    ],
                },
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-100.0, -50.0, 0.0],
                        [2000.0, 50.0, 0.0],
                        [-100.0, 50.0, 0.0],
                    ],
                },
            ],
            walls: Vec::new(),
            vehicle_radius: 0.25,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: rows,
        };
        let mut client = LocalRaceClient::new(60);
        client.use_open_preview_with_collision(collision).unwrap();
        client.configure_opponents(3, 1, true);
        let start = client.opponent_snapshots();
        assert_eq!(
            start.len(),
            3,
            "road rows must give the open preview opponents"
        );
        // The human keeps the rearmost slot; the opponents are ahead of the player, facing along the road.
        let human_x = client.snapshot().x;
        assert!(
            start.iter().all(|o| o.x > human_x),
            "{start:?} vs {human_x}"
        );
        for _ in 0..600 {
            client.tick();
        }
        let end = client.opponent_snapshots();
        assert!(
            end.iter()
                .zip(&start)
                .all(|(e, s)| e.x - s.x > 50.0 && e.y.abs() < 8.0),
            "opponents did not drive down the road: {start:?} -> {end:?}"
        );
    }

    /// A circular road of radius `radius` that starts at the origin heading
    /// along +X, with flat ground under it.
    fn ring_collision(radius: f64) -> PreviewCollisionWorld {
        let step = 4.0 / radius;
        let rows: Vec<racer_sim::TrackEdgeRow> = (0..(std::f64::consts::TAU / step) as usize)
            .map(|i| {
                let a = i as f64 * step;
                racer_sim::TrackEdgeRow {
                    x: radius * a.sin(),
                    y: radius - radius * a.cos(),
                    z: 0.0,
                    forward: Vec2::new(a.cos(), a.sin()),
                    right: Vec2::new(a.sin(), -a.cos()),
                    left_distance: 8.0,
                    right_distance: 8.0,
                    block: 0,
                }
            })
            .collect();
        let reach = radius * 2.0 + 100.0;
        PreviewCollisionWorld {
            ground: vec![
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-reach, -reach, 0.0],
                        [reach, -reach, 0.0],
                        [reach, reach, 0.0],
                    ],
                },
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-reach, -reach, 0.0],
                        [reach, reach, 0.0],
                        [-reach, reach, 0.0],
                    ],
                },
            ],
            walls: Vec::new(),
            vehicle_radius: 0.25,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: rows,
        }
    }

    #[test]
    fn imported_circuit_races_laps_with_gates_grid_and_ranked_opponents() {
        stub_imported_models();
        let mut client = LocalRaceClient::new(60);
        client
            .use_open_preview_with_collision(ring_collision(250.0))
            .unwrap();
        assert!(client.imported_course_available());
        // Free driving until laps are chosen: no gates, nothing to rank.
        assert_eq!(client.imported_laps(), 0);
        assert!(client.world.track.checkpoints.is_empty());
        assert!(client.set_imported_laps(2));
        assert_eq!(client.imported_laps(), 2);
        assert!(client.world.track.checkpoints.len() >= 3);
        client.configure_opponents(3, 1, true);
        let start = client.opponent_snapshots();
        assert_eq!(start.len(), 3);
        // Computer cars start ahead of the player on the grid and are ranked with them.
        let human = client.snapshot();
        assert!(start.iter().all(|o| o.x > human.x));
        assert_eq!(human.race_position, 4);
        assert_eq!(human.competitors, 4);
        for _ in 0..(60 * 20) {
            client.tick();
        }
        let after = client.opponent_snapshots();
        assert!(
            after.iter().all(|o| o.checkpoint > 1),
            "computer cars pass gates in order: {after:?}"
        );
        // Back to free driving: no gates, no laps.
        assert!(!client.set_imported_laps(0));
        assert_eq!(client.imported_laps(), 0);
        assert!(client.world.track.checkpoints.is_empty());
    }

    #[test]
    fn imported_open_preview_without_road_rows_has_no_opponents() {
        stub_imported_models();
        let mut client = LocalRaceClient::new(60);
        client.use_open_preview();
        client.configure_opponents(3, 1, true);
        assert!(client.opponent_snapshots().is_empty());
    }

    #[test]
    fn imported_collision_restores_scene_metric_normal_for_scaled_rotated_ramp_and_contacts_base() {
        stub_imported_models();
        let mut scene = loaded_scene(1.0);
        scene.spawn_pose_mapping.anchor_scene_forward = Vec3::new(0.0, 1.0, 0.0);
        for vertex in &mut scene.scene.meshes[0].vertices[..6] {
            vertex.position.z = vertex.position.x * 0.3 + vertex.position.y * 0.2;
        }
        for vertex in &mut scene.scene.meshes[1].vertices {
            vertex.position.z += 0.4;
        }

        let collision = derive_preview_collision(&scene, 0.5).unwrap();
        assert_eq!(collision.units_per_sim_unit, 0.5);
        assert!((collision.vehicle_min_z - 0.4).abs() < 1.0e-6);
        let support = collision.validate_initial_support().unwrap();
        let expected = Vec3::new(-0.2, 0.3, 1.0).normalized();
        let actual = Vec3::new(
            support.normal[0] as f32,
            support.normal[1] as f32,
            support.normal[2] as f32,
        );
        assert!((actual - expected).length() < 1.0e-5);
        let scene_normal =
            preview_normal_to_scene(support.normal, std::f64::consts::FRAC_PI_2).unwrap();
        let expected_scene = Vec3::new(-0.3, -0.2, 1.0).normalized();
        assert!(
            (Vec3::new(
                scene_normal[0] as f32,
                scene_normal[1] as f32,
                scene_normal[2] as f32
            ) - expected_scene)
                .length()
                < 1.0e-5
        );

        let mut client = LocalRaceClient::new(60);
        client.use_open_preview_with_collision(collision).unwrap();
        let snapshot = client.snapshot();
        let expected_origin_z = support.height - 0.4 / f64::from(expected.z);
        assert!((snapshot.z - expected_origin_z).abs() < 1.0e-6);
        assert!((snapshot.ground_normal[2] - f64::from(expected.z)).abs() < 1.0e-6);
        let scene_normal =
            preview_normal_to_scene(snapshot.ground_normal, std::f64::consts::FRAC_PI_2).unwrap();
        let instance = MeshInstance {
            mesh_index: 1,
            translation: Vec3::new(0.0, 0.0, snapshot.z as f32),
            heading: std::f32::consts::FRAC_PI_2,
            ground_normal: Vec3::new(
                scene_normal[0] as f32,
                scene_normal[1] as f32,
                scene_normal[2] as f32,
            ),
            scale: 1.0,
        };
        for x in [-0.5, 0.5] {
            for y in [-0.5, 0.5] {
                let corner = instance.transform_point(Vec3::new(x, y, 0.4));
                let plane_distance = Vec3::new(
                    scene_normal[0] as f32,
                    scene_normal[1] as f32,
                    scene_normal[2] as f32,
                )
                .dot(corner - Vec3::ZERO);
                assert!(plane_distance.abs() < 2.0e-5);
            }
        }
    }

    #[test]
    fn local_client_preview_collision_can_be_activated_and_cleared_atomically() {
        stub_imported_models();
        let collision = derive_preview_collision(&loaded_scene(1.0), 1.0).unwrap();
        let mut client = LocalRaceClient::new(60);
        client.use_open_preview_with_collision(collision).unwrap();
        assert!(client.has_preview_ground());
        assert_eq!(client.snapshot().z, 0.0);
        client.use_open_preview();
        assert!(!client.has_preview_ground());
        client.set_input(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        let after = client.tick();
        assert!(after.x.is_finite() && after.y.is_finite());
    }

    #[test]
    fn local_client_drives_a_generated_project_track() {
        stub_imported_models();
        let track = load_project_track("coastline-run").unwrap();
        let mut client = LocalRaceClient::new(60);
        client.use_project_track(&track).unwrap();
        assert!(client.has_preview_ground());
        let start = client.snapshot();
        assert!(start.x.abs() < 1.0e-9 && start.y.abs() < 1.0e-9 && start.heading.abs() < 1.0e-9);
        client.set_input(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        let mut last = start;
        for _ in 0..300 {
            last = client.tick();
        }
        // the grid is behind the line and the road starts straight along +X
        assert!(last.x > 40.0, "x {}", last.x);
        assert!(last.y.abs() < 8.0, "y {}", last.y);
        assert!(last.speed > 15.0);
        client.reset();
        let reset = client.snapshot();
        assert!(reset.x.abs() < 1.0e-6 && reset.speed < 1.0e-6);
        let metadata = project_track_metadata(&track);
        assert_eq!(metadata["track_id"], "coastline-run");
        let map = &metadata["map"];
        let points = map["centerline"].as_array().unwrap().len();
        assert!((3..=240).contains(&points), "map points {points}");
        assert_eq!(map["left"].as_array().unwrap().len(), points);
        assert_eq!(map["right"].as_array().unwrap().len(), points);
        let circuit: serde_json::Value = serde_json::from_str(&circuit_map_json()).unwrap();
        assert_eq!(circuit["centerline"].as_array().unwrap().len(), 4);
        assert_eq!(metadata["spawn_slots"].as_array().unwrap().len(), 8);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&project_track_catalog_json())
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn computer_opponents_complete_a_lap_of_a_generated_track() {
        stub_imported_models();
        let track = load_project_track("night-harbour").unwrap();
        let mut client = LocalRaceClient::new(60);
        client.use_project_track(&track).unwrap();
        client.configure_opponents(3, 1, true);
        let start = client.opponent_snapshots();
        assert_eq!(start.len(), 3);
        // grounded on the relief of the track
        assert!(start.iter().all(|o| o.z > 1.0), "{start:?}");
        let mut lapped = false;
        for tick in 0..60 * 150 {
            client.tick();
            if tick % 60 == 0 {
                lapped = client.opponent_snapshots().iter().all(|o| o.laps >= 1);
                if lapped {
                    break;
                }
            }
        }
        let end = client.opponent_snapshots();
        assert!(lapped, "opponents did not finish a lap: {end:?}");
        assert!(
            end.iter()
                .all(|o| o.race_position >= 1 && o.race_position <= 4)
        );
    }

    #[test]
    fn the_chosen_demo_car_drives_the_local_car_and_survives_track_changes() {
        stub_imported_models();
        let tops: Vec<f64> = demo_cars()
            .cars()
            .iter()
            .map(|c| classic_config(&c.vehicle).unwrap().top_speed_mps)
            .collect();
        let mut client = LocalRaceClient::new(60);
        for (index, top) in tops.iter().enumerate() {
            client.set_project_car(index);
            let installed = client.world.classic_parameters().unwrap().top_speed_mps;
            assert_eq!(installed, *top, "car {index} on the circuit");
        }
        // The choice carries over to a generated track and back to the circuit.
        client.set_project_car(3);
        client
            .use_project_track(&load_project_track("red-mesa").unwrap())
            .unwrap();
        assert_eq!(
            client.world.classic_parameters().unwrap().top_speed_mps,
            tops[3]
        );
        client.use_project_circuit();
        assert_eq!(
            client.world.classic_parameters().unwrap().top_speed_mps,
            tops[3]
        );
        // An out-of-range choice falls back to the last catalogue car.
        client.set_project_car(99);
        assert_eq!(client.project_car(), tops.len() - 1);
    }

    #[test]
    fn opponents_drive_to_their_own_cars_limits_on_every_generated_track() {
        stub_imported_models();
        let cars = demo_cars();
        let tops: Vec<f64> = cars
            .cars()
            .iter()
            .map(|c| classic_config(&c.vehicle).unwrap().top_speed_mps)
            .collect();
        for id in ["coastline-run", "pine-ridge", "red-mesa", "night-harbour"] {
            let track = load_project_track(id).unwrap();
            let mut client = LocalRaceClient::new(60);
            client.set_project_car(1);
            client.use_project_track(&track).unwrap();
            client.configure_opponents(3, 1, false);
            let mut peak = [0.0f64; 3];
            let mut lapped = false;
            for tick in 0..60 * 220 {
                client.tick();
                for (p, o) in peak.iter_mut().zip(client.opponent_snapshots()) {
                    *p = p.max(o.speed);
                }
                if tick % 60 == 0 {
                    lapped = client.opponent_snapshots().iter().all(|o| o.laps >= 1);
                    if lapped {
                        break;
                    }
                }
            }
            assert!(lapped, "{id}: opponents did not finish a lap: {peak:?}");
            for (k, p) in peak.iter().enumerate() {
                let (car, _) = cars.opponent_pick((1, 0), k);
                // Never faster than the car can go, and quick enough to be
                // a race (above the old 36 m/s ceiling somewhere on the lap).
                assert!(
                    *p <= tops[car] + 1.0,
                    "{id}: opponent {k} {p} > {}",
                    tops[car]
                );
                assert!(*p > 36.0, "{id}: opponent {k} only reached {p}");
            }
        }
    }

    #[test]
    fn local_client_can_return_from_imported_open_preview_to_project_circuit() {
        stub_imported_models();
        let collision = derive_preview_collision(&loaded_scene(1.0), 1.0).unwrap();
        let mut client = LocalRaceClient::new(60);
        client.use_open_preview_with_collision(collision).unwrap();
        assert!(client.has_preview_ground());
        client.use_project_circuit();
        assert!(!client.has_preview_ground());
        let snapshot = client.tick();
        assert!(snapshot.x.is_finite() && snapshot.y.is_finite());
        assert!(snapshot.checkpoint <= 3);
    }
}

#[cfg(target_arch = "wasm32")]
fn preview_route_coverage_summary(
    loaded: &LoadedScene,
    collision: &PreviewCollisionWorld,
) -> serde_json::Value {
    match audit_imported_preview_route(loaded, collision) {
        Ok(report) => serde_json::json!({
            "status": "available",
            "classification": "geometric_preview_only",
            "sample_spacing_scene_units_max": 0.5,
            "sample_count": report.sample_count,
            "supported_count": report.supported_count,
            "unsupported_count": report.unsupported_count,
            "unsupported_runs": report.unsupported_runs,
            "seam_supported": report.seam_supported,
            "seam_matches_initial_support": report.seam_matches_initial_support,
            "ambiguous_sample_count": report.samples.iter().filter(|sample| sample.selection_ambiguous).count(),
            "multiple_plane_sample_count": report.samples.iter().filter(|sample| sample.alternate_planes > 0).count(),
            "candidate_checks": report.candidate_checks,
        }),
        Err(RouteAuditError::ReferencePathUnavailable) => serde_json::json!({
            "status": "unavailable",
            "reason": "saved scene has no bounded supported-profile reference path metadata"
        }),
        Err(error) => serde_json::json!({
            "status": "rejected",
            "reason": format!("bounded geometric route diagnostic rejected its inputs: {error:?}")
        }),
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_api {
    use super::*;
    use js_sys::{Array, Float32Array, Uint8Array};
    use racer_content::project_cars::{ProjectCars, project_cars};
    use racer_content::{
        SceneSourceIdentity, decode_scene_descriptor, descriptor_vehicle_paint_colour,
        vehicle_render_fingerprint,
    };
    use racer_protocol::{ClientMessage, ServerMessage};
    use racer_render::Vec3;
    use racer_render::wgpu_backend::WgpuRenderer;
    use racer_render::{
        MAX_RENDER_VEHICLE_PROFILES, PreviewCameraMode, VehicleMesh, VehicleProfileIdentity,
        synthetic_scene, synthetic_scene_with_vehicle, vehicle_source_scene,
        vehicle_source_scene_on_white_page,
    };
    use sha2::{Digest, Sha256};
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    /// The demo-car field currently installed in the project scene: profile 0
    /// is the local driver's car, the others are shared out among opponents.
    #[derive(Default)]
    struct ProjectField {
        kind: ProjectSceneKind,
        player: (usize, usize),
        opponents: usize,
        /// Car and paint indices of the registered profiles (the first is the
        /// scene's own vehicle template); `profiles` runs parallel to it.
        picks: Vec<(usize, usize)>,
        profiles: Vec<VehicleProfileIdentity>,
        /// Track fingerprint the profiles are bound to and the texture page
        /// size of the scene (cars register on a matching white page).
        fingerprint: String,
        page: (u32, u32),
        /// Picks drawn by the last frame, for diagnostics.
        last_render: Vec<(usize, usize)>,
    }

    /// Which project scene the demo-car field is installed in.
    #[derive(Clone, Debug, Default, PartialEq)]
    enum ProjectSceneKind {
        #[default]
        Circuit,
        Track(String),
    }

    thread_local! {
        static PROJECT_FIELD: RefCell<ProjectField> = RefCell::new(ProjectField::default());
        /// The last generated track, whole, so the scene can be installed
        /// again with other demo cars without generating it twice.
        static TRACK_CACHE: RefCell<Option<(String, RcpTrackScene)>> = const { RefCell::new(None) };
        static GPU: RefCell<Option<WgpuRenderer>> = const { RefCell::new(None) };
        static PENDING_PREVIEW_COLLISION: RefCell<Option<PreviewCollisionWorld>> = const { RefCell::new(None) };
        static PENDING_PROJECT_TRACK: RefCell<Option<RcpTrackScene>> = const { RefCell::new(None) };
    }

    /// Renderer identity fingerprint for a painted variant; `colour < 0`
    /// returns the plain fingerprint.
    #[wasm_bindgen]
    pub fn vehicle_render_fingerprint_for_paint(
        vehicle_source_fingerprint: &str,
        colour: i32,
    ) -> String {
        vehicle_render_fingerprint(
            vehicle_source_fingerprint,
            u8::try_from(colour).ok().filter(|c| *c <= 87),
        )
    }

    fn decode_rgba_layers(rgba_layers: Array) -> Result<Vec<Vec<u8>>, JsValue> {
        const MAX_TEXTURE_LAYERS: u32 = 256;
        const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
        if rgba_layers.length() == 0 || rgba_layers.length() > MAX_TEXTURE_LAYERS {
            return Err(JsValue::from_str(
                "scene texture layer count is outside supported bounds",
            ));
        }
        let mut layers = Vec::with_capacity(rgba_layers.length() as usize);
        let mut total_bytes = 0_usize;
        for value in rgba_layers.iter() {
            if !value.is_instance_of::<Uint8Array>() {
                return Err(JsValue::from_str(
                    "scene texture layers must be Uint8Array RGBA buffers",
                ));
            }
            let bytes = Uint8Array::new(&value);
            total_bytes = total_bytes
                .checked_add(bytes.length() as usize)
                .filter(|total| *total <= MAX_TEXTURE_BYTES)
                .ok_or_else(|| {
                    JsValue::from_str("scene decoded texture data exceeds the byte limit")
                })?;
            layers.push(bytes.to_vec());
        }
        Ok(layers)
    }

    fn matches_expected_identity(loaded: &LoadedScene, expected: &SceneSourceIdentity) -> bool {
        loaded.source_identity.as_ref() == Some(expected)
    }

    #[wasm_bindgen]
    pub fn transform_preview_normal_to_scene(
        normal_x: f64,
        normal_y: f64,
        normal_z: f64,
        anchor_heading: f64,
    ) -> Result<Float32Array, JsValue> {
        let normal = preview_normal_to_scene([normal_x, normal_y, normal_z], anchor_heading)
            .ok_or_else(|| JsValue::from_str("preview support normal is invalid"))?;
        Ok(Float32Array::from(
            &[normal[0] as f32, normal[1] as f32, normal[2] as f32][..],
        ))
    }

    /// Verify a bounded private-asset response against its manifest digest.
    /// This works on the owner's plain-HTTP LAN origin, where WebCrypto's
    /// `crypto.subtle` API is unavailable because the origin is not secure.
    #[wasm_bindgen]
    pub fn verify_private_asset_sha256(bytes: &[u8], expected_hex: &str) -> bool {
        if expected_hex.len() != 64 || !expected_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        let digest = Sha256::digest(bytes);
        let mut difference = 0_u8;
        for (actual, expected) in digest.iter().zip(expected_hex.as_bytes().chunks_exact(2)) {
            let high = hex_nibble(expected[0]);
            let low = hex_nibble(expected[1]);
            difference |= actual ^ ((high << 4) | low);
        }
        difference == 0
    }

    #[wasm_bindgen]
    pub fn sha256_hex(bytes: &[u8]) -> String {
        hex_digest(&Sha256::digest(bytes))
    }

    fn hex_nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => 0,
        }
    }

    fn hex_digest(digest: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(digest.len() * 2);
        for &byte in digest {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }

    /// Encode a JSON representation of a protocol client message as postcard bytes.
    #[wasm_bindgen]
    pub fn encode_client_message(json: &str) -> Result<Vec<u8>, JsValue> {
        let message: ClientMessage =
            serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        racer_protocol::encode_client(&message).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Decode postcard server bytes to JSON for the browser's UI layer.
    #[wasm_bindgen]
    pub fn decode_server_message(bytes: &[u8]) -> Result<String, JsValue> {
        let message: ServerMessage =
            racer_protocol::decode_server(bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_json::to_string(&message).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen]
    pub async fn init_wgpu(canvas_id: &str) -> Result<(), JsValue> {
        init_renderer(canvas_id, false).await
    }

    #[wasm_bindgen]
    pub async fn init_webgl(canvas_id: &str) -> Result<(), JsValue> {
        init_renderer(canvas_id, true).await
    }

    async fn init_renderer(canvas_id: &str, force_webgl: bool) -> Result<(), JsValue> {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("Browser document is unavailable"))?;
        let canvas = document
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str("Racing canvas was not found"))?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| JsValue::from_str("Racing element is not a canvas"))?;
        let renderer = if force_webgl {
            WgpuRenderer::new_webgl(canvas).await
        } else {
            WgpuRenderer::new(canvas).await
        }
        .map_err(|e| JsValue::from_str(&e))?;
        GPU.with(|slot| *slot.borrow_mut() = Some(renderer));
        Ok(())
    }

    #[wasm_bindgen]
    pub fn gpu_ready() -> bool {
        GPU.with(|slot| slot.borrow().is_some())
    }

    #[wasm_bindgen]
    pub fn gpu_backend() -> String {
        GPU.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|renderer| renderer.backend_name().to_string())
                .unwrap_or_default()
        })
    }

    /// Restore the project-owned generic circuit after an owner-local scene.
    /// Owner cache records are kept by the browser; this only replaces the
    /// renderer scene so generic network rooms cannot display owner geometry.
    #[wasm_bindgen]
    pub fn restore_project_scene() -> Result<(), JsValue> {
        let (player, opponents) = PROJECT_FIELD.with(|field| {
            let field = field.borrow();
            (field.player, field.opponents)
        });
        if install_project_field(player, opponents).is_err() {
            // The built-in box car keeps the scene usable if the demo cars
            // cannot be registered (for example a tight GPU budget).
            let scene =
                synthetic_scene(&TrackRuntime::synthetic_circuit(), Vec2::new(0.0, 0.0), 0.0);
            GPU.with(|slot| {
                let mut slot = slot.borrow_mut();
                let renderer = slot
                    .as_mut()
                    .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
                renderer
                    .set_scene(scene, 2, &[0, 1])
                    .map_err(|error| JsValue::from_str(&error))
            })?;
            PROJECT_FIELD.with(|field| field.borrow_mut().profiles.clear());
        }
        PENDING_PREVIEW_COLLISION.with(|slot| *slot.borrow_mut() = None);
        Ok(())
    }

    /// Project-owned demo cars as JSON: `{"cars":[{"id","name","paints":[{"id","body"}]}]}`.
    #[wasm_bindgen]
    pub fn project_car_catalog() -> String {
        project_cars().catalog_json()
    }

    /// Install the project circuit with the chosen demo car and paint for the
    /// local driver and a field of up to seven different opponent cars.
    #[wasm_bindgen]
    pub fn install_project_scene(
        car_id: &str,
        paint_id: &str,
        opponents: u32,
    ) -> Result<(), JsValue> {
        let player = project_cars()
            .lookup(car_id, paint_id)
            .ok_or_else(|| JsValue::from_str("unknown project car or paint"))?;
        install_project_field(player, opponents.min(7) as usize)?;
        PENDING_PREVIEW_COLLISION.with(|slot| *slot.borrow_mut() = None);
        Ok(())
    }

    /// Profile identifiers of the installed demo-car field, comma separated,
    /// the local driver's car first (empty when the demo cars are not active).
    #[wasm_bindgen]
    pub fn project_field_ids() -> String {
        PROJECT_FIELD.with(|field| {
            field
                .borrow()
                .profiles
                .iter()
                .map(|profile| profile.vehicle_profile_id.clone())
                .collect::<Vec<_>>()
                .join(",")
        })
    }

    /// Cars drawn by the last frame as `car:paint` ids, comma separated.
    #[wasm_bindgen]
    pub fn project_render_picks() -> String {
        let cars = project_cars();
        PROJECT_FIELD.with(|field| {
            field
                .borrow()
                .last_render
                .iter()
                .map(|&(car, paint)| {
                    format!(
                        "{}:{}",
                        cars.cars()[car].id,
                        cars.cars()[car].paints[paint].id
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
    }

    /// Cars of a field as flat `car, paint` index pairs (catalogue order):
    /// the driver first, then `opponents` different computer cars. The same
    /// rule the server uses to hand out opponent cars.
    #[wasm_bindgen]
    pub fn project_roster(car_index: u32, paint_index: u32, opponents: u32) -> Uint8Array {
        let cars = project_cars();
        let car = (car_index as usize).min(cars.cars().len() - 1);
        let paint = (paint_index as usize).min(cars.cars()[car].paints.len() - 1);
        let flat: Vec<u8> = cars
            .roster((car, paint), opponents.min(7) as usize)
            .into_iter()
            .flat_map(|(car, paint)| [car as u8, paint as u8])
            .collect();
        Uint8Array::from(flat.as_slice())
    }

    fn project_track_fingerprint() -> String {
        sha256_hex_of(b"project-circuit")
    }

    fn sha256_hex_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn pick_profile(
        cars: &ProjectCars,
        (car, paint): (usize, usize),
    ) -> Result<(VehicleProfileIdentity, VehicleMesh), JsValue> {
        let fingerprint = cars
            .profile_fingerprint(car, paint)
            .map_err(|e| JsValue::from_str(&e))?;
        let identity = VehicleProfileIdentity::new(&cars.profile_id(car, paint), &fingerprint)
            .map_err(|e| JsValue::from_str(&e))?;
        let mesh = cars
            .vehicle_mesh(car, paint)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok((identity, mesh))
    }

    /// Car and paint indices of a request; an empty car id means the first car.
    fn lookup_pick(
        cars: &ProjectCars,
        car_id: &str,
        paint_id: &str,
    ) -> Result<(usize, usize), JsValue> {
        if car_id.is_empty() {
            return Ok((0, 0));
        }
        cars.lookup(car_id, paint_id)
            .ok_or_else(|| JsValue::from_str("unknown project car or paint"))
    }

    fn cached_track(id: &str) -> Result<RcpTrackScene, JsValue> {
        if let Some(track) = TRACK_CACHE.with(|slot| {
            slot.borrow()
                .as_ref()
                .filter(|(cached, _)| cached == id)
                .map(|(_, track)| track.clone())
        }) {
            return Ok(track);
        }
        let track = load_project_track(id).map_err(|e| JsValue::from_str(&e))?;
        TRACK_CACHE.with(|slot| *slot.borrow_mut() = Some((id.to_owned(), track.clone())));
        Ok(track)
    }

    /// Install a scene (the circuit or a generated track) with the given
    /// cars registered as render profiles; `picks[0]` is the scene's own
    /// vehicle. Returns the track without its scene for a track install.
    fn install_field(
        kind: ProjectSceneKind,
        picks: Vec<(usize, usize)>,
        opponents: usize,
    ) -> Result<Option<RcpTrackScene>, JsValue> {
        let cars = project_cars();
        if picks.is_empty() || picks.len() > MAX_RENDER_VEHICLE_PROFILES {
            return Err(JsValue::from_str(
                "demo car field must hold one to eight cars",
            ));
        }
        let mut meshes = Vec::with_capacity(picks.len());
        let mut identities = Vec::with_capacity(picks.len());
        for &pick in &picks {
            let (identity, mesh) = pick_profile(cars, pick)?;
            identities.push(identity);
            meshes.push(mesh);
        }
        let (scene, vehicle_instance, mesh_indices, fingerprint, track, fog) = match &kind {
            ProjectSceneKind::Circuit => (
                synthetic_scene_with_vehicle(&TrackRuntime::synthetic_circuit(), &meshes[0]),
                2,
                vec![0, 1],
                project_track_fingerprint(),
                None,
                None,
            ),
            ProjectSceneKind::Track(id) => {
                let mut track = cached_track(id)?;
                track
                    .set_vehicle(&meshes[0])
                    .map_err(|e| JsValue::from_str(&e))?;
                let scene = std::mem::take(&mut track.scene);
                let instance = track.vehicle_instance_index;
                let indices = track.track_mesh_indices.clone();
                let fog = track.fog;
                (
                    scene,
                    instance,
                    indices,
                    sha256_hex_of(format!("project-track:{id}").as_bytes()),
                    Some(track),
                    fog,
                )
            }
        };
        let page = (scene.textures.width, scene.textures.height);
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .set_scene_for_vehicle_profile(
                    scene,
                    vehicle_instance,
                    &mesh_indices,
                    &fingerprint,
                    identities[0].clone(),
                )
                .map_err(|e| JsValue::from_str(&e))?;
            renderer.set_fog(fog);
            for (identity, mesh) in identities.iter().zip(&meshes).skip(1) {
                renderer
                    .register_vehicle_profile(
                        &fingerprint,
                        identity.clone(),
                        &pick_source_scene(&kind, mesh, page),
                        0,
                    )
                    .map_err(|e| JsValue::from_str(&e))?;
            }
            Ok::<(), JsValue>(())
        })?;
        PROJECT_FIELD.with(|state| {
            *state.borrow_mut() = ProjectField {
                kind,
                player: picks[0],
                opponents,
                picks,
                profiles: identities,
                fingerprint,
                page,
                last_render: Vec::new(),
            };
        });
        Ok(track)
    }

    fn pick_source_scene(
        kind: &ProjectSceneKind,
        mesh: &VehicleMesh,
        page: (u32, u32),
    ) -> racer_render::Scene3d {
        match kind {
            ProjectSceneKind::Circuit => vehicle_source_scene(mesh),
            ProjectSceneKind::Track(_) => vehicle_source_scene_on_white_page(mesh, page.0, page.1),
        }
    }

    fn install_project_field(player: (usize, usize), opponents: usize) -> Result<(), JsValue> {
        let field = project_cars().roster(player, opponents);
        install_field(ProjectSceneKind::Circuit, field, opponents)?;
        PENDING_PREVIEW_COLLISION.with(|slot| *slot.borrow_mut() = None);
        PENDING_PROJECT_TRACK.with(|slot| *slot.borrow_mut() = None);
        Ok(())
    }

    /// Decode flat `car, paint` index pairs; an entry that names no catalogue
    /// car (for example 255) is `None`.
    fn decode_picks(bytes: &[u8]) -> Vec<Option<(usize, usize)>> {
        let cars = project_cars();
        bytes
            .chunks_exact(2)
            .map(|pair| {
                let (car, paint) = (usize::from(pair[0]), usize::from(pair[1]));
                (car < cars.cars().len() && paint < cars.cars()[car].paints.len())
                    .then_some((car, paint))
            })
            .collect()
    }

    /// Render profiles for the requested cars. Cars not yet registered are
    /// added to the scene; when that would pass the profile limit the scene is
    /// installed again with exactly the requested cars. `None` when the demo
    /// cars are not active or a registration failed.
    fn resolve_profiles(
        requested: &[Option<(usize, usize)>],
    ) -> Option<Vec<VehicleProfileIdentity>> {
        let (default, mut known, kind, fingerprint, page, opponents) =
            PROJECT_FIELD.with(|field| {
                let field = field.borrow();
                (
                    field.picks.first().copied(),
                    field.picks.clone(),
                    field.kind.clone(),
                    field.fingerprint.clone(),
                    field.page,
                    field.opponents,
                )
            });
        let default = default?;
        let wanted: Vec<(usize, usize)> = requested.iter().map(|p| p.unwrap_or(default)).collect();
        let mut missing: Vec<(usize, usize)> = Vec::new();
        for pick in &wanted {
            if !known.contains(pick) && !missing.contains(pick) {
                missing.push(*pick);
            }
        }
        if !missing.is_empty() {
            if known.len() + missing.len() <= MAX_RENDER_VEHICLE_PROFILES {
                let cars = project_cars();
                for pick in missing {
                    let (identity, mesh) = pick_profile(cars, pick).ok()?;
                    GPU.with(|slot| {
                        slot.borrow_mut()
                            .as_mut()?
                            .register_vehicle_profile(
                                &fingerprint,
                                identity.clone(),
                                &pick_source_scene(&kind, &mesh, page),
                                0,
                            )
                            .ok()
                    })?;
                    known.push(pick);
                    PROJECT_FIELD.with(|field| {
                        let mut field = field.borrow_mut();
                        field.picks.push(pick);
                        field.profiles.push(identity);
                    });
                }
            } else {
                let mut next: Vec<(usize, usize)> = Vec::new();
                for pick in &wanted {
                    if !next.contains(pick) {
                        next.push(*pick);
                    }
                }
                install_field(kind, next, opponents).ok()?;
                known = PROJECT_FIELD.with(|field| field.borrow().picks.clone());
            }
        }
        let profiles = PROJECT_FIELD.with(|field| field.borrow().profiles.clone());
        PROJECT_FIELD.with(|field| field.borrow_mut().last_render = wanted.clone());
        wanted
            .iter()
            .map(|pick| {
                known
                    .iter()
                    .position(|k| k == pick)
                    .map(|i| profiles[i].clone())
            })
            .collect()
    }

    /// Mini-map outline of the built-in test circuit as JSON.
    #[wasm_bindgen]
    pub fn project_circuit_map_json() -> String {
        circuit_map_json()
    }

    /// JSON catalog of the generated project tracks.
    #[wasm_bindgen]
    pub fn project_tracks_json() -> String {
        project_track_catalog_json()
    }

    /// Generate a project track, install its scene in the renderer with the
    /// chosen demo car for the driver and `opponents` different computer cars,
    /// and keep its simulation inputs for `BrowserRace::use_project_track`.
    /// Returns the pose-mapping metadata as JSON.
    #[wasm_bindgen]
    pub fn install_project_track(
        id: &str,
        car_id: &str,
        paint_id: &str,
        opponents: u32,
    ) -> Result<String, JsValue> {
        let cars = project_cars();
        let player = lookup_pick(cars, car_id, paint_id)?;
        let opponents = opponents.min(7) as usize;
        let track = install_field(
            ProjectSceneKind::Track(id.to_owned()),
            cars.roster(player, opponents),
            opponents,
        )?
        .ok_or_else(|| JsValue::from_str("track install returned no track"))?;
        let metadata = project_track_metadata(&track).to_string();
        PENDING_PREVIEW_COLLISION.with(|slot| *slot.borrow_mut() = None);
        PENDING_PROJECT_TRACK.with(|slot| *slot.borrow_mut() = Some(track));
        Ok(metadata)
    }

    /// Install the generated track that is already active again with other
    /// demo cars (the race itself is not touched).
    #[wasm_bindgen]
    pub fn reinstall_project_track_cars(
        car_id: &str,
        paint_id: &str,
        opponents: u32,
    ) -> Result<(), JsValue> {
        let cars = project_cars();
        let player = lookup_pick(cars, car_id, paint_id)?;
        let kind = PROJECT_FIELD.with(|field| field.borrow().kind.clone());
        if !matches!(kind, ProjectSceneKind::Track(_)) {
            return Err(JsValue::from_str("no generated track scene is installed"));
        }
        let opponents = opponents.min(7) as usize;
        install_field(kind, cars.roster(player, opponents), opponents)?;
        PENDING_PREVIEW_COLLISION.with(|slot| *slot.borrow_mut() = None);
        Ok(())
    }

    /// Install a large synthetic scene for renderer throughput measurement.
    #[wasm_bindgen]
    pub fn install_perf_scene(prop_instances: u32, track_triangles: u32) -> Result<(), JsValue> {
        if prop_instances > 20_000 || track_triangles > 400_000 {
            return Err(JsValue::from_str("performance scene exceeds its bounds"));
        }
        let scene = racer_render::perf_fixture_scene(
            &TrackRuntime::synthetic_circuit(),
            prop_instances,
            track_triangles,
        );
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .set_scene(scene, 2, &[0, 1])
                .map_err(|error| JsValue::from_str(&error))
        })
    }

    #[wasm_bindgen]
    pub fn render_wgpu(
        x: f64,
        y: f64,
        heading: f64,
        speed: f64,
        checkpoint: usize,
        width: u32,
        height: u32,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .render(racer_sim::Vec2::new(x, y), heading, speed, checkpoint)
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Install a validated scene descriptor and its already-decoded RGBA8
    /// texture-array layers. The caller fetches only opaque asset IDs from the
    /// private provider; filesystem paths never cross this boundary.
    #[wasm_bindgen]
    pub fn install_scene_descriptor(
        json: &str,
        rgba_layers: Array,
        units_per_sim_unit: f64,
        enable_preview_collision: bool,
    ) -> Result<String, JsValue> {
        install_scene_descriptor_inner(
            json,
            rgba_layers,
            units_per_sim_unit,
            enable_preview_collision,
            None,
        )
    }

    /// Install a scene for multiplayer only when its local descriptor binds
    /// the exact selected track and car identities. This clears all old car
    /// templates and installs the active profile atomically with the scene.
    #[wasm_bindgen]
    pub fn install_race_scene_descriptor(
        track_profile_id: &str,
        track_source_fingerprint: &str,
        vehicle_profile_id: &str,
        vehicle_source_fingerprint: &str,
        json: &str,
        rgba_layers: Array,
        units_per_sim_unit: f64,
        enable_preview_collision: bool,
    ) -> Result<String, JsValue> {
        install_scene_descriptor_inner(
            json,
            rgba_layers,
            units_per_sim_unit,
            enable_preview_collision,
            Some(SceneSourceIdentity {
                track_profile_id: track_profile_id.to_owned(),
                track_source_fingerprint: track_source_fingerprint.to_owned(),
                vehicle_profile_id: vehicle_profile_id.to_owned(),
                vehicle_source_fingerprint: vehicle_source_fingerprint.to_owned(),
            }),
        )
    }

    fn install_scene_descriptor_inner(
        json: &str,
        rgba_layers: Array,
        units_per_sim_unit: f64,
        enable_preview_collision: bool,
        expected_identity: Option<SceneSourceIdentity>,
    ) -> Result<String, JsValue> {
        let layers = decode_rgba_layers(rgba_layers)?;
        let loaded =
            decode_scene_descriptor(json, layers).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let paint_colour =
            descriptor_vehicle_paint_colour(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        if expected_identity
            .as_ref()
            .is_some_and(|expected| !matches_expected_identity(&loaded, expected))
        {
            return Err(JsValue::from_str(
                "scene source identity does not match the selected multiplayer profiles",
            ));
        }
        let preview_collision = if enable_preview_collision {
            Some(
                derive_preview_collision(&loaded, units_per_sim_unit)
                    .map_err(|error| JsValue::from_str(&error))?,
            )
        } else {
            None
        };
        let preview_route_coverage = match preview_collision.as_ref() {
            Some(collision) => preview_route_coverage_summary(&loaded, collision),
            None => serde_json::json!({ "status": "not_run" }),
        };
        let metadata = serde_json::json!({
            "texture_asset_ids": loaded.texture_asset_ids,
            "track_mesh_indices": loaded.track_mesh_indices.clone(),
            "track_bounds": {
                "min": [loaded.track_bounds.min.x, loaded.track_bounds.min.y, loaded.track_bounds.min.z],
                "max": [loaded.track_bounds.max.x, loaded.track_bounds.max.y, loaded.track_bounds.max.z]
            },
            "source_to_scene": {
                "matrix": loaded.source_to_scene.matrix,
                "source_scale_m": loaded.source_to_scene.source_scale_m
            },
            "spawn_pose_mapping": {
                "anchor_source_position_m": [
                    loaded.spawn_pose_mapping.anchor_source_position_m.x,
                    loaded.spawn_pose_mapping.anchor_source_position_m.y,
                    loaded.spawn_pose_mapping.anchor_source_position_m.z
                ],
                "anchor_scene_position_m": [
                    loaded.spawn_pose_mapping.anchor_scene_position_m.x,
                    loaded.spawn_pose_mapping.anchor_scene_position_m.y,
                    loaded.spawn_pose_mapping.anchor_scene_position_m.z
                ],
                "anchor_source_forward": [
                    loaded.spawn_pose_mapping.anchor_source_forward.x,
                    loaded.spawn_pose_mapping.anchor_source_forward.y,
                    loaded.spawn_pose_mapping.anchor_source_forward.z
                ],
                "anchor_scene_forward": [
                    loaded.spawn_pose_mapping.anchor_scene_forward.x,
                    loaded.spawn_pose_mapping.anchor_scene_forward.y,
                    loaded.spawn_pose_mapping.anchor_scene_forward.z
                ],
                "confidence": loaded.spawn_pose_mapping.confidence
            },
            "preview_collision": preview_collision.as_ref().map(|collision| serde_json::json!({
                "ground_triangles": collision.ground.len(),
                "wall_segments": collision.walls.len(),
                "vehicle_radius": collision.vehicle_radius
            })),
            "preview_route_coverage": preview_route_coverage
        });
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            match expected_identity.as_ref() {
                Some(identity) => {
                    let vehicle = VehicleProfileIdentity::new(
                        &identity.vehicle_profile_id,
                        &vehicle_render_fingerprint(
                            &identity.vehicle_source_fingerprint,
                            paint_colour,
                        ),
                    )
                    .map_err(|e| JsValue::from_str(&e))?;
                    renderer.set_scene_for_vehicle_profile(
                        loaded.scene,
                        loaded.vehicle_instance_index,
                        &loaded.track_mesh_indices,
                        &identity.track_source_fingerprint,
                        vehicle,
                    )
                }
                None => renderer.set_scene(
                    loaded.scene,
                    loaded.vehicle_instance_index,
                    &loaded.track_mesh_indices,
                ),
            }
            .map_err(|e| JsValue::from_str(&e))
        })?;
        PENDING_PREVIEW_COLLISION.with(|slot| {
            *slot.borrow_mut() = preview_collision;
        });
        serde_json::to_string(&metadata).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Register a car scene imported from local data. Only the
    /// descriptor-designated vehicle mesh and its referenced materials/pages
    /// are copied into the active renderer scene.
    #[wasm_bindgen]
    pub fn register_wgpu_vehicle_profile(
        track_profile_id: &str,
        track_source_fingerprint: &str,
        vehicle_profile_id: &str,
        vehicle_source_fingerprint: &str,
        json: &str,
        rgba_layers: Array,
    ) -> Result<(), JsValue> {
        let loaded = decode_scene_descriptor(json, decode_rgba_layers(rgba_layers)?)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let expected = SceneSourceIdentity {
            track_profile_id: track_profile_id.to_owned(),
            track_source_fingerprint: track_source_fingerprint.to_owned(),
            vehicle_profile_id: vehicle_profile_id.to_owned(),
            vehicle_source_fingerprint: vehicle_source_fingerprint.to_owned(),
        };
        if !matches_expected_identity(&loaded, &expected) {
            return Err(JsValue::from_str(
                "vehicle scene source identity does not match the selected multiplayer profiles",
            ));
        }
        let paint_colour =
            descriptor_vehicle_paint_colour(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let vehicle = VehicleProfileIdentity::new(
            vehicle_profile_id,
            &vehicle_render_fingerprint(vehicle_source_fingerprint, paint_colour),
        )
        .map_err(|e| JsValue::from_str(&e))?;
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .register_vehicle_profile(
                    track_source_fingerprint,
                    vehicle,
                    &loaded.scene,
                    loaded.vehicle_instance_index,
                )
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render a server-authoritative roster using each locally registered car
    /// template. Poses are x,y,z,heading,ground-normal-x/y/z tuples.
    #[wasm_bindgen]
    pub fn render_wgpu_world_vehicles_with_profiles(
        poses: Float32Array,
        profile_pairs_json: &str,
        roster_signature: &str,
        local_vehicle_index: i32,
        camera_x: f32,
        camera_y: f32,
        camera_z: f32,
        camera_heading: f32,
        width: u32,
        height: u32,
    ) -> Result<f32, JsValue> {
        const MAX_POSES: usize = racer_render::MAX_RENDER_VEHICLES;
        if profile_pairs_json.len() > 2048 {
            return Err(JsValue::from_str(
                "render profile list exceeds its byte bound",
            ));
        }
        let profile_values: serde_json::Value = serde_json::from_str(profile_pairs_json)
            .map_err(|e| JsValue::from_str(&format!("invalid render profile list: {e}")))?;
        let entries = profile_values
            .as_array()
            .ok_or_else(|| JsValue::from_str("render profile list must be a JSON array"))?;
        if entries.len() > MAX_POSES || poses.length() as usize != entries.len() * 7 {
            return Err(JsValue::from_str(
                "render pose and profile lists must contain matching seven-float vehicle entries",
            ));
        }
        if local_vehicle_index < -1
            || (local_vehicle_index >= 0 && local_vehicle_index as usize >= entries.len())
        {
            return Err(JsValue::from_str(
                "local render vehicle index is outside the pose list",
            ));
        }
        const MAX_COORDINATE: f32 = 1_000_000.0;
        if [camera_x, camera_y, camera_z, camera_heading]
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_COORDINATE)
        {
            return Err(JsValue::from_str(
                "render camera pose is outside supported bounds",
            ));
        }
        let mut identities = Vec::with_capacity(entries.len());
        for entry in entries {
            let profile_id = entry
                .get("vehicle_profile_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| JsValue::from_str("render profile is missing vehicle_profile_id"))?;
            let fingerprint = entry
                .get("vehicle_source_fingerprint")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    JsValue::from_str("render profile is missing vehicle_source_fingerprint")
                })?;
            // Optional painted-variant colour (absent: the unpainted identity).
            let paint_colour = entry
                .get("paint_colour")
                .and_then(serde_json::Value::as_u64)
                .and_then(|colour| u8::try_from(colour).ok())
                .filter(|colour| *colour <= 87);
            identities.push(
                VehicleProfileIdentity::new(
                    profile_id,
                    &vehicle_render_fingerprint(fingerprint, paint_colour),
                )
                .map_err(|e| JsValue::from_str(&e))?,
            );
        }
        let mut values = vec![0.0_f32; poses.length() as usize];
        poses.copy_to(&mut values);
        let parsed_poses =
            parse_profiled_render_poses(&values, &identities).map_err(JsValue::from_str)?;
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .set_vehicle_roster(roster_signature)
                .map_err(|e| JsValue::from_str(&e))?;
            renderer
                .render_world_vehicles_with_profiles(
                    Vec3::new(camera_x, camera_y, camera_z),
                    camera_heading,
                    &parsed_poses,
                    &identities,
                    (local_vehicle_index >= 0).then_some(local_vehicle_index as usize),
                )
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render the loaded scene at a pose already converted into its declared
    /// render coordinate frame. Network and simulation code own that mapping.
    #[wasm_bindgen]
    pub fn render_wgpu_world(
        x: f32,
        y: f32,
        z: f32,
        heading: f32,
        width: u32,
        height: u32,
    ) -> Result<f32, JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .set_vehicle_roster("legacy-single")
                .map_err(|e| JsValue::from_str(&e))?;
            renderer
                .render_world_pose(Vec3::new(x, y, z), heading)
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render several vehicles on the project-owned circuit scene. Poses are
    /// x,y,heading triples in simulation coordinates (the project scene uses
    /// them unchanged); the camera follows `local_vehicle_index`.
    #[wasm_bindgen]
    pub fn render_wgpu_project_vehicles(
        poses: Float32Array,
        roster_signature: &str,
        local_vehicle_index: i32,
        width: u32,
        height: u32,
        car_picks: Uint8Array,
    ) -> Result<f32, JsValue> {
        const MAX_POSES: usize = racer_render::MAX_RENDER_VEHICLES;
        if poses.length() as usize > MAX_POSES * 3 || poses.length() % 3 != 0 || poses.length() == 0
        {
            return Err(JsValue::from_str(
                "project pose array must hold one to eight x,y,heading triples",
            ));
        }
        if local_vehicle_index < 0 || local_vehicle_index as usize >= poses.length() as usize / 3 {
            return Err(JsValue::from_str(
                "local render vehicle index is outside the pose list",
            ));
        }
        let mut values = vec![0.0_f32; poses.length() as usize];
        poses.copy_to(&mut values);
        if values.iter().any(|value| !value.is_finite()) {
            return Err(JsValue::from_str("render pose contains a non-finite value"));
        }
        let vehicles = values
            .chunks_exact(3)
            .map(|pose| (Vec3::new(pose[0], pose[1], 0.0), pose[2]))
            .collect::<Vec<_>>();
        let local = local_vehicle_index as usize;
        let (camera, heading) = vehicles[local];
        // Demo cars: the page names the car of every vehicle; without names
        // the local driver uses profile 0 and every other vehicle the next
        // opponent profile in turn.
        let named = decode_picks(&car_picks.to_vec());
        let assigned = if named.len() == vehicles.len() {
            resolve_profiles(&named)
        } else {
            None
        };
        let profiles = PROJECT_FIELD.with(|field| field.borrow().profiles.clone());
        let assigned = assigned.or_else(|| {
            (!profiles.is_empty()).then(|| {
                let mut next = 0usize;
                (0..vehicles.len())
                    .map(|index| {
                        if index == local || profiles.len() == 1 {
                            profiles[0].clone()
                        } else {
                            next += 1;
                            profiles[1 + (next - 1) % (profiles.len() - 1)].clone()
                        }
                    })
                    .collect::<Vec<_>>()
            })
        });
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .set_vehicle_roster(roster_signature)
                .map_err(|e| JsValue::from_str(&e))?;
            if let Some(identities) = &assigned {
                let poses = vehicles
                    .iter()
                    .map(|(position, heading)| (*position, *heading, Vec3::UP))
                    .collect::<Vec<_>>();
                if let Ok(z) = renderer.render_world_vehicles_with_profiles_surface(
                    camera,
                    heading,
                    &poses,
                    identities,
                    Some(local),
                    true,
                ) {
                    return Ok(z);
                }
            }
            renderer
                .render_world_vehicles(camera, heading, &vehicles, Some(local))
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render a bounded authoritative vehicle list in render-space coordinates.
    /// Pose tuples are x,y,z,heading; a negative local index keeps the supplied
    /// fallback camera while still drawing any available remote vehicles.
    #[wasm_bindgen]
    pub fn render_wgpu_world_vehicles(
        poses: Float32Array,
        roster_signature: &str,
        local_vehicle_index: i32,
        camera_x: f32,
        camera_y: f32,
        camera_z: f32,
        camera_heading: f32,
        width: u32,
        height: u32,
        use_simulation_ground: bool,
        ground_normal_x: f32,
        ground_normal_y: f32,
        ground_normal_z: f32,
        car_picks: Uint8Array,
    ) -> Result<f32, JsValue> {
        const MAX_POSES: usize = racer_render::MAX_RENDER_VEHICLES;
        if poses.length() as usize > MAX_POSES * 4 || poses.length() % 4 != 0 {
            return Err(JsValue::from_str(
                "render pose array must contain at most eight x,y,z,heading tuples",
            ));
        }
        if local_vehicle_index < -1
            || (local_vehicle_index >= 0
                && local_vehicle_index as usize >= poses.length() as usize / 4)
        {
            return Err(JsValue::from_str(
                "local render vehicle index is outside the pose list",
            ));
        }
        let mut values = vec![0.0_f32; poses.length() as usize];
        poses.copy_to(&mut values);
        if values.iter().any(|value| !value.is_finite()) {
            return Err(JsValue::from_str("render pose contains a non-finite value"));
        }
        let vehicles = values
            .chunks_exact(4)
            .map(|pose| (Vec3::new(pose[0], pose[1], pose[2]), pose[3]))
            .collect::<Vec<_>>();
        // Demo cars named by the page (generated tracks): one profile per vehicle.
        let named = decode_picks(&car_picks.to_vec());
        let profiles = if named.len() == vehicles.len() && !vehicles.is_empty() {
            resolve_profiles(&named)
        } else {
            None
        };
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .set_vehicle_roster(roster_signature)
                .map_err(|e| JsValue::from_str(&e))?;
            if let Some(identities) = &profiles {
                let local = (local_vehicle_index >= 0).then_some(local_vehicle_index as usize);
                let attitude = Vec3::new(ground_normal_x, ground_normal_y, ground_normal_z);
                let profiled = vehicles
                    .iter()
                    .enumerate()
                    .map(|(index, (position, heading))| {
                        let up = if Some(index) == local && attitude.z > 0.0 {
                            attitude
                        } else {
                            Vec3::UP
                        };
                        (*position, *heading, up)
                    })
                    .collect::<Vec<_>>();
                if let Ok(z) = renderer.render_world_vehicles_with_profiles_surface(
                    Vec3::new(camera_x, camera_y, camera_z),
                    camera_heading,
                    &profiled,
                    identities,
                    local,
                    !use_simulation_ground,
                ) {
                    return Ok(z);
                }
            }
            renderer
                .render_world_vehicles_with_attitude(
                    Vec3::new(camera_x, camera_y, camera_z),
                    camera_heading,
                    &vehicles,
                    (local_vehicle_index >= 0).then_some(local_vehicle_index as usize),
                    !use_simulation_ground,
                    (local_vehicle_index >= 0).then_some(Vec3::new(
                        ground_normal_x,
                        ground_normal_y,
                        ground_normal_z,
                    )),
                )
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Select a project-owned local preview camera. This affects rendering
    /// only and does not alter vehicle simulation or race state.
    #[wasm_bindgen]
    pub fn set_preview_camera_mode(name: &str) -> Result<(), JsValue> {
        let mode = PreviewCameraMode::from_name(name)
            .ok_or_else(|| JsValue::from_str("unknown preview camera mode"))?;
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.set_preview_camera_mode(mode);
            Ok(())
        })
    }

    /// Hold or release the project-owned preview look-behind view.
    #[wasm_bindgen]
    pub fn set_preview_look_behind(enabled: bool) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.set_preview_look_behind(enabled);
            Ok(())
        })
    }

    /// Set the smoothed yaw (radians, render space) used by the external chase
    /// and look-behind views, or clear it with a non-finite value so the view
    /// follows the body heading. Rendering only; simulation is unaffected.
    #[wasm_bindgen]
    pub fn set_chase_camera_heading(heading: f64) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.set_chase_camera_heading(heading.is_finite().then_some(heading as f32));
            Ok(())
        })
    }

    /// Show or hide the project-owned rear-view inset without changing race state.
    #[wasm_bindgen]
    pub fn set_preview_mirror_visible(visible: bool) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.set_preview_mirror_visible(visible);
            Ok(())
        })
    }

    /// Render a known overlapping-plane fixture through the active GPU
    /// pipeline. Used by browser acceptance tooling to verify depth order.
    #[wasm_bindgen]
    pub fn render_depth_order_test(width: u32, height: u32) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .render_depth_order_fixture()
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render a two-material scene that selects distinct texture-array layers.
    #[wasm_bindgen]
    pub fn render_texture_array_test(width: u32, height: u32) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .render_texture_array_fixture()
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render a known cutout texture over a background to verify transparent
    /// texels do not write color or depth.
    #[wasm_bindgen]
    pub fn render_alpha_cutout_test(width: u32, height: u32) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer.resize(width, height);
            renderer
                .render_alpha_cutout_fixture()
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render partially transparent project-owned surfaces for WebGL pixel
    /// checks; flags reverse submission order or add an opaque depth occluder.
    #[wasm_bindgen]
    pub fn render_straight_alpha_test(
        width: u32,
        height: u32,
        reverse_order: bool,
        opaque_occluder: bool,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .render_straight_alpha_fixture(width, height, reverse_order, opaque_occluder)
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    /// Render project-owned background panels for WebGL pixel acceptance.
    #[wasm_bindgen]
    pub fn render_background_test(
        width: u32,
        height: u32,
        yaw: f32,
        camera_x: f32,
        camera_y: f32,
        foreground: bool,
        enabled: bool,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .render_background_fixture(
                    width, height, yaw, camera_x, camera_y, foreground, enabled,
                )
                .map_err(|e| JsValue::from_str(&e))
        })
    }

    #[wasm_bindgen]
    pub fn render_horizon_test(
        width: u32,
        height: u32,
        yaw: f32,
        pitch: f32,
        camera_x: f32,
        camera_y: f32,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .render_horizon_fixture(width, height, yaw, pitch, camera_x, camera_y)
                .map_err(|error| JsValue::from_str(&error))
        })
    }

    /// Render a project-owned actual rear-view scene inset for browser pixel
    /// checks. Fixture colors distinguish front, rear, the local car, and an
    /// optional nearer occluder; attitude inputs affect only mirror camera-up.
    #[wasm_bindgen]
    pub fn render_rear_mirror_test(
        width: u32,
        height: u32,
        up_x: f32,
        up_y: f32,
        occluder: bool,
        transparent: bool,
        reverse_alpha: bool,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .render_rear_mirror_fixture(
                    width,
                    height,
                    up_x,
                    up_y,
                    occluder,
                    transparent,
                    reverse_alpha,
                )
                .map_err(|error| JsValue::from_str(&error))
        })
    }

    /// Exercise successful mirror-cache replacement and failed-install
    /// rollback using only project-owned synthetic scenes.
    #[wasm_bindgen]
    pub fn render_mirror_cache_lifecycle_test(
        width: u32,
        height: u32,
        stage: u8,
    ) -> Result<(), JsValue> {
        GPU.with(|slot| {
            let mut slot = slot.borrow_mut();
            let renderer = slot
                .as_mut()
                .ok_or_else(|| JsValue::from_str("WebGPU renderer is not initialized"))?;
            renderer
                .render_mirror_cache_lifecycle_fixture(width, height, stage)
                .map_err(|error| JsValue::from_str(&error))
        })
    }

    fn saturating_ticks(ticks: u64) -> u32 {
        u32::try_from(ticks).unwrap_or(u32::MAX)
    }

    #[wasm_bindgen]
    pub struct BrowserRace {
        inner: LocalRaceClient,
    }

    impl BrowserRace {
        /// Follow the demo car chosen in the installed scene: its classic
        /// parameters drive the local car and set the opponents' limits.
        fn sync_project_car(&mut self) {
            let car = PROJECT_FIELD.with(|field| field.borrow().player.0);
            self.inner.set_project_car(car);
        }
    }

    #[wasm_bindgen]
    impl BrowserRace {
        #[wasm_bindgen(constructor)]
        pub fn new(physics_hz: u32) -> BrowserRace {
            BrowserRace {
                inner: LocalRaceClient::new(physics_hz),
            }
        }
        /// Install the descriptor's imported car-data block (JSON) with the parser a
        /// private build registered; the stock build rejects it.
        pub fn set_imported_car_data(&mut self, json: &str) -> Result<(), JsValue> {
            let data = parse_imported_car_data(json).map_err(|e| JsValue::from_str(&e))?;
            self.inner.set_imported_data(Some(data));
            Ok(())
        }
        pub fn clear_imported_car_data(&mut self) {
            self.inner.set_imported_data(None);
        }
        /// Diagnostic line for the import status UI.
        pub fn imported_car_data_status(&self) -> String {
            match self.inner.imported_data() {
                Some(data) => data.status_text(),
                None => "imported car data: unavailable".to_string(),
            }
        }
        /// JSON of the client message carrying the derived model selection, or
        /// `undefined` without complete data. The page decides whether to send it.
        pub fn imported_car_message_json(&self) -> Option<String> {
            self.inner.imported_message_json()
        }
        pub fn set_controls(&mut self, throttle: f64, brake: f64, steer: f64, handbrake: bool) {
            self.inner.set_input(DriverInput {
                throttle,
                brake,
                steer,
                handbrake,
                shift_up: false,
                shift_down: false,
                reset_request: false,
            });
        }
        /// Gear-change requests for the next tick (A up, Z down). Held keys
        /// are one request; the model detects the rising edge.
        pub fn set_shift(&mut self, shift_up: bool, shift_down: bool) {
            self.inner.set_shift(shift_up, shift_down);
        }
        /// Gearbox mode of the local driver: `true` is manual (A/Z walk
        /// R, N, 1, 2 ...), `false` automatic.
        pub fn set_manual_transmission(&mut self, manual: bool) {
            self.inner
                .set_transmission(racer_sim::Transmission::from_manual(manual));
        }
        /// Auto reverse: no shifting; brake at a standstill reverses.
        pub fn set_auto_reverse(&mut self, enabled: bool) {
            self.inner.set_auto_reverse(enabled);
        }
        pub fn auto_reverse(&self) -> bool {
            self.inner.auto_reverse()
        }
        pub fn manual_transmission(&self) -> bool {
            self.inner.transmission().is_manual()
        }
        pub fn request_reset(&mut self) {
            self.inner.reset();
        }
        /// "handling: <model id>" of the active vehicle model.
        pub fn handling_status(&self) -> String {
            self.inner.handling_status_text()
        }
        /// Protocol `HANDLING_*` id of the active vehicle model.
        pub fn handling_model_id(&self) -> u32 {
            u32::from(self.inner.handling_code())
        }
        /// Select an external vehicle model (registered at build time by the
        /// embedding crate) with opaque parameter bytes. Installs it for solo
        /// driving. Returns the JSON of the `SetCarModel` message to send when
        /// in a room (pass it to `encode_client_message`), or rejects with the
        /// provider's reason.
        pub fn set_car_model(&mut self, model_id: &str, params: &[u8]) -> Result<String, JsValue> {
            self.inner
                .set_car_model(model_id, params)
                .map_err(|e| JsValue::from_str(&e))?;
            let message = self
                .inner
                .car_model_message()
                .ok_or_else(|| JsValue::from_str("no car model selected"))?;
            serde_json::to_string(&message).map_err(|e| JsValue::from_str(&e.to_string()))
        }
        pub fn clear_car_model(&mut self) {
            self.inner.clear_car_model();
        }
        /// JSON of the `SetCarModel` message for the selected external model.
        pub fn car_model_message_json(&self) -> Option<String> {
            serde_json::to_string(&self.inner.car_model_message()?).ok()
        }
        /// Ids of the external vehicle models linked into this build.
        pub fn registered_car_models(&self) -> Array {
            registered_vehicle_models()
                .into_iter()
                .map(|id| JsValue::from_str(&id))
                .collect()
        }
        pub fn use_open_preview(&mut self) -> Result<(), JsValue> {
            let collision = PENDING_PREVIEW_COLLISION.with(|slot| slot.borrow_mut().take());
            if let Some(collision) = collision {
                self.inner
                    .use_open_preview_with_collision(collision)
                    .map_err(|error| JsValue::from_str(error))
            } else {
                self.inner.use_open_preview();
                Ok(())
            }
        }
        pub fn use_project_circuit(&mut self) {
            self.sync_project_car();
            self.inner.use_project_circuit();
        }
        /// Race the imported track for `laps` laps (0 = free driving). True
        /// when it races; false when the track has no race course. Call
        /// after `use_open_preview` and before `configure_opponents`.
        pub fn set_imported_laps(&mut self, laps: u32) -> bool {
            self.inner.set_imported_laps(laps)
        }
        /// True when the imported track's road makes a race course.
        pub fn imported_course_available(&self) -> bool {
            self.inner.imported_course_available()
        }
        /// Laps of the race on the imported track (0 when free driving).
        pub fn imported_laps(&self) -> u32 {
            self.inner.imported_laps()
        }
        /// Drive the track installed by `install_project_track`.
        pub fn use_project_track(&mut self) -> Result<(), JsValue> {
            let pending = PENDING_PROJECT_TRACK.with(|slot| slot.borrow_mut().take());
            let track =
                pending.ok_or_else(|| JsValue::from_str("no project track is installed"))?;
            self.sync_project_car();
            self.inner
                .use_project_track(&track)
                .map_err(JsValue::from_str)
        }
        pub fn preview_ground_enabled(&self) -> bool {
            self.inner.has_preview_ground()
        }
        pub fn z(&self) -> f64 {
            self.inner.snapshot().z
        }
        pub fn ground_normal_x(&self) -> f64 {
            self.inner.snapshot().ground_normal[0]
        }
        pub fn ground_normal_y(&self) -> f64 {
            self.inner.snapshot().ground_normal[1]
        }
        pub fn ground_normal_z(&self) -> f64 {
            self.inner.snapshot().ground_normal[2]
        }
        pub fn velocity_x(&self) -> f64 {
            self.inner.snapshot().velocity_x
        }
        pub fn velocity_y(&self) -> f64 {
            self.inner.snapshot().velocity_y
        }
        pub fn tick(&mut self) {
            self.sync_project_car();
            self.inner.tick();
        }
        pub fn x(&self) -> f64 {
            self.inner.snapshot().x
        }
        pub fn y(&self) -> f64 {
            self.inner.snapshot().y
        }
        pub fn heading(&self) -> f64 {
            self.inner.snapshot().heading
        }
        pub fn speed(&self) -> f64 {
            self.inner.snapshot().speed
        }
        pub fn lap(&self) -> u32 {
            self.inner.snapshot().lap
        }
        pub fn checkpoint(&self) -> usize {
            self.inner.snapshot().checkpoint
        }
        pub fn race_position(&self) -> u32 {
            u32::from(self.inner.snapshot().race_position)
        }
        /// Cars ranked together with the player (1 without opponents).
        pub fn competitors(&self) -> u32 {
            u32::from(self.inner.snapshot().competitors)
        }
        /// Add `count` computer opponents (0..=7) on the project circuit.
        /// `skill` 0 = beginner, 1 = advanced. Call after `use_project_circuit`.
        pub fn configure_opponents(&mut self, count: u32, skill: u32, catch_up: bool) {
            let count = (count as usize).min(racer_protocol::MAX_AI_OPPONENTS as usize);
            self.inner
                .configure_opponents(count, skill.min(1) as u8, catch_up);
        }
        pub fn opponent_count(&self) -> u32 {
            self.inner.opponent_snapshots().len() as u32
        }
        /// Flattened opponent state, 10 numbers each: id, x, y, heading,
        /// velocity x, velocity y, speed, laps, finished (0/1), race position.
        pub fn opponent_states(&self) -> js_sys::Float64Array {
            let values: Vec<f64> = self
                .inner
                .opponent_snapshots()
                .iter()
                .flat_map(|o| {
                    [
                        f64::from(o.id),
                        o.x,
                        o.y,
                        o.heading,
                        o.velocity_x,
                        o.velocity_y,
                        o.speed,
                        f64::from(o.laps),
                        f64::from(u8::from(o.finished)),
                        f64::from(o.race_position),
                    ]
                })
                .collect();
            js_sys::Float64Array::from(values.as_slice())
        }
        /// Origin height (scene Z) of each opponent, in `opponent_states` order.
        pub fn opponent_heights(&self) -> js_sys::Float64Array {
            let values: Vec<f64> = self
                .inner
                .opponent_snapshots()
                .iter()
                .map(|o| o.z)
                .collect();
            js_sys::Float64Array::from(values.as_slice())
        }
        /// Ticks are 60 Hz race ticks; saturate rather than wrap at u32::MAX.
        pub fn current_lap_ticks(&self) -> u32 {
            saturating_ticks(self.inner.snapshot().current_lap_ticks)
        }
        pub fn last_lap_ticks(&self) -> Option<u32> {
            self.inner.snapshot().last_lap_ticks.map(saturating_ticks)
        }
        pub fn best_lap_ticks(&self) -> Option<u32> {
            self.inner.snapshot().best_lap_ticks.map(saturating_ticks)
        }
        /// Gear 1..=5; 0 when the active model has no gearbox.
        pub fn gear(&self) -> u32 {
            u32::from(self.inner.snapshot().gear)
        }
        pub fn rpm01(&self) -> f64 {
            self.inner.snapshot().rpm01
        }
        pub fn finish_ticks(&self) -> Option<u32> {
            self.inner.snapshot().finish_ticks.map(saturating_ticks)
        }
        pub fn tick_count(&self) -> u64 {
            self.inner.snapshot().tick
        }
        pub fn finished(&self) -> bool {
            self.inner.snapshot().finished
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestModel;
    impl racer_sim::model_ext::VehicleDynamics for TestModel {
        fn clone_box(&self) -> Box<dyn racer_sim::model_ext::VehicleDynamics> {
            Box::new(TestModel)
        }
        fn step(
            &mut self,
            vehicle: racer_sim::Vehicle,
            _input: DriverInput,
            dt: f64,
        ) -> racer_sim::model_ext::VehicleStep {
            let velocity = Vec2::new(5.0, 0.0);
            racer_sim::model_ext::VehicleStep {
                heading: 0.0,
                angular_velocity: 0.0,
                velocity,
                proposed_position: vehicle.position + velocity * dt,
            }
        }
    }
    struct TestProvider;
    impl VehicleModelProvider for TestProvider {
        fn id(&self) -> &str {
            "test.client-model"
        }
        fn create(
            &self,
            params: &[u8],
        ) -> Result<Box<dyn racer_sim::model_ext::VehicleDynamics>, String> {
            if params == b"bad" {
                Err("bad params".into())
            } else {
                Ok(Box::new(TestModel))
            }
        }
    }

    #[test]
    fn external_model_drives_solo_and_survives_track_switches() {
        let _ = register_vehicle_model(Box::new(TestProvider));
        assert!(registered_vehicle_models().contains(&"test.client-model".to_string()));
        let mut client = LocalRaceClient::new(60);
        assert!(client.car_model_message().is_none());
        assert!(client.set_car_model("missing.model", b"{}").is_err());
        assert!(client.set_car_model("test.client-model", b"bad").is_err());
        assert_eq!(client.handling_model(), VehicleModel::Classic);
        client.set_car_model("test.client-model", b"{}").unwrap();
        assert_eq!(client.handling_model(), VehicleModel::Custom);
        let message = client.car_model_message().unwrap();
        assert_eq!(
            message,
            racer_protocol::ClientMessage::SetCarModel {
                model_id: "test.client-model".into(),
                params: b"{}".to_vec()
            }
        );
        let x0 = client.snapshot().x;
        for _ in 0..30 {
            client.tick();
        }
        assert!(client.snapshot().x > x0 + 1.0);
        client.use_project_circuit();
        assert_eq!(client.handling_model(), VehicleModel::Custom);
        client.clear_car_model();
        assert_eq!(client.handling_model(), VehicleModel::Classic);
        assert!(client.car_model_message().is_none());
    }

    #[test]
    fn profiled_render_pose_parser_matches_profile_count_and_support_normal() {
        let profiles = [
            VehicleProfileIdentity::new("vehicle-profile-a", &"a".repeat(64)).unwrap(),
            VehicleProfileIdentity::new("vehicle-profile-b", &"b".repeat(64)).unwrap(),
        ];
        let poses = [
            1.0, 2.0, 3.0, 0.25, 0.0, 0.0, 1.0, -4.0, 5.0, 6.0, -0.5, 0.0, 0.6, 0.8,
        ];
        let parsed = parse_profiled_render_poses(&poses, &profiles).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(parsed[1].1, -0.5);
        assert!((parsed[1].2.length() - 1.0).abs() < 1.0e-6);
        assert_eq!(parsed[1].2, Vec3::new(0.0, 0.6, 0.8));
        assert!(parse_profiled_render_poses(&poses[..7], &profiles).is_err());
        let mut invalid = poses;
        invalid[4] = f32::NAN;
        assert!(parse_profiled_render_poses(&invalid, &profiles).is_err());
        let mut invalid = poses;
        invalid[11..14].copy_from_slice(&[0.0, 0.0, 0.0]);
        assert!(parse_profiled_render_poses(&invalid, &profiles).is_err());
        let mut invalid = poses;
        invalid[4] = f32::MAX;
        assert!(parse_profiled_render_poses(&invalid, &profiles).is_err());
        let mut invalid = poses;
        invalid[0] = f32::MAX;
        assert!(parse_profiled_render_poses(&invalid, &profiles).is_err());
    }

    #[test]
    fn local_opponents_start_on_the_grid_move_and_share_the_ranking() {
        let mut client = LocalRaceClient::new(60);
        assert!(client.opponent_snapshots().is_empty());
        assert_eq!(client.snapshot().competitors, 1);
        client.configure_opponents(3, 1, true);
        let start = client.opponent_snapshots();
        assert_eq!(start.len(), 3);
        assert_eq!(client.snapshot().competitors, 4);
        // The human takes the rearmost grid slot.
        assert_eq!(client.snapshot().race_position, 4);
        for _ in 0..(2 * 60) {
            client.tick();
        }
        let later = client.opponent_snapshots();
        for (a, b) in start.iter().zip(&later) {
            assert!(
                (b.x - a.x).hypot(b.y - a.y) > 5.0,
                "opponent {} stayed",
                a.id
            );
        }
        for _ in 0..(6 * 60) {
            client.tick();
        }
        let later = client.opponent_snapshots();
        let mut ranks: Vec<u16> = later.iter().map(|o| o.race_position).collect();
        ranks.push(client.snapshot().race_position);
        ranks.sort_unstable();
        assert_eq!(ranks, vec![1, 2, 3, 4]);
        // Removing them restores solo practice.
        client.configure_opponents(0, 1, true);
        assert!(client.opponent_snapshots().is_empty());
        assert_eq!(client.snapshot().competitors, 1);
        client.configure_opponents(2, 0, false);
        client.use_open_preview();
        assert!(client.opponent_snapshots().is_empty());
    }

    #[test]
    fn local_client_uses_explicit_simulation_ticks() {
        let mut client = LocalRaceClient::new(60);
        client.set_input(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        let first = client.tick();
        assert_eq!(first.tick, 1);
        let later = (0..59).fold(first, |_, _| client.tick());
        assert_eq!(later.tick, 60);
        assert!(later.x > first.x);
    }
}
