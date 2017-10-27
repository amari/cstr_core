// Copyright 2012 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![no_std]
#![cfg_attr(feature = "alloc", feature(alloc))]

#[cfg(test)]
#[macro_use]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;
extern crate cty;
extern crate memchr;

#[cfg(feature = "alloc")]
use alloc::borrow::{Borrow, Cow, ToOwned};
#[cfg(feature = "alloc")]
use alloc::{String, Vec};
#[cfg(feature = "alloc")]
use alloc::boxed::Box;
#[cfg(feature = "alloc")]
use core::{mem, ops, ptr};

use core::cmp::Ordering;
use core::fmt::{self, Write};
use core::slice;
use core::str::{self, Utf8Error};

/// Re-export c_char
pub use cty::c_char;

#[inline]
unsafe fn strlen(p: *const c_char) -> usize {
    let mut n = 0;
    while *p.offset(n as isize) != 0 {
        n += 1;
    }
    n
}

mod ascii {
    use core::ops::Range;

    /// An iterator over the escaped version of a byte.
    ///
    /// This `struct` is created by the [`escape_default`] function. See its
    /// documentation for more.
    ///
    /// [`escape_default`]: fn.escape_default.html
    pub struct EscapeDefault {
        range: Range<usize>,
        data: [u8; 4],
    }

    /// Returns an iterator that produces an escaped version of a `u8`.
    ///
    /// The default is chosen with a bias toward producing literals that are
    /// legal in a variety of languages, including C++11 and similar C-family
    /// languages. The exact rules are:
    ///
    /// - Tab, CR and LF are escaped as '\t', '\r' and '\n' respectively.
    /// - Single-quote, double-quote and backslash chars are backslash-escaped.
    /// - Any other chars in the range [0x20,0x7e] are not escaped.
    /// - Any other chars are given hex escapes of the form '\xNN'.
    /// - Unicode escapes are never generated by this function.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ascii;
    ///
    /// let escaped = ascii::escape_default(b'0').next().unwrap();
    /// assert_eq!(b'0', escaped);
    ///
    /// let mut escaped = ascii::escape_default(b'\t');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b't', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'\r');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'r', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'\n');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'n', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'\'');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'\'', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'"');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'"', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'\\');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'\\', escaped.next().unwrap());
    ///
    /// let mut escaped = ascii::escape_default(b'\x9d');
    ///
    /// assert_eq!(b'\\', escaped.next().unwrap());
    /// assert_eq!(b'x', escaped.next().unwrap());
    /// assert_eq!(b'9', escaped.next().unwrap());
    /// assert_eq!(b'd', escaped.next().unwrap());
    /// ```
    pub fn escape_default(c: u8) -> EscapeDefault {
        let (data, len) = match c {
            b'\t' => ([b'\\', b't', 0, 0], 2),
            b'\r' => ([b'\\', b'r', 0, 0], 2),
            b'\n' => ([b'\\', b'n', 0, 0], 2),
            b'\\' => ([b'\\', b'\\', 0, 0], 2),
            b'\'' => ([b'\\', b'\'', 0, 0], 2),
            b'"' => ([b'\\', b'"', 0, 0], 2),
            b'\x20'...b'\x7e' => ([c, 0, 0, 0], 1),
            _ => ([b'\\', b'x', hexify(c >> 4), hexify(c & 0xf)], 4),
        };

        return EscapeDefault {
            range: (0..len),
            data: data,
        };

        fn hexify(b: u8) -> u8 {
            match b {
                0...9 => b'0' + b,
                _ => b'a' + b - 10,
            }
        }
    }

    impl Iterator for EscapeDefault {
        type Item = u8;
        fn next(&mut self) -> Option<u8> {
            self.range.next().map(|i| self.data[i])
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.range.size_hint()
        }
    }
    impl DoubleEndedIterator for EscapeDefault {
        fn next_back(&mut self) -> Option<u8> {
            self.range.next_back().map(|i| self.data[i])
        }
    }
    impl ExactSizeIterator for EscapeDefault {}
}

/// A type representing an owned C-compatible string.
///
/// This type serves the primary purpose of being able to safely generate a
/// C-compatible string from a Rust byte slice or vector. An instance of this
/// type is a static guarantee that the underlying bytes contain no interior 0
/// bytes and the final byte is 0.
///
/// A `CString` is created from either a byte slice or a byte vector. A [`u8`]
/// slice can be obtained with the `as_bytes` method. Slices produced from a
/// `CString` do *not* contain the trailing nul terminator unless otherwise
/// specified.
///
/// [`u8`]: ../primitive.u8.html
///
/// # Examples
///
/// ```no_run
/// # fn main() {
/// use cstr_core::CString;
/// use cstr_core::c_char;
///
/// extern {
///     fn my_printer(s: *const c_char);
/// }
///
/// let c_to_print = CString::new("Hello, world!").unwrap();
/// unsafe {
///     my_printer(c_to_print.as_ptr());
/// }
/// # }
/// ```
///
/// # Safety
///
/// `CString` is intended for working with traditional C-style strings
/// (a sequence of non-null bytes terminated by a single null byte); the
/// primary use case for these kinds of strings is interoperating with C-like
/// code. Often you will need to transfer ownership to/from that external
/// code. It is strongly recommended that you thoroughly read through the
/// documentation of `CString` before use, as improper ownership management
/// of `CString` instances can lead to invalid memory accesses, memory leaks,
/// and other memory errors.

