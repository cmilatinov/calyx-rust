use engine::uuid::Uuid;
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionType {
    GameObject,
    Asset,
    AnimationNode(Uuid),
    AnimationTransition(Uuid),
    #[default]
    None,
}

#[derive(Default, Debug)]
pub struct Selection {
    ty: SelectionType,
    set: HashSet<Uuid>,
}

impl Selection {
    pub fn none() -> Self {
        Default::default()
    }

    pub fn from_id(ty: SelectionType, id: Uuid) -> Self {
        Self {
            ty,
            set: [id].into(),
        }
    }

    pub fn from_iter(ty: SelectionType, iter: impl Iterator<Item = Uuid>) -> Self {
        Self {
            ty,
            set: iter.collect(),
        }
    }

    pub fn is(&self, ty: SelectionType) -> bool {
        self.ty == ty
    }

    pub fn contains(&self, ty: SelectionType, id: Uuid) -> bool {
        self.is(ty) && self.set.contains(&id)
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = Uuid> + 'a {
        self.set.iter().copied()
    }

    pub fn first(&self, ty: SelectionType) -> Option<Uuid> {
        if self.is(ty) {
            self.iter().next()
        } else {
            None
        }
    }

    pub fn last(&self, ty: SelectionType) -> Option<Uuid> {
        if self.is(ty) {
            self.iter().last()
        } else {
            None
        }
    }

    pub fn ty(&self) -> SelectionType {
        self.ty
    }
}

impl Deref for Selection {
    type Target = HashSet<Uuid>;

    fn deref(&self) -> &Self::Target {
        &self.set
    }
}

impl DerefMut for Selection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.set
    }
}
