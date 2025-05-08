#![no_std]

extern crate errors;
extern crate macros;
extern crate ptr;
extern crate raw;
extern crate result;
extern crate slice_ext;
extern crate try_clone;

use core::convert::{AsMut, AsRef};
use core::marker::PhantomData;
use core::mem::{drop, needs_drop, size_of};
use core::ops::{Deref, DerefMut, Index, IndexMut, Range, RangeFrom, RangeFull, RangeTo};
use core::ptr::{drop_in_place, null_mut, read, write, write_bytes};
use core::slice::{from_raw_parts, from_raw_parts_mut};
use errors::*;
use macros::prelude::*;
use ptr::Ptr;
use raw::{AsRaw, AsRawMut};
use result::Result;
use slice_ext::SliceExt;
use try_clone::TryClone;

const VEC_MIN_SIZE: usize = 64;

pub struct Vec<T> {
    value: Ptr<u8>,
    capacity: usize,
    elements: usize,
    _marker: PhantomData<T>,
}

/*
impl<T: Display> Display for Vec<T> {
    fn format(&self, f: &mut Formatter) -> Result<()> {
        let mut first = true;
        for x in self {
            if first {
                writef!(f, "[{}", x)?;
            } else {
                writef!(f, ", {}", x)?;
            }
            first = false;
        }
        if first {
            writef!(f, "[")?;
        }
        writef!(f, "]")
    }
}
*/

impl<T> AsRef<[T]> for Vec<T> {
    fn as_ref(&self) -> &[T] {
        self.slice_all()
    }
}

impl<T> AsMut<[T]> for Vec<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self.slice_mut_all()
    }
}

impl<T> Deref for Vec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> DerefMut for Vec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<T: TryClone> TryClone for Vec<T> {
    fn try_clone(&self) -> Result<Self>
    where
        Self: Sized,
    {
        // Allocate new Vec with same capacity
        let mut v = Vec::with_capacity(self.capacity)?;

        // Clone elements one by one
        let mut i = 0;
        while i < self.elements {
            match self[i].try_clone() {
                Ok(cloned) => {
                    // Write cloned element to uninitialized slot
                    unsafe {
                        let dest_ptr = v.value.as_ptr() as *mut u8;
                        let dest_ptr = dest_ptr.add(size_of::<T>() * i) as *mut T;
                        write(dest_ptr, cloned);
                    }
                    i += 1;
                    v.elements = i; // Update elements for drop safety
                }
                Err(e) => {
                    // Drop cloned elements and return error
                    v.elements = i; // Ensure only cloned elements are dropped
                    drop(v);
                    return Err(e);
                }
            }
        }
        Ok(v)
    }
}

impl<T: PartialEq> PartialEq for Vec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        if self.len() != other.len() {
            false
        } else {
            for i in 0..self.len() {
                if self[i] != other[i] {
                    return false;
                }
            }
            true
        }
    }
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        let raw = self.value.as_ptr();
        if !raw.is_null() {
            if needs_drop::<T>() {
                for i in 0..self.elements {
                    unsafe {
                        let ptr = (raw as *const u8).add(i * size_of::<T>()) as *mut T;
                        drop_in_place(ptr);
                    }
                }
            }
            unsafe {
                ffi::release(raw as *const u8);
            }
        }
    }
}

impl<T> Index<Range<usize>> for Vec<T> {
    type Output = [T];
    fn index(&self, r: Range<usize>) -> &Self::Output {
        let slice = self.slice(r.start, r.end);
        &slice
    }
}

impl<T> IndexMut<Range<usize>> for Vec<T> {
    fn index_mut(&mut self, r: Range<usize>) -> &mut <Self as Index<Range<usize>>>::Output {
        self.slice_mut(r.start, r.end)
    }
}

impl<T> Index<RangeFrom<usize>> for Vec<T> {
    type Output = [T];
    fn index(&self, r: RangeFrom<usize>) -> &Self::Output {
        self.slice(r.start, self.len())
    }
}

impl<T> IndexMut<RangeFrom<usize>> for Vec<T> {
    fn index_mut(&mut self, r: RangeFrom<usize>) -> &mut Self::Output {
        self.slice_mut(r.start, self.len())
    }
}

impl<T> Index<RangeTo<usize>> for Vec<T> {
    type Output = [T];
    fn index(&self, r: RangeTo<usize>) -> &Self::Output {
        self.slice(0, r.end)
    }
}

impl<T> IndexMut<RangeTo<usize>> for Vec<T> {
    fn index_mut(&mut self, r: RangeTo<usize>) -> &mut Self::Output {
        self.slice_mut(0, r.end)
    }
}

impl<T> Index<RangeFull> for Vec<T> {
    type Output = [T];
    fn index(&self, _r: RangeFull) -> &Self::Output {
        self.slice(0, self.len())
    }
}