#[cfg(feature = "alloc")]
#[derive(PartialEq, PartialOrd, Eq, Ord, Hash, Clone)]
pub struct CString {
    // Invariant 1: the slice ends with a zero byte and has a length of at least one.
    // Invariant 2: the slice contains only one zero byte.
    // Improper usage of unsafe function can break Invariant 2, but not Invariant 1.
    inner: Box<[u8]>,
}

/// Representation of a borrowed C string.
///
/// This dynamically sized type is only safely constructed via a borrowed
/// version of an instance of `CString`. This type can be constructed from a raw
/// C string as well and represents a C string borrowed from another location.
///
/// Note that this structure is **not** `repr(C)` and is not recommended to be
/// placed in the signatures of FFI functions. Instead safe wrappers of FFI
/// functions may leverage the unsafe [`from_ptr`] constructor to provide a safe
/// interface to other consumers.
///
/// [`from_ptr`]: #method.from_ptr
///
/// # Examples
///
/// Inspecting a foreign C string:
///
/// ```no_run
/// use cstr_core::CStr;
/// use cstr_core::c_char;
///
/// extern { fn my_string() -> *const c_char; }
///
/// unsafe {
///     let slice = CStr::from_ptr(my_string());
///     println!("string length: {}", slice.to_bytes().len());
/// }
/// ```
///
/// Passing a Rust-originating C string:
///
/// ```no_run
/// use cstr_core::{CString, CStr};
/// use cstr_core::c_char;
///
/// fn work(data: &CStr) {
///     extern { fn work_with(data: *const c_char); }
///
///     unsafe { work_with(data.as_ptr()) }
/// }
///
/// let s = CString::new("data data data data").unwrap();
/// work(&s);
/// ```
///
/// Converting a foreign C string into a Rust [`String`]:
///
/// [`String`]: ../string/struct.String.html
///
/// ```no_run
/// use cstr_core::CStr;
/// use cstr_core::c_char;
///
/// extern { fn my_string() -> *const c_char; }
///
/// fn my_string_safe() -> String {
///     unsafe {
///         CStr::from_ptr(my_string()).to_string_lossy().into_owned()
///     }
/// }
///
/// println!("string: {}", my_string_safe());
/// ```
#[derive(Hash)]
pub struct CStr {
    // FIXME: this should not be represented with a DST slice but rather with
    //        just a raw `c_char` along with some form of marker to make
    //        this an unsized type. Essentially `sizeof(&CStr)` should be the
    //        same as `sizeof(&c_char)` but `CStr` should be an unsized type.
    inner: [c_char],
}

/// An error returned from [`CString::new`] to indicate that a nul byte was found
/// in the vector provided.
///
/// [`CString::new`]: struct.CString.html#method.new
///
/// # Examples
///
/// ```
/// use cstr_core::{CString, NulError};
///
/// let _: NulError = CString::new(b"f\0oo".to_vec()).unwrap_err();
/// ```
#[cfg(feature = "alloc")]
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NulError(usize, Vec<u8>);

