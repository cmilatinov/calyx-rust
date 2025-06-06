use egui::{Key, Modifiers};
use engine::component::{Component, ComponentEventContext, ReflectComponent};
use engine::input::Input;
use engine::net::{GameMessage, Server};
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "a7e45032-e721-42f3-87af-7fc5e60cac82"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network", update)]
#[repr(C)]
pub struct ComponentNetworkManager {}

impl Component for ComponentNetworkManager {
    fn update(&mut self, _ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {
        let network = resources.network_mut();
        let connect_host = input
            .input_mut(|input| input.consume_key(Modifiers::NONE, Key::H))
            .unwrap_or(false);
        if connect_host && !network.is_host() {
            match network.host(Server::addr()) {
                Ok(_) => println!("SERVER - {:?}", Server::addr()),
                Err(err) => println!("{}", err),
            }
        }

        let connect_client = input
            .input_mut(|input| input.consume_key(Modifiers::NONE, Key::C))
            .unwrap_or(false);
        if connect_client && !network.client.is_connected() {
            match network.client.connect(Server::addr()) {
                Ok(_) => println!("CLIENT - Connecting ..."),
                Err(err) => println!("{}", err),
            }
        }

        let ping = input
            .input_mut(|input| input.consume_key(Modifiers::NONE, Key::P))
            .unwrap_or(false);
        if ping && network.client.is_connected() {
            println!("CLIENT - Ping {:?}", network.client.rtt());
        }
    }
}
