use git_object::mutable;
use std::io;

/// Describe the capability to write git objects into an object store.
pub trait Write {
    /// The error type used for all trait methods.
    ///
    /// _Note_ the default implementations require the `From<io::Error>` bound.
    type Error: std::error::Error + From<io::Error>;

    /// Write [`object`][mutable::Object] using the given kind of [`hash`][git_hash::Kind] into the database,
    /// returning id to reference it in subsequent reads.
    fn write(&self, object: &mutable::Object, hash: git_hash::Kind) -> Result<git_hash::ObjectId, Self::Error> {
        let mut buf = Vec::with_capacity(2048);
        object.write_to(&mut buf)?;
        self.write_stream(object.kind(), buf.len() as u64, buf.as_slice(), hash)
    }
    /// As [`write`][Write::write], but takes an [`object` kind][git_object::Kind] along with its encoded bytes.
    fn write_buf(
        &self,
        object: git_object::Kind,
        from: &[u8],
        hash: git_hash::Kind,
    ) -> Result<git_hash::ObjectId, Self::Error> {
        self.write_stream(object, from.len() as u64, from, hash)
    }
    /// As [`write`][Write::write], but takes an input stream.
    /// This is commonly used for writing blobs directly without reading them to memory first.
    fn write_stream(
        &self,
        kind: git_object::Kind,
        size: u64,
        from: impl io::Read,
        hash: git_hash::Kind,
    ) -> Result<git_hash::ObjectId, Self::Error>;
}

/// Meta data of any object
pub struct ObjectInfo {
    /// The kind of object
    pub kind: git_object::Kind,
    /// The decompressed size of the objects raw data.
    pub size: u64,
}

/// An object that can represent no less than three different kinds of data and helps to avoid unnecessary copies or allocations.
///
/// It can represent…
///
/// * loose objects
/// * decompressed packed objects
/// * entries in packs
///
pub trait Object {
    /// Provide basic information about the object
    fn info(&self) -> ObjectInfo;

    /// Return the objects raw, undecoded data.
    fn data(&self) -> &[u8];

    /// Returns the packed entry if this object is indeed a base object allowing to copy data from pack to pack
    /// and avoiding a decompress/compress round-trip for some objects.
    fn packed_base_data(&self) -> Option<&[u8]> {
        None
    }
}

/// Describe how object can be located in an object store
pub trait Locate {
    /// The object returned by [`locate()`][Locate::locate()]
    type Object: self::Object;
    /// The error returned by [`locate()`][Locate::locate()]
    type Error;

    #[allow(missing_docs)] // TODO
    fn locate<'a>(
        &self,
        id: impl AsRef<git_hash::oid>,
        buffer: &'a mut Vec<u8>,
        pack_cache: &mut impl crate::pack::cache::DecodeEntry,
    ) -> Result<Option<Self::Object>, Self::Error>;
}

#[cfg(test)]
mod tests {
    mod locate {
        use super::super::*;
        use crate::pack::cache::DecodeEntry;
        use git_hash::oid;

        #[test]
        fn can_return_self_contained_objects() {
            struct Db;
            struct SelfContainedObject<'a> {
                our_data: &'a [u8],
            }

            impl<'a> Object for SelfContainedObject<'a> {
                fn info(&self) -> ObjectInfo {
                    todo!()
                }

                fn data(&self) -> &[u8] {
                    self.our_data
                }
            }

            impl Locate for Db {
                type Object<'a> = SelfContainedObject<'a>;
                type Error = ();

                fn locate<'a>(
                    &self,
                    id: impl AsRef<oid>,
                    buffer: &'a mut Vec<u8>,
                    pack_cache: &mut impl DecodeEntry,
                ) -> Result<Option<Self::Object>, Self::Error> {
                    Ok(Some(SelfContainedObject { our_data: buffer }))
                }
            }
        }
    }
}