/// An error returned from [`CStr::from_bytes_with_nul`] to indicate that a nul
/// byte was found too early in the slice provided or one wasn't found at all.
///
/// [`CStr::from_bytes_with_nul`]: struct.CStr.html#method.from_bytes_with_nul
///
/// # Examples
///
/// ```
/// use cstr_core::{CStr, FromBytesWithNulError};
///
/// let _: FromBytesWithNulError = CStr::from_bytes_with_nul(b"f\0oo").unwrap_err();
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FromBytesWithNulError {
    kind: FromBytesWithNulErrorKind,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum FromBytesWithNulErrorKind {
    InteriorNul(usize),
    NotNulTerminated,
}

impl FromBytesWithNulError {
    fn interior_nul(pos: usize) -> FromBytesWithNulError {
        FromBytesWithNulError {
            kind: FromBytesWithNulErrorKind::InteriorNul(pos),
        }
    }
    fn not_nul_terminated() -> FromBytesWithNulError {
        FromBytesWithNulError {
            kind: FromBytesWithNulErrorKind::NotNulTerminated,
        }
    }
}

/// An error returned from [`CString::into_string`] to indicate that a UTF-8 error
/// was encountered during the conversion.
///
/// [`CString::into_string`]: struct.CString.html#method.into_string
#[cfg(feature = "alloc")]
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntoStringError {
    inner: CString,
    error: Utf8Error,
}

#[cfg(feature = "alloc")]
impl CString {
    /// Creates a new C-compatible string from a container of bytes.
    ///
    /// This method will consume the provided data and use the underlying bytes
    /// to construct a new string, ensuring that there is a trailing 0 byte.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cstr_core::CString;
    /// use cstr_core::c_char;
    ///
    /// extern { fn puts(s: *const c_char); }
    ///
    /// let to_print = CString::new("Hello!").unwrap();
    /// unsafe {
    ///     puts(to_print.as_ptr());
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// This function will return an error if the bytes yielded contain an
    /// internal 0 byte. The error returned will contain the bytes as well as
    /// the position of the nul byte.
    pub fn new<T: Into<Vec<u8>>>(t: T) -> Result<CString, NulError> {
        Self::_new(t.into())
    }

    fn _new(bytes: Vec<u8>) -> Result<CString, NulError> {
        match memchr::memchr(0, &bytes) {
            Some(i) => Err(NulError(i, bytes)),
            None => Ok(unsafe { CString::from_vec_unchecked(bytes) }),
        }
    }

    /// Creates a C-compatible string from a byte vector without checking for
    /// interior 0 bytes.
    ///
    /// This method is equivalent to [`new`] except that no runtime assertion
    /// is made that `v` contains no 0 bytes, and it requires an actual
    /// byte vector, not anything that can be converted to one with Into.
    ///
    /// [`new`]: #method.new
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let raw = b"foo".to_vec();
    /// unsafe {
    ///     let c_string = CString::from_vec_unchecked(raw);
    /// }
    /// ```
    pub unsafe fn from_vec_unchecked(mut v: Vec<u8>) -> CString {
        v.reserve_exact(1);
        v.push(0);
        CString {
            inner: v.into_boxed_slice(),
        }
    }

    /// Retakes ownership of a `CString` that was transferred to C.
    ///
    /// Additionally, the length of the string will be recalculated from the pointer.
    ///
    /// # Safety
    ///
    /// This should only ever be called with a pointer that was earlier
    /// obtained by calling [`into_raw`] on a `CString`. Other usage (e.g. trying to take
    /// ownership of a string that was allocated by foreign code) is likely to lead
    /// to undefined behavior or allocator corruption.
    ///
    /// [`into_raw`]: #method.into_raw
    ///
    /// # Examples
    ///
    /// Create a `CString`, pass ownership to an `extern` function (via raw pointer), then retake
    /// ownership with `from_raw`:
    ///
    /// ```no_run
    /// use cstr_core::CString;
    /// use cstr_core::c_char;
    ///
    /// extern {
    ///     fn some_extern_function(s: *mut c_char);
    /// }
    ///
    /// let c_string = CString::new("Hello!").unwrap();
    /// let raw = c_string.into_raw();
    /// unsafe {
    ///     some_extern_function(raw);
    ///     let c_string = CString::from_raw(raw);
    /// }
    /// ```
    pub unsafe fn from_raw(ptr: *mut c_char) -> CString {
        let len = strlen(ptr) + 1; // Including the NUL byte
        let slice = slice::from_raw_parts_mut(ptr, len as usize);
        CString {
            inner: Box::from_raw(slice as *mut [c_char] as *mut [u8]),
        }
    }

    /// Transfers ownership of the string to a C caller.
    ///
    /// The pointer must be returned to Rust and reconstituted using
    /// [`from_raw`] to be properly deallocated. Specifically, one
    /// should *not* use the standard C `free` function to deallocate
    /// this string.
    ///
    /// Failure to call [`from_raw`] will lead to a memory leak.
    ///
    /// [`from_raw`]: #method.from_raw
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new("foo").unwrap();
    ///
    /// let ptr = c_string.into_raw();
    ///
    /// unsafe {
    ///     assert_eq!(b'f', *ptr as u8);
    ///     assert_eq!(b'o', *ptr.offset(1) as u8);
    ///     assert_eq!(b'o', *ptr.offset(2) as u8);
    ///     assert_eq!(b'\0', *ptr.offset(3) as u8);
    ///
    ///     // retake pointer to free memory
    ///     let _ = CString::from_raw(ptr);
    /// }
    /// ```
    #[inline]
    pub fn into_raw(self) -> *mut c_char {
        Box::into_raw(self.into_inner()) as *mut c_char
    }

    /// Converts the `CString` into a [`String`] if it contains valid Unicode data.
    ///
    /// On failure, ownership of the original `CString` is returned.
    ///
    /// [`String`]: ../string/struct.String.html
    pub fn into_string(self) -> Result<String, IntoStringError> {
        String::from_utf8(self.into_bytes()).map_err(|e| {
            IntoStringError {
                error: e.utf8_error(),
                inner: unsafe { CString::from_vec_unchecked(e.into_bytes()) },
            }
        })
    }

    /// Returns the underlying byte buffer.
    ///
    /// The returned buffer does **not** contain the trailing nul separator and
    /// it is guaranteed to not have any interior nul bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new("foo").unwrap();
    /// let bytes = c_string.into_bytes();
    /// assert_eq!(bytes, vec![b'f', b'o', b'o']);
    /// ```
    pub fn into_bytes(self) -> Vec<u8> {
        let mut vec = self.into_inner().into_vec();
        let _nul = vec.pop();
        debug_assert_eq!(_nul, Some(0u8));
        vec
    }

    /// Equivalent to the [`into_bytes`] function except that the returned vector
    /// includes the trailing nul byte.
    ///
    /// [`into_bytes`]: #method.into_bytes
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new("foo").unwrap();
    /// let bytes = c_string.into_bytes_with_nul();
    /// assert_eq!(bytes, vec![b'f', b'o', b'o', b'\0']);
    /// ```
    pub fn into_bytes_with_nul(self) -> Vec<u8> {
        self.into_inner().into_vec()
    }

    /// Returns the contents of this `CString` as a slice of bytes.
    ///
    /// The returned slice does **not** contain the trailing nul separator and
    /// it is guaranteed to not have any interior nul bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new("foo").unwrap();
    /// let bytes = c_string.as_bytes();
    /// assert_eq!(bytes, &[b'f', b'o', b'o']);
    /// ```
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.inner[..self.inner.len() - 1]
    }

    /// Equivalent to the [`as_bytes`] function except that the returned slice
    /// includes the trailing nul byte.
    ///
    /// [`as_bytes`]: #method.as_bytes
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new("foo").unwrap();
    /// let bytes = c_string.as_bytes_with_nul();
    /// assert_eq!(bytes, &[b'f', b'o', b'o', b'\0']);
    /// ```
    #[inline]
    pub fn as_bytes_with_nul(&self) -> &[u8] {
        &self.inner
    }

    /// Extracts a [`CStr`] slice containing the entire string.
    ///
    /// [`CStr`]: struct.CStr.html
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::{CString, CStr};
    ///
    /// let c_string = CString::new(b"foo".to_vec()).unwrap();
    /// let c_str = c_string.as_c_str();
    /// assert_eq!(c_str, CStr::from_bytes_with_nul(b"foo\0").unwrap());
    /// ```
    #[inline]
    pub fn as_c_str(&self) -> &CStr {
        &*self
    }

    /// Converts this `CString` into a boxed [`CStr`].
    ///
    /// [`CStr`]: struct.CStr.html
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::{CString, CStr};
    ///
    /// let c_string = CString::new(b"foo".to_vec()).unwrap();
    /// let boxed = c_string.into_boxed_c_str();
    /// assert_eq!(&*boxed, CStr::from_bytes_with_nul(b"foo\0").unwrap());
    /// ```
    pub fn into_boxed_c_str(self) -> Box<CStr> {
        unsafe { Box::from_raw(Box::into_raw(self.into_inner()) as *mut CStr) }
    }

    // Bypass "move out of struct which implements [`Drop`] trait" restriction.
    ///
    /// [`Drop`]: ../ops/trait.Drop.html
    fn into_inner(self) -> Box<[u8]> {
        unsafe {
            let result = ptr::read(&self.inner);
            mem::forget(self);
            result
        }
    }
}

