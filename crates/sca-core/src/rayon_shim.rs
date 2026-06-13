//! Single-threaded shim for `rayon::prelude::*` on wasm32.
//!
//! Browsers don't expose POSIX threads to wasm32-unknown-unknown, so rayon's
//! work-stealing pool can't run there. This shim provides the same method
//! names (`par_iter`, `into_par_iter`, `par_iter_mut`) that resolve to the
//! standard sequential iterators.
//!
//! Native builds use `rayon::prelude::*` unchanged via the conditional
//! re-export in `lib.rs`. Call sites are identical on both targets.

#![cfg(target_arch = "wasm32")]

use std::ops::Range;

pub trait IntoParallelRefIterator<'a> {
    type Iter: Iterator + 'a;
    fn par_iter(&'a self) -> Self::Iter;
}

pub trait IntoParallelRefMutIterator<'a> {
    type Iter: Iterator + 'a;
    fn par_iter_mut(&'a mut self) -> Self::Iter;
}

pub trait IntoParallelIterator {
    type Iter: Iterator;
    fn into_par_iter(self) -> Self::Iter;
}

impl<'a, T: 'a> IntoParallelRefIterator<'a> for [T] {
    type Iter = std::slice::Iter<'a, T>;
    fn par_iter(&'a self) -> Self::Iter {
        self.iter()
    }
}

impl<'a, T: 'a> IntoParallelRefIterator<'a> for Vec<T> {
    type Iter = std::slice::Iter<'a, T>;
    fn par_iter(&'a self) -> Self::Iter {
        self.iter()
    }
}

impl<'a, T: 'a> IntoParallelRefMutIterator<'a> for Vec<T> {
    type Iter = std::slice::IterMut<'a, T>;
    fn par_iter_mut(&'a mut self) -> Self::Iter {
        self.iter_mut()
    }
}

impl<T> IntoParallelIterator for Vec<T> {
    type Iter = std::vec::IntoIter<T>;
    fn into_par_iter(self) -> Self::Iter {
        self.into_iter()
    }
}

impl<T> IntoParallelIterator for Range<T>
where
    Range<T>: Iterator,
{
    type Iter = Range<T>;
    fn into_par_iter(self) -> Self::Iter {
        self
    }
}

/// Drop-in replacement for `rayon::prelude::*` on wasm32.
pub mod prelude {
    pub use super::{
        IntoParallelIterator, IntoParallelRefIterator, IntoParallelRefMutIterator,
    };
}
