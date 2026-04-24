use egui::{Key, Modifiers};
use engine::assets::AssetRef;
use engine::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use engine::input::Input;
use engine::net::Server;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::Prefab;
use engine::try_all;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use log::{error, info, trace};
use serde::{Deserialize, Serialize};

#[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "a7e45032-e721-42f3-87af-7fc5e60cac82"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Network Manager")]
#[repr(C)]
pub struct ComponentNetworkManager {
    pub player_prefab: AssetRef<Prefab>,
}

impl Component for ComponentNetworkManager {}

impl ComponentUpdate for ComponentNetworkManager {
    fn update(
        &self,
        ComponentEventContext {
            scene,
            registries: assets,
            game_object,
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let Some(player_prefab) =
            scene.read_component::<ComponentNetworkManager, _, _>(game_object, |c| {
                c.player_prefab.clone()
            })
        else {
            return;
        };

        let network = resources.network_mut();

        'connect_host: {
            let connect_host = input
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::H))
                .unwrap_or(false);
            if !connect_host || network.is_host() {
                break 'connect_host;
            }
            match network.host(Server::addr()) {
                Ok(_) => trace!("SERVER - {:?}", Server::addr()),
                Err(err) => error!("{}", err),
            }
        }

        'connect_client: {
            let connect_client = input
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::C))
                .unwrap_or(false);
            if !connect_client || network.client.is_connected() {
                break 'connect_client;
            }
            match network.client.connect(Server::addr()) {
                Ok(_) => {
                    info!("CLIENT - Connecting ...");
                    try_all!(
                        None => return;
                        let prefab_ref = player_prefab.get_ref(assets);
                    );
                    network.instantiate_prefab(scene, prefab_ref);
                }
                Err(err) => error!("CLIENT - Failed to connect: {}", err),
            }
        }
    }
}