// Turns this `CString` into an empty string to prevent
// memory unsafe code from working by accident. Inline
// to prevent LLVM from optimizing it away in debug builds.
#[cfg(feature = "alloc")]
impl Drop for CString {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            *self.inner.get_unchecked_mut(0) = 0;
        }
    }
}

#[cfg(feature = "alloc")]
impl ops::Deref for CString {
    type Target = CStr;

    #[inline]
    fn deref(&self) -> &CStr {
        unsafe { CStr::from_bytes_with_nul_unchecked(self.as_bytes_with_nul()) }
    }
}

#[cfg(feature = "alloc")]
impl fmt::Debug for CString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[cfg(feature = "alloc")]
impl From<CString> for Vec<u8> {
    #[inline]
    fn from(s: CString) -> Vec<u8> {
        s.into_bytes()
    }
}

impl fmt::Debug for CStr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "\"")?;
        for byte in self.to_bytes()
            .iter()
            .flat_map(|&b| ascii::escape_default(b))
        {
            f.write_char(byte as char)?;
        }
        write!(f, "\"")
    }
}

impl<'a> Default for &'a CStr {
    fn default() -> &'a CStr {
        const SLICE: &'static [c_char] = &[0];
        unsafe { CStr::from_ptr(SLICE.as_ptr()) }
    }
}

