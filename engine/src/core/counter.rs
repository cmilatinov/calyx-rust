use num_traits::PrimInt;

pub struct Counter<T: Default + PrimInt> {
    next: T,
}

impl<T: Default + PrimInt> Counter<T> {
    pub fn next(&mut self) -> T {
        let value = self.next;
        self.next = self.next.checked_add(&1).unwrap_or_default();
        value
    }
}
