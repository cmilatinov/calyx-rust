use glm::{Mat3, Mat4, Vec2, Vec3, Vec4};
use uuid::Uuid;
use engine_derive::impl_extern_type_uuid;
use crate as engine;

impl_extern_type_uuid!(Vec2, "c16b091f-edfa-46d5-8316-5eab5550fa34");
impl_extern_type_uuid!(Vec3, "6e70688c-98e9-4f09-9869-90be09c25f88");
impl_extern_type_uuid!(Vec4, "1ddc4442-5d57-4234-856b-26f6b2179c14");
impl_extern_type_uuid!(Mat3, "f7686e07-1e79-4fd4-a407-1477ddc7f541");
impl_extern_type_uuid!(Mat4, "2e1650e5-4718-40a9-9f82-fbe8d8048727");

impl_extern_type_uuid!(bool, "011e2cb3-4db1-4745-aea6-bc4d813b266c");
impl_extern_type_uuid!(u8, "b15efc01-71d9-48a1-af13-e29c97709d91");
impl_extern_type_uuid!(u16, "73fd2550-b985-43a0-b531-2da557413920");
impl_extern_type_uuid!(u32, "b002c3ec-393b-4536-a7dd-cb63d49f817a");
impl_extern_type_uuid!(u64, "342b5009-fa3e-419c-8816-d0227c01eda7");
impl_extern_type_uuid!(u128, "c95caaf5-2669-4497-ba25-582c2cd1e746");
impl_extern_type_uuid!(i8, "9a44fd4c-d2a0-4349-ad11-d2382e362ffe");
impl_extern_type_uuid!(i16, "4e47c476-2053-4eb3-a96b-2f6e5b703908");
impl_extern_type_uuid!(i32, "e204c3c3-672d-4928-9efa-b76d5b3ea4a5");
impl_extern_type_uuid!(i64, "8047457b-6f15-40f2-a773-2d043fe0df92");
impl_extern_type_uuid!(i128, "59f2c247-dd41-43a0-bb1b-a9add287efcb");
impl_extern_type_uuid!(usize, "14317c1e-9974-457c-ae7b-fb91d56247de");
impl_extern_type_uuid!(isize, "cf4b886c-1ec9-4892-8c12-9eea9dcfca8b");
impl_extern_type_uuid!(f32, "accb9256-833e-4b7e-9b93-4b5d90aafd6c");
impl_extern_type_uuid!(f64, "b097ab57-bde8-4007-8181-268f8d90a4b0");
impl_extern_type_uuid!(String, "e6fe8a72-0495-4877-88e4-6d9f8062b8f2");

impl_extern_type_uuid!(Uuid, "26e09929-d7bb-4928-bed4-49bf8e62d5af");