#[cfg(feature = "alloc")]
impl Default for CString {
    /// Creates an empty `CString`.
    fn default() -> CString {
        let a: &CStr = Default::default();
        a.to_owned()
    }
}

#[cfg(feature = "alloc")]
impl Borrow<CStr> for CString {
    #[inline]
    fn borrow(&self) -> &CStr {
        self
    }
}

#[cfg(feature = "alloc")]
impl<'a> From<&'a CStr> for Box<CStr> {
    fn from(s: &'a CStr) -> Box<CStr> {
        let boxed: Box<[u8]> = Box::from(s.to_bytes_with_nul());
        unsafe { Box::from_raw(Box::into_raw(boxed) as *mut CStr) }
    }
}

#[cfg(feature = "alloc")]
impl From<Box<CStr>> for CString {
    #[inline]
    fn from(s: Box<CStr>) -> CString {
        s.into_c_string()
    }
}

#[cfg(feature = "alloc")]
impl From<CString> for Box<CStr> {
    #[inline]
    fn from(s: CString) -> Box<CStr> {
        s.into_boxed_c_str()
    }
}

#[cfg(feature = "alloc")]
impl Default for Box<CStr> {
    fn default() -> Box<CStr> {
        let boxed: Box<[u8]> = Box::from([0]);
        unsafe { Box::from_raw(Box::into_raw(boxed) as *mut CStr) }
    }
}

#[cfg(feature = "alloc")]
impl NulError {
    /// Returns the position of the nul byte in the slice that was provided to
    /// [`CString::new`].
    ///
    /// [`CString::new`]: struct.CString.html#method.new
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let nul_error = CString::new("foo\0bar").unwrap_err();
    /// assert_eq!(nul_error.nul_position(), 3);
    ///
    /// let nul_error = CString::new("foo bar\0").unwrap_err();
    /// assert_eq!(nul_error.nul_position(), 7);
    /// ```
    pub fn nul_position(&self) -> usize {
        self.0
    }

    /// Consumes this error, returning the underlying vector of bytes which
    /// generated the error in the first place.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let nul_error = CString::new("foo\0bar").unwrap_err();
    /// assert_eq!(nul_error.into_vec(), b"foo\0bar");
    /// ```
    pub fn into_vec(self) -> Vec<u8> {
        self.1
    }
}

#[cfg(feature = "alloc")]
impl fmt::Display for NulError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "nul byte found in provided data at position: {}", self.0)
    }
}

impl fmt::Display for FromBytesWithNulError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            FromBytesWithNulErrorKind::InteriorNul(..) => {
                f.write_str("data provided contains an interior nul byte")?
            }
            FromBytesWithNulErrorKind::NotNulTerminated => {
                f.write_str("data provided is not nul terminated")?
            }
        }
        if let FromBytesWithNulErrorKind::InteriorNul(pos) = self.kind {
            write!(f, " at byte pos {}", pos)?;
        }
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl IntoStringError {
    /// Consumes this error, returning original [`CString`] which generated the
    /// error.
    ///
    /// [`CString`]: struct.CString.html
    pub fn into_cstring(self) -> CString {
        self.inner
    }

    /// Access the underlying UTF-8 error that was the cause of this error.
    pub fn utf8_error(&self) -> Utf8Error {
        self.error
    }
}

#[cfg(feature = "alloc")]
impl fmt::Display for IntoStringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "C string contained non-utf8 bytes")
    }
}

impl CStr {
    /// Casts a raw C string to a safe C string wrapper.
    ///
    /// This function will cast the provided `ptr` to the `CStr` wrapper which
    /// allows inspection and interoperation of non-owned C strings. This method
    /// is unsafe for a number of reasons:
    ///
    /// * There is no guarantee to the validity of `ptr`.
    /// * The returned lifetime is not guaranteed to be the actual lifetime of
    ///   `ptr`.
    /// * There is no guarantee that the memory pointed to by `ptr` contains a
    ///   valid nul terminator byte at the end of the string.
    ///
    /// > **Note**: This operation is intended to be a 0-cost cast but it is
    /// > currently implemented with an up-front calculation of the length of
    /// > the string. This is not guaranteed to always be the case.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() {
    /// use cstr_core::CStr;
    /// use cstr_core::c_char;
    ///
    /// extern {
    ///     fn my_string() -> *const c_char;
    /// }
    ///
    /// unsafe {
    ///     let slice = CStr::from_ptr(my_string());
    ///     println!("string returned: {}", slice.to_str().unwrap());
    /// }
    /// # }
    /// ```
    pub unsafe fn from_ptr<'a>(ptr: *const c_char) -> &'a CStr {
        let len = strlen(ptr);
        let ptr = ptr as *const u8;
        CStr::from_bytes_with_nul_unchecked(slice::from_raw_parts(ptr, len as usize + 1))
    }