impl<T> IndexMut<RangeFull> for Vec<T> {
    fn index_mut(&mut self, _r: RangeFull) -> &mut Self::Output {
        self.slice_mut(0, self.len())
    }
}

impl<T> Index<usize> for Vec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index >= self.elements as usize {
            exit!("array index out of bounds!");
        }

        unsafe {
            let target = self.value.as_ptr() as *const T;
            let target = target.add(index);
            &*(target as *const T)
        }
    }
}

impl<T> IndexMut<usize> for Vec<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.elements as usize {
            exit!("array index out of bounds!");
        }

        unsafe {
            let target = self.value.as_ptr() as *const T;
            let target = target.add(index);
            &mut *(target as *mut T)
        }
    }
}

pub struct VecIterator<T> {
    vec: Vec<T>,
    index: usize,
    len: usize,
}

impl<T> Iterator for VecIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let size = size_of::<T>();
        if self.index < self.len {
            let ptr = self.vec.value.as_ptr() as *const u8;
            let ptr = unsafe { ptr.add(self.index * size) as *mut T };
            let element = unsafe { read(ptr) }; // Move the element out
            self.index += 1;
            Some(element)
        } else {
            None
        }
    }
}

impl<T> Drop for VecIterator<T> {
    fn drop(&mut self) {
        if self.vec.value.as_ptr().is_null() {
            return;
        }
        if needs_drop::<T>() {
            let size = size_of::<T>();
            while self.index < self.vec.elements {
                let ptr = unsafe { self.vec.value.as_ptr().add(self.index * size) as *mut T };
                unsafe { drop_in_place(ptr) };
                self.index += 1;
            }
        }
        self.vec.elements = 0;
    }
}

impl<T> IntoIterator for Vec<T> {
    type Item = T;
    type IntoIter = VecIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        let len = self.elements;
        let ret = VecIterator {
            vec: self,
            index: 0,
            len,
        };
        ret
    }
}

pub struct VecRefMutIterator<'a, T> {
    vec: &'a mut Vec<T>,
    index: usize,
}

impl<'a, T> Iterator for VecRefMutIterator<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.elements && !self.vec.value.as_ptr().is_null() {
            unsafe {
                let ptr = self.vec.value.as_ptr() as *mut T;
                let item_ptr = ptr.add(self.index);
                self.index += 1;
                Some(&mut *item_ptr)
            }
        } else {
            None
        }
    }
}

pub struct VecRefIterator<'a, T> {
    vec: &'a Vec<T>,
    index: usize,
}

impl<'a, T> Iterator for VecRefIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.elements && !self.vec.value.as_ptr().is_null() {
            unsafe {
                let ptr = self.vec.value.as_ptr() as *const T;
                let item_ptr = ptr.add(self.index);
                self.index += 1;
                Some(&*item_ptr)
            }
        } else {
            None
        }
    }
}

impl<'a, T> IntoIterator for &'a Vec<T> {
    type Item = &'a T;
    type IntoIter = VecRefIterator<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        VecRefIterator {
            vec: &self,
            index: 0,
        }
    }
}

impl<T: Copy> Vec<T> {
    pub fn resize(&mut self, n: usize) -> Result<()> {
        self.resize_impl(n)?;
        self.elements = n;
        Ok(())
    }
}

impl<T> Vec<T> {
    pub fn new() -> Self {
        let value = Ptr::null();
        let capacity = 0;
        let elements = 0;

        Self {
            value,
            capacity,
            elements,
            _marker: PhantomData,
        }
    }

