//! Storage for AS3 Vectors

use crate::avm2::activation::Activation;
use crate::avm2::error::range_error;
use crate::avm2::object::{ClassObject, Object};
use crate::avm2::value::Value;
use crate::avm2::Error;
use gc_arena::Collect;
use std::cmp::{max, min};
use std::ops::{Index, RangeBounds};
use std::slice::SliceIndex;

/// The vector storage portion of a vector object.
///
/// Vector values are restricted to a single type, decided at the time of the
/// construction of the vector's storage. The type is determined by the type
/// argument associated with the class of the vector. Vector holes are
/// evaluated to a default value based on the type of the vector.
///
/// A vector may also be configured to have a fixed size; when this is enabled,
/// attempts to modify the length fail.
#[derive(Collect, Clone)]
#[collect(no_drop)]
pub struct VectorStorage<'gc> {
    /// The storage for vector values.
    storage: Vec<Value<'gc>>,

    /// Whether or not the array length is fixed.
    is_fixed: bool,

    /// The allowed type of the contents of the vector, in the form of a class
    /// object. None represents a Vector.<*>.
    ///
    /// Vector typing is enforced by one of two ways: either by generating
    /// exceptions on values that are not of the given type, or by coercing
    /// incorrectly typed values to the given type if possible. Values that do
    /// not coerce are replaced with the default value for the given value
    /// type.
    value_type: Option<ClassObject<'gc>>,
}

impl<'gc> VectorStorage<'gc> {
    pub fn new(
        length: usize,
        is_fixed: bool,
        value_type: Option<ClassObject<'gc>>,
        activation: &mut Activation<'_, 'gc>,
    ) -> Self {
        let storage = Vec::new();

        let mut self_vec = VectorStorage {
            storage,
            is_fixed,
            value_type,
        };

        self_vec
            .storage
            .resize(length, self_vec.default(activation));

        self_vec
    }

