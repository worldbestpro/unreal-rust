use bevy_ecs::prelude::*;
use ffi::{ActorComponentPtr, ActorComponentType};
use std::{collections::HashMap, ffi::c_void};

use crate::{
    ffi::{self, AActorOpaque},
    iterate_actors,
    math::{Quat, Vec3},
    module::{bindings, UserModule}, input::Input,
};
pub struct UnrealCore {
    world: World,
    schedule: Schedule,
    startup: Schedule,
    reflection_registry: ReflectionRegistry,
}

impl UnrealCore {
    pub fn new(module: &dyn UserModule) -> Self {
        log::info!("Initialize Rust");
        let mut startup = Schedule::default();
        startup.add_stage(CoreStage::Startup, SystemStage::single_threaded());

        let mut schedule = Schedule::default();
        schedule
            .add_stage(CoreStage::PreUpdate, SystemStage::single_threaded())
            .add_stage(CoreStage::Update, SystemStage::single_threaded())
            .add_stage(CoreStage::PostUpdate, SystemStage::single_threaded());

        schedule.add_system_to_stage(CoreStage::PreUpdate, download_transform_from_unreal.system());
        schedule.add_system_to_stage(CoreStage::PostUpdate, upload_transform_to_unreal.system());

        let mut reflection_registry = ReflectionRegistry::default();
        register_core_components(&mut reflection_registry);
        module.register(&mut reflection_registry);
        module.systems(&mut startup, &mut schedule);
        Self {
            world: World::new(),
            schedule,
            startup,
            reflection_registry,
        }
    }

    pub fn begin_play(&mut self, module: &dyn UserModule) {
        std::panic::set_hook(Box::new(|panic_info| {
            if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                let location = panic_info.location().map_or("".to_string(), |loc| {
                    format!("{}, at line {}", loc.file(), loc.line())
                });
                log::error!("Panic: {} => {}", location, s);
            } else {
                log::error!("panic occurred");
            }
        }));
        *self = Self::new(module);
        log::info!("BeginPlay Rust");
        self.world.insert_resource(Frame::default());
        self.world.insert_resource(Input::default());
        self.world.insert_resource(ActorRegistration::default());
        let mut startup = Schedule::default();
        startup.add_stage(CoreStage::Startup, SystemStage::single_threaded());
        startup.add_system_to_stage(CoreStage::Startup, register_actors.system());
        startup.run_once(&mut self.world);
        self.startup.run_once(&mut self.world);
        let mut schedule = Schedule::default();
        schedule
            .add_stage(CoreStage::PreUpdate, SystemStage::single_threaded())
            .add_stage(CoreStage::Update, SystemStage::single_threaded())
            .add_stage(CoreStage::PostUpdate, SystemStage::single_threaded());
        schedule.add_system_to_stage(CoreStage::PreUpdate, update_input.system());
        schedule.add_system_to_stage(CoreStage::PreUpdate, download_transform_from_unreal.system());
        schedule.add_system_to_stage(CoreStage::PostUpdate, upload_transform_to_unreal.system());
        module.systems(&mut startup, &mut schedule);
        self.schedule = schedule;
        
    }
    pub fn tick(&mut self, dt: f32) {
        if let Some(mut frame) = self.world.get_resource_mut::<Frame>() {
            frame.dt = dt;
        }
        self.schedule.run_once(&mut self.world);
        self.world.clear_trackers();
    }
}

pub unsafe extern "C" fn retrieve_uuids(ptr: *mut ffi::Uuid, len: *mut usize) {
    if let Some(global) = crate::module::MODULE.as_mut() {
        if ptr == std::ptr::null_mut() {
            *len = global.core.reflection_registry.uuid_set.len();
        } else {
            let slice = std::ptr::slice_from_raw_parts_mut(ptr, *len);
            for (idx, uuid) in global
                .core
                .reflection_registry
                .uuid_set
                .iter()
                .take(*len)
                .enumerate()
            {
                (*slice)[idx] = ffi::Uuid {
                    bytes: *uuid.as_bytes(),
                };
            }
        }
    }
}