    /// Creates a C string wrapper from a byte slice.
    ///
    /// This function will cast the provided `bytes` to a `CStr` wrapper after
    /// ensuring that it is null terminated and does not contain any interior
    /// nul bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let cstr = CStr::from_bytes_with_nul(b"hello\0");
    /// assert!(cstr.is_ok());
    /// ```
    ///
    /// Creating a `CStr` without a trailing nul byte is an error:
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"hello");
    /// assert!(c_str.is_err());
    /// ```
    ///
    /// Creating a `CStr` with an interior nul byte is an error:
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"he\0llo\0");
    /// assert!(c_str.is_err());
    /// ```
    pub fn from_bytes_with_nul(bytes: &[u8]) -> Result<&CStr, FromBytesWithNulError> {
        let nul_pos = memchr::memchr(0, bytes);
        if let Some(nul_pos) = nul_pos {
            if nul_pos + 1 != bytes.len() {
                return Err(FromBytesWithNulError::interior_nul(nul_pos));
            }
            Ok(unsafe { CStr::from_bytes_with_nul_unchecked(bytes) })
        } else {
            Err(FromBytesWithNulError::not_nul_terminated())
        }
    }

    /// Unsafely creates a C string wrapper from a byte slice.
    ///
    /// This function will cast the provided `bytes` to a `CStr` wrapper without
    /// performing any sanity checks. The provided slice must be null terminated
    /// and not contain any interior nul bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::{CStr, CString};
    ///
    /// unsafe {
    ///     let cstring = CString::new("hello").unwrap();
    ///     let cstr = CStr::from_bytes_with_nul_unchecked(cstring.to_bytes_with_nul());
    ///     assert_eq!(cstr, &*cstring);
    /// }
    /// ```
    #[inline]
    pub unsafe fn from_bytes_with_nul_unchecked(bytes: &[u8]) -> &CStr {
        &*(bytes as *const [u8] as *const CStr)
    }

    /// Returns the inner pointer to this C string.
    ///
    /// The returned pointer will be valid for as long as `self` is and points
    /// to a contiguous region of memory terminated with a 0 byte to represent
    /// the end of the string.
    ///
    /// **WARNING**
    ///
    /// It is your responsibility to make sure that the underlying memory is not
    /// freed too early. For example, the following code will cause undefined
    /// behavior when `ptr` is used inside the `unsafe` block:
    ///
    /// ```no_run
    /// use cstr_core::{CString};
    ///
    /// let ptr = CString::new("Hello").unwrap().as_ptr();
    /// unsafe {
    ///     // `ptr` is dangling
    ///     *ptr;
    /// }
    /// ```
    ///
    /// This happens because the pointer returned by `as_ptr` does not carry any
    /// lifetime information and the string is deallocated immediately after
    /// the `CString::new("Hello").unwrap().as_ptr()` expression is evaluated.
    /// To fix the problem, bind the string to a local variable:
    ///
    /// ```no_run
    /// use cstr_core::{CString};
    ///
    /// let hello = CString::new("Hello").unwrap();
    /// let ptr = hello.as_ptr();
    /// unsafe {
    ///     // `ptr` is valid because `hello` is in scope
    ///     *ptr;
    /// }
    /// ```
    #[inline]
    pub fn as_ptr(&self) -> *const c_char {
        self.inner.as_ptr()
    }

    /// Converts this C string to a byte slice.
    ///
    /// This function will calculate the length of this string (which normally
    /// requires a linear amount of work to be done) and then return the
    /// resulting slice of `u8` elements.
    ///
    /// The returned slice will **not** contain the trailing nul that this C
    /// string has.
    ///
    /// > **Note**: This method is currently implemented as a 0-cost cast, but
    /// > it is planned to alter its definition in the future to perform the
    /// > length calculation whenever this method is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"foo\0").unwrap();
    /// assert_eq!(c_str.to_bytes(), b"foo");
    /// ```
    #[inline]
    pub fn to_bytes(&self) -> &[u8] {
        let bytes = self.to_bytes_with_nul();
        &bytes[..bytes.len() - 1]
    }

    /// Converts this C string to a byte slice containing the trailing 0 byte.
    ///
    /// This function is the equivalent of [`to_bytes`] except that it will retain
    /// the trailing nul instead of chopping it off.
    ///
    /// > **Note**: This method is currently implemented as a 0-cost cast, but
    /// > it is planned to alter its definition in the future to perform the
    /// > length calculation whenever this method is called.
    ///
    /// [`to_bytes`]: #method.to_bytes
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"foo\0").unwrap();
    /// assert_eq!(c_str.to_bytes_with_nul(), b"foo\0");
    /// ```
    #[inline]
    pub fn to_bytes_with_nul(&self) -> &[u8] {
        unsafe { &*(&self.inner as *const [c_char] as *const [u8]) }
    }

    /// Yields a [`&str`] slice if the `CStr` contains valid UTF-8.
    ///
    /// This function will calculate the length of this string and check for
    /// UTF-8 validity, and then return the [`&str`] if it's valid.
    ///
    /// > **Note**: This method is currently implemented to check for validity
    /// > after a 0-cost cast, but it is planned to alter its definition in the
    /// > future to perform the length calculation in addition to the UTF-8
    /// > check whenever this method is called.
    ///
    /// [`&str`]: ../primitive.str.html
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"foo\0").unwrap();
    /// assert_eq!(c_str.to_str(), Ok("foo"));
    /// ```
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        // NB: When CStr is changed to perform the length check in .to_bytes()
        // instead of in from_ptr(), it may be worth considering if this should
        // be rewritten to do the UTF-8 check inline with the length calculation
        // instead of doing it afterwards.
        str::from_utf8(self.to_bytes())
    }

    /// Converts a `CStr` into a [`Cow`]`<`[`str`]`>`.
    ///
    /// This function will calculate the length of this string (which normally
    /// requires a linear amount of work to be done) and then return the
    /// resulting slice as a [`Cow`]`<`[`str`]`>`, replacing any invalid UTF-8 sequences
    /// with `U+FFFD REPLACEMENT CHARACTER`.
    ///
    /// > **Note**: This method is currently implemented to check for validity
    /// > after a 0-cost cast, but it is planned to alter its definition in the
    /// > future to perform the length calculation in addition to the UTF-8
    /// > check whenever this method is called.
    ///
    /// [`Cow`]: ../borrow/enum.Cow.html
    /// [`str`]: ../primitive.str.html
    ///
    /// # Examples
    ///
    /// Calling `to_string_lossy` on a `CStr` containing valid UTF-8:
    ///
    /// ```
    /// use std::borrow::Cow;
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"Hello World\0").unwrap();
    /// assert_eq!(c_str.to_string_lossy(), Cow::Borrowed("Hello World"));
    /// ```
    ///
    /// Calling `to_string_lossy` on a `CStr` containing invalid UTF-8:
    ///
    /// ```
    /// use std::borrow::Cow;
    /// use cstr_core::CStr;
    ///
    /// let c_str = CStr::from_bytes_with_nul(b"Hello \xF0\x90\x80World\0").unwrap();
    /// assert_eq!(
    ///     c_str.to_string_lossy(),
    ///     Cow::Owned(String::from("Hello �World")) as Cow<str>
    /// );
    /// ```
    #[cfg(feature = "alloc")]
    pub fn to_string_lossy(&self) -> Cow<str> {
        String::from_utf8_lossy(self.to_bytes())
    }

    /// Converts a [`Box`]`<CStr>` into a [`CString`] without copying or allocating.
    ///
    /// [`Box`]: ../boxed/struct.Box.html
    /// [`CString`]: struct.CString.html
    ///
    /// # Examples
    ///
    /// ```
    /// use cstr_core::CString;
    ///
    /// let c_string = CString::new(b"foo".to_vec()).unwrap();
    /// let boxed = c_string.into_boxed_c_str();
    /// assert_eq!(boxed.into_c_string(), CString::new("foo").unwrap());
    /// ```
    #[cfg(feature = "alloc")]
    pub fn into_c_string(self: Box<CStr>) -> CString {
        let raw = Box::into_raw(self) as *mut [u8];
        CString {
            inner: unsafe { Box::from_raw(raw) },
        }
    }
}