    fn check_fixed(&self, activation: &mut Activation<'_, 'gc>) -> Result<(), Error<'gc>> {
        if self.is_fixed {
            return Err(Error::AvmError(range_error(
                activation,
                "Error #1126: Cannot change the length of a fixed Vector.",
                1126,
            )?));
        }
        Ok(())
    }

    /// Create a new vector storage from a list of values.
    ///
    /// The values are assumed to already have been coerced to the value type
    /// given.
    pub fn from_values(
        storage: Vec<Value<'gc>>,
        is_fixed: bool,
        value_type: Option<ClassObject<'gc>>,
    ) -> Self {
        VectorStorage {
            storage,
            is_fixed,
            value_type,
        }
    }

    pub fn is_fixed(&self) -> bool {
        self.is_fixed
    }

    pub fn set_is_fixed(&mut self, is_fixed: bool) {
        self.is_fixed = is_fixed;
    }

    pub fn length(&self) -> usize {
        self.storage.len()
    }

    pub fn reserve_exact(&mut self, length: usize) {
        self.storage.reserve_exact(length);
    }

    pub fn resize(
        &mut self,
        new_length: usize,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        self.check_fixed(activation)?;
        self.storage.resize(new_length, self.default(activation));

        Ok(())
    }

    /// Get the default value for this vector.
    pub fn default(&self, activation: &mut Activation<'_, 'gc>) -> Value<'gc> {
        if let Some(value_type) = self.value_type {
            if Object::ptr_eq(value_type, activation.avm2().classes().int)
                || Object::ptr_eq(value_type, activation.avm2().classes().uint)
            {
                Value::Integer(0)
            } else if Object::ptr_eq(value_type, activation.avm2().classes().number) {
                Value::Number(0.0)
            } else {
                Value::Null
            }
        } else {
            Value::Null
        }
    }

    /// Get the value type stored in this vector (same as the class <T> type).
    pub fn value_type(&self) -> Option<ClassObject<'gc>> {
        self.value_type
    }

    /// Get the value type this vector coerces things to.
    pub fn value_type_for_coercion(
        &self,
        activation: &mut Activation<'_, 'gc>,
    ) -> ClassObject<'gc> {
        self.value_type
            .unwrap_or_else(|| activation.avm2().classes().object)
    }

    /// Check if a vector index is in bounds.
    pub fn is_in_range(&self, pos: usize) -> bool {
        pos < self.storage.len()
    }

    /// Change an arbitrary i32 into a positive parameter index.
    ///
    /// This converts negative indicies into positive indicies indexed from the
    /// end of the array. Negative indicies that point before the start of the
    /// array are clamped to zero.
    pub fn clamp_parameter_index(&self, pos: i32) -> usize {
        if pos < 0 {
            max(pos + self.storage.len() as i32, 0) as usize
        } else {
            min(pos as usize, self.storage.len())
        }
    }

    /// Retrieve a value from the vector.
    pub fn get(
        &self,
        pos: usize,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        if let Some(val) = self.storage.get(pos).cloned() {
            Ok(val)
        } else {
            Err(Error::AvmError(range_error(
                activation,
                &format!("{pos} is outside the range of the vector"),
                0,
            )?))
        }
    }

    /// Store a value into the vector.
    ///
    /// This function does no coercion as calling it requires mutably borrowing
    /// the vector (and thus it is unwise to reenter the AVM2 runtime to coerce
    /// things). You must use the associated `coerce` fn before storing things
    /// in the vector.
    ///
    /// This function yields an error if the position is outside the length of
    /// the vector.
    pub fn set(
        &mut self,
        pos: usize,
        value: Value<'gc>,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        if !self.is_fixed && pos == self.length() {
            self.storage.resize(pos + 1, self.default(activation));
        }

        if let Some(v) = self.storage.get_mut(pos) {
            *v = value;
            Ok(())
        } else {
            Err(Error::AvmError(range_error(
                activation,
                &format!("{pos} is outside the range of the vector"),
                0,
            )?))
        }
    }

    /// Push a value to the end of the vector.
    ///
    /// This function returns an error if the vector is fixed.
    ///
    /// This function does no coercion as calling it requires mutably borrowing
    /// the vector (and thus it is unwise to reenter the AVM2 runtime to coerce
    /// things). You must use the associated `coerce` fn before storing things
    /// in the vector.
    pub fn push(
        &mut self,
        value: Value<'gc>,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        self.check_fixed(activation)?;
        self.storage.push(value);

        Ok(())
    }

    /// Pop a value off the end of the vector.
    ///
    /// This function returns an error if the vector is fixed.
    pub fn pop(&mut self, activation: &mut Activation<'_, 'gc>) -> Result<Value<'gc>, Error<'gc>> {
        self.check_fixed(activation)?;

        if let Some(v) = self.storage.pop() {
            Ok(v)
        } else if let Some(value_type) = self.value_type() {
            if Object::ptr_eq(value_type, activation.avm2().classes().uint)
                || Object::ptr_eq(value_type, activation.avm2().classes().int)
            {
                Ok(Value::Integer(0))
            } else if Object::ptr_eq(value_type, activation.avm2().classes().number) {
                Ok(Value::Number(0.0))
            } else {
                Ok(Value::Undefined)
            }
        } else {
            Ok(Value::Undefined)
        }
    }

    /// Push a value to the end of the vector.
    ///
    /// This function returns an error if the vector is fixed.
    ///
    /// This function does no coercion as calling it requires mutably borrowing
    /// the vector (and thus it is unwise to reenter the AVM2 runtime to coerce
    /// things). You must use the associated `coerce` fn before storing things
    /// in the vector.
    pub fn unshift(
        &mut self,
        value: Value<'gc>,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        self.check_fixed(activation)?;

        self.storage.insert(0, value);

        Ok(())
    }

    /// Pop a value off the start of the vector.
    ///
    /// This function returns an error if the vector is fixed.
    pub fn shift(
        &mut self,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        self.check_fixed(activation)?;

        if !self.storage.is_empty() {
            Ok(self.storage.remove(0))
        } else if let Some(value_type) = self.value_type() {
            if Object::ptr_eq(value_type, activation.avm2().classes().uint)
                || Object::ptr_eq(value_type, activation.avm2().classes().int)
            {
                Ok(Value::Integer(0))
            } else if Object::ptr_eq(value_type, activation.avm2().classes().number) {
                Ok(Value::Number(0.0))
            } else {
                Ok(Value::Undefined)
            }
        } else {
            Ok(Value::Undefined)
        }
    }

    /// Insert a value at a specific position in the vector.
    ///
    /// This function returns an error if the vector is fixed.
    ///
    /// This function does no coercion as calling it requires mutably borrowing
    /// the vector (and thus it is unwise to reenter the AVM2 runtime to coerce
    /// things). You must use the associated `coerce` fn before storing things
    /// in the vector.
    ///
    /// Negative bounds are supported and treated as indexing from the end of
    /// the array, backwards.
    pub fn insert(
        &mut self,
        position: i32,
        value: Value<'gc>,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<(), Error<'gc>> {
        self.check_fixed(activation)?;

        let position = self.clamp_parameter_index(position);
        if position >= self.storage.len() {
            self.storage.push(value);
        } else {
            self.storage.insert(position, value);
        }

        Ok(())
    }

    /// Remove a value from a specific position in the vector.
    ///
    /// This function returns an error if the vector is fixed, empty, or being
    /// indexed out of bounds. Otherwise, it returns the removed value.
    ///
    /// Negative bounds are supported and treated as indexing from the end of
    /// the array, backwards. Negative arrays are *not* subject to the bounds
    /// check error.
    pub fn remove(
        &mut self,
        position: i32,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        self.check_fixed(activation)?;

        let position = if position < 0 {
            max(position + self.storage.len() as i32, 0) as usize
        } else {
            position as usize
        };

        if position >= self.storage.len() {
            Err(format!("RangeError: Index {position} extends beyond the end of the vector").into())
        } else {
            Ok(self.storage.remove(position))
        }
    }

    /// Reverse the vector's storage.
    pub fn reverse(&mut self) {
        self.storage.reverse();
    }

    /// Iterate over vector values.
    pub fn iter<'a>(
        &'a self,
    ) -> impl DoubleEndedIterator<Item = Value<'gc>> + ExactSizeIterator<Item = Value<'gc>> + 'a
    {
        self.storage.iter().cloned()
    }

    /// Replace this vector's storage with new values.
    pub fn replace_storage(&mut self, new_storage: Vec<Value<'gc>>) {
        self.storage = new_storage;
    }

    pub fn splice<R>(
        &mut self,
        range: R,
        replace_with: Vec<Value<'gc>>,
    ) -> Result<Vec<Value<'gc>>, Error<'gc>>
    where
        R: Clone + SliceIndex<[Value<'gc>], Output = [Value<'gc>]> + RangeBounds<usize>,
    {
        if self.is_fixed && self.storage.index(range.clone()).len() != replace_with.len() {
            return Err("RangeError: Vector is fixed".into());
        }

        Ok(self.storage.splice(range, replace_with).collect())
    }
}