pub unsafe extern "C" fn get_velocity(actor: *const AActorOpaque, velocity: &mut ffi::Vector3) {
    if let Some(global) = crate::module::MODULE.as_mut() {
        if let Some(entity) = global
            .core
            .world
            .get_resource::<ActorRegistration>()
            .and_then(|reg| reg.actor_to_entity.get(&ActorPtr(actor as *mut _)))
            .copied()
        {
            if let Some(movement) = global
                .core
                .world
                .get_entity(entity)
                .and_then(|eref| eref.get::<MovementComponent>())
            {
                *velocity = movement.velocity.into();
            }
        }
    }
}
pub extern "C" fn tick(dt: f32) -> crate::ffi::ResultCode {
    let r = std::panic::catch_unwind(|| unsafe {
        UnrealCore::tick(&mut crate::module::MODULE.as_mut().unwrap().core, dt);
    });
    match r {
        Ok(_) => ffi::ResultCode::Success,
        Err(_) => ffi::ResultCode::Panic,
    }
}

pub extern "C" fn begin_play() -> ffi::ResultCode {
    let r = std::panic::catch_unwind(|| unsafe {
        let global = crate::module::MODULE.as_mut().unwrap();
        UnrealCore::begin_play(&mut global.core, global.module.as_ref());
    });
    match r {
        Ok(_) => ffi::ResultCode::Success,
        Err(_) => ffi::ResultCode::Panic,
    }
}
pub fn register_core_components(registry: &mut ReflectionRegistry) {
    registry.register::<TransformComponent>();
    registry.register::<ActorComponent>();
    registry.register::<PlayerInputComponent>();
    registry.register::<MovementComponent>();
    registry.register::<CameraComponent>();
    registry.register::<ParentComponent>();
    registry.register::<PhysicsComponent>();
}

use unreal_reflect::{impl_component, registry::ReflectionRegistry, TypeUuid};
#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
pub enum CoreStage {
    Startup,
    PreUpdate,
    Update,
    PostUpdate,
}
#[derive(Default, Debug, Copy, Clone)]
pub struct Frame {
    pub dt: f32,
}

#[derive(Default, Debug, TypeUuid)]
#[uuid = "5ad05c2b-7cbc-4081-8819-1997b3e13331"]
pub struct ActorComponent {
    pub ptr: ActorPtr,
}
impl_component!(ActorComponent);
#[derive(Default, Debug, TypeUuid)]
#[uuid = "ffc10b5c-635c-43ce-8288-e3c6f6d67e36"]
pub struct PhysicsComponent {
    pub ptr: UnrealPtr<Primitive>,
    pub is_simulating: bool,
    pub velocity: Vec3,
}

impl PhysicsComponent {
    pub fn new(ptr: UnrealPtr<Primitive>) -> Self {
        let mut p = Self {
            ptr,
            ..Default::default()
        };
        p.download_state();
        p
    }
    pub fn download_state(&mut self) {
        self.is_simulating = (bindings().physics_bindings.is_simulating)(self.ptr.ptr) == 1;
        self.velocity = (bindings().physics_bindings.get_velocity)(self.ptr.ptr).into();
    }

    pub fn upload_state(&mut self) {
        (bindings().physics_bindings.set_velocity)(self.ptr.ptr, self.velocity.into());
    }

    pub fn add_impulse(&mut self, impulse: Vec3) {
        (bindings().physics_bindings.add_impulse)(self.ptr.ptr, impulse.into());
    }

    pub fn add_force(&mut self, force: Vec3) {
        (bindings().physics_bindings.add_force)(self.ptr.ptr, force.into());
    }
}

impl_component!(PhysicsComponent);

#[derive(Default, Debug, TypeUuid, Clone)]
#[uuid = "b8738d9e-ab21-47db-8587-4019b38e35a6"]
pub struct TransformComponent {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
impl TransformComponent {
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::X
    }
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }
    pub fn is_nan(&self) -> bool {
        self.position.is_nan() || self.rotation.is_nan() || self.scale.is_nan()
    }
}

impl_component!(TransformComponent);
#[derive(Default, Debug, TypeUuid)]
#[uuid = "8d2df877-499b-46f3-9660-bd2e1867af0d"]
pub struct CameraComponent {
    pub x: f32,
    pub y: f32,
    pub current_x: f32,
    pub current_y: f32,
}
impl_component!(CameraComponent);

#[derive(Default, Debug, TypeUuid)]
#[uuid = "fc8bd668-fc0a-4ab7-8b3d-f0f22bb539e2"]
pub struct MovementComponent {
    pub velocity: Vec3,
    pub view: Quat,
    pub is_falling: bool,
}
impl_component!(MovementComponent);

