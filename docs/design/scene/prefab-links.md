# Prefab Links

A prefab instance the editor places keeps a link to the prefab asset it came
from, so it can be updated from the prefab later while keeping its overrides.
Updating runs only when asked.

Code: `crates/calyx_engine/src/scene/prefab_link.rs` (its module docs state
the full rules), `crates/calyx_editor/src/prefab_instances.rs` (the editor
actions).

## Linking

- The editor instantiates a prefab with `Scene::instantiate_linked_prefab`,
  which records on each instance object the prefab asset, the prefab object it
  came from and a hash of each component value the prefab gave it.
- The hash covers the component's enabled state too
  ([Component Enabled State](component-enabled-state.md)).
- The links save with the scene in `SceneData::prefab_links`. A subtree, and
  so a prefab, carries none.
- Instantiation remaps object ids; the remap reaches project components
  through `ComponentInstance::deserialize_remapped`
  ([Project Assembly](../project/project-assembly.md#statics)).

## Updating

`Scene::update_prefab_instances` updates linked instances from the prefab's
current data.

- It keeps each instance's root transform and object ids.
- It keeps every override: a component whose value differs from the one the
  prefab last gave it, or that the instance added or removed.
- It adds objects the prefab added, and deletes objects it removed unless they
  carry an override.
- It leaves objects the instance deleted deleted.

In the editor the update is the explicit Update Prefab Instances action (Edit
menu for every prefab, or the prefab asset's Update Instances context
action). It reads each `.cxprefab` from disk and updates the open scene as one
undo step, refused in prefab mode and while a simulation scene exists. Saving
a prefab in [Prefab Mode](../editor/prefab-mode.md) never changes instances by
itself.

An imported model's material set by hand on an instance is an override too,
so it survives a reimport ([Model Import](../assets/model-import.md)).
