use legion::Entity;
use petgraph::prelude::{EdgeRef, StableGraph};
use petgraph::stable_graph::{DefaultIx, NodeIndex, WalkNeighbors};
use petgraph::visit::{Bfs, Walker};
use petgraph::Direction;

use super::scene::Scene;
use super::GameObject;

pub enum SiblingDir {
    Before,
    After,
}

/// Manages the hierarchical parent-child relationships between game objects
/// using a directed graph (petgraph StableGraph).
pub struct SceneGraph {
    arena: StableGraph<Entity, i32>,
}

pub struct WalkChildren {
    walker: WalkNeighbors<DefaultIx>,
}

impl WalkChildren {
    pub fn next(&mut self, scene: &Scene) -> Option<GameObject> {
        scene.graph.walk_next(&mut self.walker)
    }
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self {
            arena: StableGraph::default(),
        }
    }
}

impl SceneGraph {
    pub fn add_node(&mut self, entity: Entity) -> NodeIndex {
        self.arena.add_node(entity)
    }

    pub fn game_object_from_node(&self, node: NodeIndex) -> Option<GameObject> {
        self.arena
            .node_weight(node)
            .map(|e| GameObject { node, entity: *e })
    }

    /// Advance a detached walker one step, returning the next GameObject if any.
    pub fn walk_next(&self, walker: &mut WalkNeighbors<DefaultIx>) -> Option<GameObject> {
        walker
            .next_node(&self.arena)
            .and_then(|node| self.game_object_from_node(node))
    }

    /// BFS traversal starting from a node, returning all reachable GameObjects
    /// (including the start node).
    pub fn bfs_from(&self, start: NodeIndex) -> Vec<GameObject> {
        Bfs::new(&self.arena, start)
            .iter(&self.arena)
            .filter_map(|node| self.game_object_from_node(node))
            .collect()
    }

    pub fn parent(&self, game_object: GameObject) -> Option<GameObject> {
        self.arena
            .neighbors_directed(game_object.node, Direction::Incoming)
            .next()
            .and_then(|node| self.game_object_from_node(node))
    }

    pub fn children(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        self.arena
            .neighbors(game_object.node)
            .filter_map(|node| self.game_object_from_node(node))
    }

    pub fn children_walker(&self, game_object: GameObject) -> WalkChildren {
        WalkChildren {
            walker: self.arena.neighbors(game_object.node).detach(),
        }
    }

    pub fn children_ordered(
        &self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + '_ {
        let mut children = self
            .arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .filter_map(|edge| {
                self.game_object_from_node(edge.target())
                    .map(|go| (edge.weight(), go))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|c| c.0);
        children.into_iter().map(|c| c.1)
    }

    pub fn child_at(&self, game_object: GameObject, index: i32) -> Option<GameObject> {
        self.arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .find(|edge| *edge.weight() == index)
            .and_then(|edge| self.game_object_from_node(edge.target()))
    }

    pub fn descendants(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        Bfs::new(&self.arena, game_object.node)
            .iter(&self.arena)
            .filter_map(|node| self.game_object_from_node(node))
    }

    pub fn descendant_nodes(&self, node: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        Bfs::new(&self.arena, node).iter(&self.arena)
    }

    pub fn is_descendant(&self, parent: GameObject, game_object: GameObject) -> bool {
        std::iter::once(parent)
            .chain(self.descendants(parent))
            .any(|go| go == game_object)
    }

    pub fn ancestors(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        std::iter::successors(self.parent(game_object), |go| self.parent(*go))
    }

    pub fn objects(&self, root: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        Bfs::new(&self.arena, root.node)
            .iter(&self.arena)
            .skip(1)
            .filter_map(|n| self.game_object_from_node(n))
    }

    pub fn next_edge_index(&self, parent: GameObject) -> i32 {
        self.children(parent).count() as i32
    }

    pub fn add_edge(&mut self, parent: NodeIndex, child: NodeIndex, weight: i32) {
        self.arena.add_edge(parent, child, weight);
    }

    pub fn remove_node(&mut self, node: NodeIndex) {
        self.arena.remove_node(node);
    }

    pub fn index_in_parent(
        &self,
        parent: GameObject,
        sibling: GameObject,
        dir: SiblingDir,
    ) -> Option<i32> {
        let edge = self
            .arena
            .edges_directed(parent.node, Direction::Outgoing)
            .find(|edge| {
                self.game_object_from_node(edge.target())
                    .map(|go| go == sibling)
                    .unwrap_or(false)
            });
        edge.map(|edge| match dir {
            SiblingDir::Before => *edge.weight(),
            SiblingDir::After => *edge.weight() + 1,
        })
    }

    pub fn shift_edge_weights(&mut self, parent: GameObject, start: i32, offset: i32) {
        let mut walker = self.arena.neighbors(parent.node).detach();
        while let Some((edge, _)) = walker.next(&self.arena) {
            if let Some(edge_weight) = self.arena.edge_weight_mut(edge) {
                if *edge_weight >= start {
                    *edge_weight += offset;
                }
            }
        }
    }

    pub fn swap_edge_weights(&mut self, parent: GameObject, first: i32, second: i32) {
        let find_edge = |weight: i32| {
            self.arena
                .edges_directed(parent.node, Direction::Outgoing)
                .find_map(|edge| {
                    if *edge.weight() == weight {
                        Some(edge.id())
                    } else {
                        None
                    }
                })
        };
        let Some(first_edge) = find_edge(first) else {
            return;
        };
        let Some(second_edge) = find_edge(second) else {
            return;
        };
        self.arena[first_edge] = second;
        self.arena[second_edge] = first;
    }

    pub fn set_parent(
        &mut self,
        game_object: GameObject,
        parent: GameObject,
        sibling: Option<(GameObject, SiblingDir)>,
    ) {
        let mut insert_index = None;
        if let Some((sibling, dir)) = sibling {
            if let Some(index) = self.index_in_parent(parent, sibling, dir) {
                if let Some(current) = self.index_in_parent(parent, game_object, SiblingDir::Before)
                {
                    self.swap_edge_weights(parent, current, index);
                    return;
                } else {
                    self.shift_edge_weights(parent, index, 1);
                }
                insert_index = Some(index);
            }
        }

        if let Some((parent, edge)) = self.parent(game_object).and_then(|parent| {
            self.arena
                .find_edge(parent.node, game_object.node)
                .map(|edge| (parent, edge))
        }) {
            let index = self.arena[edge];
            self.arena.remove_edge(edge);
            self.shift_edge_weights(parent, index, -1);
        }
        let insert_index = insert_index.unwrap_or_else(|| self.next_edge_index(parent));
        self.arena
            .add_edge(parent.node, game_object.node, insert_index);
    }
}
