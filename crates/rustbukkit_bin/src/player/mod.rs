mod configuration;
mod connection_state;
mod join_game;
mod movement_handler;
mod play_state;
mod player_data;

use std::borrow::{Borrow, BorrowMut};
use std::fmt::{Debug, Display};
use std::ops::{Add, Deref};

pub use play_state::PlayStateHandler;
pub use player_data::PlayerData;

pub trait CrossAssign<Rhs = Self> {
    fn cross_assign(&mut self, rhs: Rhs);
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Vec3<N> {
    pub x: N,
    pub y: N,
    pub z: N,
}

impl<N> From<&[N; 3]> for Vec3<N>
where
    N: Copy,
{
    fn from(array: &[N; 3]) -> Self {
        Self {
            x: array[0],
            y: array[1],
            z: array[2],
        }
    }
}

impl<N> CrossAssign for Vec3<N>
where
    N: Copy,
{
    fn cross_assign(&mut self, rhs: Self) {
        self.x = rhs.x;
        self.y = rhs.y;
        self.z = rhs.z;
    }
}

impl<N> CrossAssign for &mut Vec3<N>
where
    N: Copy,
{
    fn cross_assign(&mut self, rhs: Self) {
        self.x = rhs.x;
        self.y = rhs.y;
        self.z = rhs.z;
    }
}

impl<T> Deref for Vec3<T> {
    type Target = [T; 3];

    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // This conversion holds because Vec3 has the same memory layout as [T; 3]
        // we're not storing them as heap allocatioons so there is no pointer indirection to worry about
        #[allow(unsafe_code)]
        unsafe {
            &*(self as *const Vec3<T> as *const [T; 3])
        }
    }
}

impl<N> Vec3<N> {
    pub fn new(x: N, y: N, z: N) -> Self {
        Self { x, y, z }
    }
}

impl<N> From<(N, N, N)> for Vec3<N> {
    fn from(tuple: (N, N, N)) -> Self {
        Self {
            x: tuple.0,
            y: tuple.1,
            z: tuple.2,
        }
    }
}

impl<N> Add for Vec3<N>
where
    N: Add<Output = N>,
{
    type Output = Vec3<N>;

    fn add(self, rhs: Self) -> Self::Output {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl<N> Add<&Vec3<N>> for Vec3<N>
where
    N: Add<Output = N> + Copy,
{
    type Output = Vec3<N>;

    fn add(self, rhs: &Vec3<N>) -> Self::Output {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl<N> Add<Vec3<N>> for &mut Vec3<N>
where
    N: Add<Output = N> + Copy,
{
    type Output = Vec3<N>;

    fn add(self, rhs: Vec3<N>) -> Self::Output {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl<N> AsRef<[N; 3]> for Vec3<N> {
    fn as_ref(&self) -> &[N; 3] {
        #[allow(unsafe_code)]
        unsafe {
            //
            &*(self as *const Vec3<N> as *const [N; 3])
        }
    }
}

impl<N> Borrow<[N; 3]> for Vec3<N> {
    fn borrow(&self) -> &[N; 3] {
        self.as_ref()
    }
}

impl<N> BorrowMut<[N; 3]> for Vec3<N> {
    fn borrow_mut(&mut self) -> &mut [N; 3] {
        #[allow(unsafe_code)]
        unsafe {
            //
            &mut *(self as *mut Vec3<N> as *mut [N; 3])
        }
    }
}

impl<N: Display> Debug for Vec3<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vec3 {{ x: {:.2}, y: {:.2}, z: {:.2} }}", self.x, self.y, self.z)
    }
}

impl<N: Display> Display for Vec3<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.2}, {:.2}, {:.2})", self.x, self.y, self.z)
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Vec2<N> {
    pub yaw:   N,
    pub pitch: N,
}

impl<T> Deref for Vec2<T> {
    type Target = [T; 2];

    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // This conversion holds because Vec2 has the same memory layout as [T; 2]
        // we're not storing them as heap allocatioons so there is no pointer indirection to worry about
        #[allow(unsafe_code)]
        unsafe {
            &*(self as *const Vec2<T> as *const [T; 2])
        }
    }
}

impl<N> CrossAssign for Vec2<N>
where
    N: Copy,
{
    fn cross_assign(&mut self, rhs: Self) {
        self.yaw = rhs.yaw;
        self.pitch = rhs.pitch;
    }
}

impl<N> CrossAssign for &mut Vec2<N>
where
    N: Copy,
{
    fn cross_assign(&mut self, rhs: Self) {
        self.yaw = rhs.yaw;
        self.pitch = rhs.pitch;
    }
}

impl<N> Vec2<N> {
    pub fn new(yaw: N, pitch: N) -> Self {
        Self { yaw, pitch }
    }
}

impl<N> From<(N, N)> for Vec2<N> {
    fn from(tuple: (N, N)) -> Self {
        Self {
            yaw:   tuple.0,
            pitch: tuple.1,
        }
    }
}

impl<N> Add for Vec2<N>
where
    N: Add<Output = N>,
{
    type Output = Vec2<N>;

    fn add(self, rhs: Self) -> Self::Output {
        Vec2 {
            yaw:   self.yaw + rhs.yaw,
            pitch: self.pitch + rhs.pitch,
        }
    }
}

impl<N> Add<&Vec2<N>> for Vec2<N>
where
    N: Add<Output = N> + Copy,
{
    type Output = Vec2<N>;

    fn add(self, rhs: &Vec2<N>) -> Self::Output {
        Vec2 {
            yaw:   self.yaw + rhs.yaw,
            pitch: self.pitch + rhs.pitch,
        }
    }
}

impl<N> Add<Vec2<N>> for &mut Vec2<N>
where
    N: Add<Output = N> + Copy,
{
    type Output = Vec2<N>;

    fn add(self, rhs: Vec2<N>) -> Self::Output {
        Vec2 {
            yaw:   self.yaw + rhs.yaw,
            pitch: self.pitch + rhs.pitch,
        }
    }
}

impl<N> AsRef<[N; 2]> for Vec2<N> {
    fn as_ref(&self) -> &[N; 2] {
        #[allow(unsafe_code)]
        unsafe {
            //
            &*(self as *const Vec2<N> as *const [N; 2])
        }
    }
}

impl<N> Borrow<[N; 2]> for Vec2<N> {
    fn borrow(&self) -> &[N; 2] {
        self.as_ref()
    }
}

impl<N> BorrowMut<[N; 2]> for Vec2<N> {
    fn borrow_mut(&mut self) -> &mut [N; 2] {
        #[allow(unsafe_code)]
        unsafe {
            //
            &mut *(self as *mut Vec2<N> as *mut [N; 2])
        }
    }
}

impl<N: Display> Debug for Vec2<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vec2 {{ yaw: {:.2}, pitch: {:.2} }}", self.yaw, self.pitch)
    }
}

impl<N: Display> Display for Vec2<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.2}, {:.2})", self.yaw, self.pitch)
    }
}