impl PartialEq for CStr {
    fn eq(&self, other: &CStr) -> bool {
        self.to_bytes().eq(other.to_bytes())
    }
}
impl Eq for CStr {}
impl PartialOrd for CStr {
    fn partial_cmp(&self, other: &CStr) -> Option<Ordering> {
        self.to_bytes().partial_cmp(&other.to_bytes())
    }
}
impl Ord for CStr {
    fn cmp(&self, other: &CStr) -> Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

#[cfg(feature = "alloc")]
impl ToOwned for CStr {
    type Owned = CString;

    fn to_owned(&self) -> CString {
        CString {
            inner: self.to_bytes_with_nul().into(),
        }
    }
}

#[cfg(feature = "alloc")]
impl<'a> From<&'a CStr> for CString {
    fn from(s: &'a CStr) -> CString {
        s.to_owned()
    }
}

#[cfg(feature = "alloc")]
impl ops::Index<ops::RangeFull> for CString {
    type Output = CStr;

    #[inline]
    fn index(&self, _index: ops::RangeFull) -> &CStr {
        self
    }
}

impl AsRef<CStr> for CStr {
    #[inline]
    fn as_ref(&self) -> &CStr {
        self
    }
}

#[cfg(feature = "alloc")]
impl AsRef<CStr> for CString {
    #[inline]
    fn as_ref(&self) -> &CStr {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow::{Borrowed, Owned};
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    #[test]
    fn c_to_rust() {
        let data = b"123\0";
        let ptr = data.as_ptr() as *const c_char;
        unsafe {
            assert_eq!(CStr::from_ptr(ptr).to_bytes(), b"123");
            assert_eq!(CStr::from_ptr(ptr).to_bytes_with_nul(), b"123\0");
        }
    }

    #[test]
    fn simple() {
        let s = CString::new("1234").unwrap();
        assert_eq!(s.as_bytes(), b"1234");
        assert_eq!(s.as_bytes_with_nul(), b"1234\0");
    }

    #[test]
    fn build_with_zero1() {
        assert!(CString::new(&b"\0"[..]).is_err());
    }
    #[test]
    fn build_with_zero2() {
        assert!(CString::new(vec![0]).is_err());
    }

    #[test]
    fn build_with_zero3() {
        unsafe {
            let s = CString::from_vec_unchecked(vec![0]);
            assert_eq!(s.as_bytes(), b"\0");
        }
    }

    #[test]
    fn formatted() {
        let s = CString::new(&b"abc\x01\x02\n\xE2\x80\xA6\xFF"[..]).unwrap();
        assert_eq!(format!("{:?}", s), r#""abc\x01\x02\n\xe2\x80\xa6\xff""#);
    }

    #[test]
    fn borrowed() {
        unsafe {
            let s = CStr::from_ptr(b"12\0".as_ptr() as *const _);
            assert_eq!(s.to_bytes(), b"12");
            assert_eq!(s.to_bytes_with_nul(), b"12\0");
        }
    }

    #[test]
    fn to_str() {
        let data = b"123\xE2\x80\xA6\0";
        let ptr = data.as_ptr() as *const c_char;
        unsafe {
            assert_eq!(CStr::from_ptr(ptr).to_str(), Ok("123…"));
            assert_eq!(CStr::from_ptr(ptr).to_string_lossy(), Borrowed("123…"));
        }
        let data = b"123\xE2\0";
        let ptr = data.as_ptr() as *const c_char;
        unsafe {
            assert!(CStr::from_ptr(ptr).to_str().is_err());
            assert_eq!(
                CStr::from_ptr(ptr).to_string_lossy(),
                Owned::<str>(format!("123\u{FFFD}"))
            );
        }
    }

    #[test]
    fn to_owned() {
        let data = b"123\0";
        let ptr = data.as_ptr() as *const c_char;

        let owned = unsafe { CStr::from_ptr(ptr).to_owned() };
        assert_eq!(owned.as_bytes_with_nul(), data);
    }

    #[test]
    fn equal_hash() {
        let data = b"123\xE2\xFA\xA6\0";
        let ptr = data.as_ptr() as *const c_char;
        let cstr: &'static CStr = unsafe { CStr::from_ptr(ptr) };

        let mut s = DefaultHasher::new();
        cstr.hash(&mut s);
        let cstr_hash = s.finish();
        let mut s = DefaultHasher::new();
        CString::new(&data[..data.len() - 1]).unwrap().hash(&mut s);
        let cstring_hash = s.finish();

        assert_eq!(cstr_hash, cstring_hash);
    }

    #[test]
    fn from_bytes_with_nul() {
        let data = b"123\0";
        let cstr = CStr::from_bytes_with_nul(data);
        assert_eq!(cstr.map(CStr::to_bytes), Ok(&b"123"[..]));
        let cstr = CStr::from_bytes_with_nul(data);
        assert_eq!(cstr.map(CStr::to_bytes_with_nul), Ok(&b"123\0"[..]));

        unsafe {
            let cstr = CStr::from_bytes_with_nul(data);
            let cstr_unchecked = CStr::from_bytes_with_nul_unchecked(data);
            assert_eq!(cstr, Ok(cstr_unchecked));
        }
    }

    #[test]
    fn from_bytes_with_nul_unterminated() {
        let data = b"123";
        let cstr = CStr::from_bytes_with_nul(data);
        assert!(cstr.is_err());
    }

    #[test]
    fn from_bytes_with_nul_interior() {
        let data = b"1\023\0";
        let cstr = CStr::from_bytes_with_nul(data);
        assert!(cstr.is_err());
    }

    #[test]
    fn into_boxed() {
        let orig: &[u8] = b"Hello, world!\0";
        let cstr = CStr::from_bytes_with_nul(orig).unwrap();
        let boxed: Box<CStr> = Box::from(cstr);
        let cstring = cstr.to_owned().into_boxed_c_str().into_c_string();
        assert_eq!(cstr, &*boxed);
        assert_eq!(&*boxed, &*cstring);
        assert_eq!(&*cstring, cstr);
    }

    #[test]
    fn boxed_default() {
        let boxed = <Box<CStr>>::default();
        assert_eq!(boxed.to_bytes_with_nul(), &[0]);
    }
}