    pub fn with_capacity(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Ok(Self::new());
        }
        let ptr = unsafe { ffi::alloc(capacity * size_of::<T>()) };
        if ptr.is_null() {
            err!(Alloc)
        } else {
            Ok(Self {
                value: Ptr::new(ptr as *const u8),
                capacity,
                elements: 0,
                _marker: PhantomData,
            })
        }
    }

    pub fn allow_zero_alloc(&mut self, v: bool) {
        self.value.set_bit(v);
    }

    pub fn push(&mut self, v: T) -> Result<()> {
        let size = size_of::<T>();

        if self.elements + 1 > self.capacity {
            self.resize_impl(self.elements + 1)?;
        }

        let dest_ptr = self.value.as_mut_ptr() as *mut u8;
        unsafe {
            let dest_ptr = dest_ptr.add(size * self.elements) as *mut T;
            write(dest_ptr, v);
        }
        self.elements += 1;

        Ok(())
    }

    pub fn extend(&mut self, v: &Vec<T>) -> Result<()>
    where
        T: Copy,
    {
        self.extend_from_slice(v.slice_all())
    }

    pub unsafe fn force_resize(&mut self, n: usize) -> Result<()> {
        self.resize_impl(n)?;
        self.elements = n;
        Ok(())
    }

    pub fn extend_from_slice(&mut self, other: &[T]) -> Result<()>
    where
        T: Copy,
    {
        let len = self.len();
        let other_len = other.len();
        self.resize_impl(other_len + len)?;
        self.elements = other_len + len;
        self.as_mut()[len..len + other_len].slice_copy(&other)
    }

    pub fn iter_mut(&mut self) -> VecRefMutIterator<'_, T> {
        VecRefMutIterator {
            vec: self,
            index: 0,
        }
    }

    pub fn clear(&mut self) {
        let _ = self.truncate(0);
        let _ = self.resize_impl(0);
    }

    pub fn truncate(&mut self, n: usize) -> Result<()> {
        if n > self.elements {
            return err!(IllegalArgument);
        }

        // Drop elements from n to self.elements
        if needs_drop::<T>() {
            for i in n..self.elements {
                unsafe {
                    let ptr = self.value.as_mut_ptr().add(i * size_of::<T>()) as *mut T;
                    drop_in_place(ptr);
                }
            }
        }

        self.elements = n;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.elements
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.value.as_mut_ptr() as *mut T
    }

    pub fn as_ptr(&self) -> *const T {
        self.value.as_ptr() as *const T
    }

    pub fn slice(&self, start: usize, end: usize) -> &[T] {
        if start > end || end > self.elements {
            exit!(
                "Slice out of bounds: {}..{} > {}",
                start,
                end,
                self.elements
            );
        } else if start == end {
            &[]
        } else {
            let size = size_of::<T>();
            unsafe {
                from_raw_parts(
                    self.value.as_ptr().add(start * size) as *const T,
                    end - start,
                )
            }
        }
    }

    pub fn slice_mut(&mut self, start: usize, end: usize) -> &mut [T] {
        if start > end || end > self.elements {
            exit!(
                "Slice out of bounds: {}..{} > {}",
                start,
                end,
                self.elements
            );
        } else if start == end {
            &mut []
        } else {
            let size = size_of::<T>();
            unsafe {
                from_raw_parts_mut(
                    self.value.as_mut_ptr().add(start * size) as *mut T,
                    end - start,
                )
            }
        }
    }

    pub fn slice_all(&self) -> &[T] {
        self.slice(0, self.len())
    }

    pub fn slice_mut_all(&mut self) -> &mut [T] {
        self.slice_mut(0, self.len())
    }

    pub fn slice_to(&self, end: usize) -> &[T] {
        self.slice(0, end)
    }

    pub fn slice_mut_to(&mut self, end: usize) -> &mut [T] {
        self.slice_mut(0, end)
    }

    pub fn slice_from(&self, start: usize) -> &[T] {
        self.slice(start, self.len())
    }

    pub fn slice_mut_from(&mut self, start: usize) -> &mut [T] {
        self.slice_mut(start, self.len())
    }

    fn next_power_of_two(&self, mut n: usize) -> usize {
        if self.value.get_bit() && n == 0 {
            return 0;
        }
        if n < VEC_MIN_SIZE {
            return VEC_MIN_SIZE;
        }
        if n == 0 {
            return 0;
        }
        n -= 1;
        n |= n >> 1;
        n |= n >> 2;
        n |= n >> 4;
        n |= n >> 8;
        n |= n >> 16;
        n |= n >> 32;
        n + 1
    }

    fn resize_impl(&mut self, needed: usize) -> Result<()> {
        let ncapacity = self.next_power_of_two(needed);

        if ncapacity == self.capacity {
            return Ok(());
        }

        let rptr = self.value.as_mut_ptr();

        let nptr = if ncapacity == 0 {
            if !rptr.is_null() {
                unsafe {
                    ffi::release(rptr as *const u8);
                }
            }
            null_mut()
        } else if rptr.is_null() {
            unsafe { ffi::alloc(ncapacity * size_of::<T>()) }
        } else {
            unsafe { ffi::resize(rptr as *const u8, ncapacity * size_of::<T>()) }
        };

        if !nptr.is_null() {
            if ncapacity > self.capacity {
                let old_size = self.capacity * size_of::<T>();
                let new_size = ncapacity * size_of::<T>();
                unsafe {
                    write_bytes((nptr as *mut u8).add(old_size), 0, new_size - old_size);
                }
            }
            self.capacity = ncapacity;
            let mut nptr = Ptr::new(nptr as *mut u8);
            if self.value.get_bit() {
                nptr.set_bit(true);
            }
            if self.value.as_ptr().is_null() {
                self.value = nptr;
            } else {
                self.value = nptr;
            }
            Ok(())
        } else {
            self.value = Ptr::null();
            if ncapacity == 0 {
                Ok(())
            } else {
                err!(Alloc)
            }
        }
    }
}