#[derive(Debug, TypeUuid)]
#[uuid = "f1e22f5b-2bfe-4ce5-938b-7c093def708e"]
pub struct ParentComponent {
    pub parent: Entity,
}
impl_component!(ParentComponent);

impl Default for ParentComponent {
    fn default() -> Self {
        todo!()
    }
}

#[derive(Default, Debug, TypeUuid)]
#[uuid = "35256309-43b4-4459-9884-eb6e9137faf5"]
pub struct PlayerInputComponent {
    pub direction: Vec3,
}
impl_component!(PlayerInputComponent);

// TODO: Implement unregister.
#[derive(Default)]
pub struct ActorRegistration {
    pub actor_to_entity: HashMap<ActorPtr, Entity>,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct ActorPtr(pub *mut AActorOpaque);
unsafe impl Send for ActorPtr {}
unsafe impl Sync for ActorPtr {}
impl Default for ActorPtr {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnrealPtr<T> {
    pub ptr: *mut c_void,
    _m: std::marker::PhantomData<T>,
}
impl<T> UnrealPtr<T> {
    pub fn from_raw(ptr: *mut c_void) -> Self {
        Self {
            ptr,
            ..Default::default()
        }
    }
}
unsafe impl<T> Send for UnrealPtr<T> {}
unsafe impl<T> Sync for UnrealPtr<T> {}
impl<T> Default for UnrealPtr<T> {
    fn default() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            _m: Default::default(),
        }
    }
}
impl<T> Clone for UnrealPtr<T> {
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr.clone(),
            _m: self._m.clone(),
        }
    }
}

impl<T> Copy for UnrealPtr<T> {}

#[derive(Debug)]
pub enum Capsule {}
#[derive(Debug)]
pub enum Primitive {}

fn download_transform_from_unreal(mut query: Query<(&ActorComponent, &mut TransformComponent)>) {
    for (actor, mut transform) in query.iter_mut() {
        let mut position = ffi::Vector3::default();
        let mut rotation = ffi::Quaternion::default();
        let mut scale = ffi::Vector3::default();

        (bindings().get_spatial_data)(actor.ptr.0, &mut position, &mut rotation, &mut scale);

        transform.position = position.into();
        transform.rotation = rotation.into();
        transform.scale = scale.into();
        assert!(!transform.is_nan());
    }
}
fn upload_transform_to_unreal(query: Query<(&ActorComponent, &TransformComponent)>) {
    for (actor, transform) in query.iter() {
        assert!(!transform.is_nan());
        (bindings().set_spatial_data)(
            actor.ptr.0,
            transform.position.into(),
            transform.rotation.into(),
            transform.scale.into(),
        );
    }
}

fn update_input(mut input: ResMut<Input>) {
    input.update();
}

fn register_actors(mut actor_register: ResMut<ActorRegistration>, mut commands: Commands) {
    for actor in iterate_actors(bindings()) {
        let entity = commands
            .spawn()
            .insert_bundle((
                ActorComponent {
                    ptr: ActorPtr(actor),
                },
                TransformComponent::default(),
                MovementComponent::default(),
                PlayerInputComponent::default(),
            ))
            .id();

        //let mut len: usize = 0;
        //(bindings().get_actor_components)(actor, std::ptr::null_mut(), &mut len);
        //let mut components: Vec<ActorComponentPtr> = Vec::with_capacity(len);
        //(bindings().get_actor_components)(actor, components.as_mut_ptr(), &mut len);
        //unsafe {
        //    components.set_len(len);
        //}
        //for component in components {
        //    match component.ty {
        //        ActorComponentType::Capsule => {
        //            let mut capsule = CapsuleComponent{
        //                ptr: UnrealPtr::from_raw(component.ptr)
        //            };

        //            capsule.apply_force(Vec3::Z * 10000000.0);
        //        }
        //    }
        //}
        let mut root_component = ActorComponentPtr::default();
        (bindings().get_root_component)(actor, &mut root_component);
        if root_component.ty == ActorComponentType::Primitive && root_component.ptr != std::ptr::null_mut() {
            let physics_component = PhysicsComponent::new(UnrealPtr::from_raw(root_component.ptr));
            commands.entity(entity).insert(physics_component);

        }

        actor_register
            .actor_to_entity
            .insert(ActorPtr(actor), entity);
    }
}