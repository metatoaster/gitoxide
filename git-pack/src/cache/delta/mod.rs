use std::collections::VecDeque;

/// Returned when using various methods on a [`Tree`]
#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Pack offsets must only increment. The previous pack offset was {last_pack_offset}, the current one is {pack_offset}")]
    InvariantIncreasingPackOffset {
        /// The last seen pack offset
        last_pack_offset: crate::data::Offset,
        /// The invariant violating offset
        pack_offset: crate::data::Offset,
    },
}

mod iter;
pub use iter::{Chunk, Node};
///
pub mod traverse;

///
pub mod from_offsets;

/// An item stored within the [`Tree`]
pub struct Item<T> {
    /// The offset into the pack file at which the pack entry's data is located.
    pub offset: crate::data::Offset,
    /// The offset of the next item in the pack file.
    pub next_offset: crate::data::Offset,
    /// Data to store with each Item, effectively data associated with each entry in a pack.
    pub data: T,
    /// Indices into our Tree's `items`, one for each pack entry that depends on us.
    children: Vec<usize>,
}
/// A tree that allows one-time iteration over all nodes and their children, consuming it in the process,
/// while being shareable among threads without a lock.
/// It does this by making the guarantee that iteration only happens once.
pub struct Tree<T> {
    /// Roots are first, then children.
    items: VecDeque<Item<T>>,
    roots: usize,
    /// The last child index into the `items` array
    last_index: usize,
    /// Future child offsets, associating their offset into the pack with their index in the items array.
    /// (parent_offset, child_index)
    future_child_offsets: Vec<(crate::data::Offset, usize)>,
}

impl<T> Tree<T> {
    /// Instantiate a empty tree capable of storing `num_objects` amounts of items.
    pub fn with_capacity(num_objects: usize) -> Result<Self, Error> {
        Ok(Tree {
            items: VecDeque::with_capacity(num_objects),
            roots: 0,
            last_index: 0,
            future_child_offsets: Vec::new(),
        })
    }

    fn assert_is_incrementing_and_update_next_offset(&mut self, offset: crate::data::Offset) -> Result<(), Error> {
        if self.items.is_empty() {
            return Ok(());
        }
        let item = &mut self.items[self.last_index];
        let last_offset = item.offset;
        if offset <= last_offset {
            return Err(Error::InvariantIncreasingPackOffset {
                last_pack_offset: last_offset,
                pack_offset: offset,
            });
        }
        item.next_offset = offset;
        Ok(())
    }

    fn set_pack_entries_end_and_resolve_ref_offsets(
        &mut self,
        pack_entries_end: crate::data::Offset,
    ) -> Result<(), traverse::Error> {
        if self.items.is_empty() {
            return Ok(());
        };

        if !self.future_child_offsets.is_empty() {
            let (roots, children) = self.items.as_mut_slices();
            assert_eq!(
                roots.len(),
                self.roots,
                "item deque has been resized, maybe we added more nodes than we declared in the constructor?"
            );
            for (parent_offset, child_index) in self.future_child_offsets.drain(..) {
                if let Ok(i) = children.binary_search_by_key(&parent_offset, |i| i.offset) {
                    children[i].children.push(child_index);
                } else if let Ok(i) = roots.binary_search_by(|i| parent_offset.cmp(&i.offset)) {
                    roots[i].children.push(child_index);
                } else {
                    return Err(traverse::Error::OutOfPackRefDelta {
                        base_pack_offset: parent_offset,
                    });
                }
            }
        }

        self.items[self.last_index].next_offset = pack_entries_end;
        Ok(())
    }

    /// Add a new root node, one that only has children but is not a child itself, at the given pack `offset` and associate
    /// custom `data` with it.
    pub fn add_root(&mut self, offset: crate::data::Offset, data: T) -> Result<(), Error> {
        self.assert_is_incrementing_and_update_next_offset(offset)?;
        self.last_index = 0;
        self.items.push_front(Item {
            offset,
            next_offset: 0,
            data,
            children: Vec::new(),
        });
        self.roots += 1;
        Ok(())
    }

    /// Add a child of the item at `base_offset` which itself resides at pack `offset` and associate custom `data` with it.
    pub fn add_child(
        &mut self,
        base_offset: crate::data::Offset,
        offset: crate::data::Offset,
        data: T,
    ) -> Result<(), Error> {
        self.assert_is_incrementing_and_update_next_offset(offset)?;
        let (roots, children) = self.items.as_mut_slices();
        assert_eq!(
            roots.len(),
            self.roots,
            "item deque has been resized, maybe we added more nodes than we declared in the constructor?"
        );
        let next_child_index = children.len();
        if let Ok(i) = children.binary_search_by_key(&base_offset, |i| i.offset) {
            children[i].children.push(next_child_index);
        } else if let Ok(i) = roots.binary_search_by(|i| base_offset.cmp(&i.offset)) {
            roots[i].children.push(next_child_index);
        } else {
            self.future_child_offsets.push((base_offset, next_child_index));
        }
        self.last_index = self.items.len();
        self.items.push_back(Item {
            offset,
            next_offset: 0,
            data,
            children: Vec::new(),
        });
        Ok(())
    }

    /// Transform this `Tree` into its items.
    pub fn into_items(self) -> VecDeque<Item<T>> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    mod tree {
        mod from_offsets_in_pack {
            use std::sync::atomic::AtomicBool;

            use git_odb::pack;

            const SMALL_PACK_INDEX: &str = "objects/pack/pack-a2bf8e71d8c18879e499335762dd95119d93d9f1.idx";
            const SMALL_PACK: &str = "objects/pack/pack-a2bf8e71d8c18879e499335762dd95119d93d9f1.pack";

            const INDEX_V1: &str = "objects/pack/pack-c0438c19fb16422b6bbcce24387b3264416d485b.idx";
            const PACK_FOR_INDEX_V1: &str = "objects/pack/pack-c0438c19fb16422b6bbcce24387b3264416d485b.pack";

            use git_testtools::fixture_path;

            #[test]
            fn v1() -> Result<(), Box<dyn std::error::Error>> {
                tree(INDEX_V1, PACK_FOR_INDEX_V1)
            }

            #[test]
            fn v2() -> Result<(), Box<dyn std::error::Error>> {
                tree(SMALL_PACK_INDEX, SMALL_PACK)
            }

            fn tree(index_path: &str, pack_path: &str) -> Result<(), Box<dyn std::error::Error>> {
                let idx = pack::index::File::at(fixture_path(index_path), git_hash::Kind::Sha1)?;
                crate::cache::delta::Tree::from_offsets_in_pack(
                    fixture_path(pack_path),
                    idx.sorted_offsets().into_iter(),
                    |ofs| *ofs,
                    |id| idx.lookup(id).map(|index| idx.pack_offset_at_index(index)),
                    git_features::progress::Discard,
                    &AtomicBool::new(false),
                    git_hash::Kind::Sha1,
                )?;
                Ok(())
            }
        }
    }

    struct TreeItem<D> {
        _offset: crate::data::Offset,
        _data: D,
        _children: Vec<usize>,
    }

    #[test]
    fn using_option_as_data_does_not_increase_size_in_memory() {
        struct Entry {
            pub _id: Option<git_hash::ObjectId>,
            pub _crc32: u32,
        }

        struct TreeItemOption<D> {
            _offset: crate::data::Offset,
            _data: Option<D>,
            _children: Vec<usize>,
        }
        assert_eq!(
            std::mem::size_of::<TreeItem<Entry>>(),
            std::mem::size_of::<TreeItemOption<Entry>>(),
            "we hope niche filling optimizations kick in for our data structures to not pay for the Option at all"
        );
        assert_eq!(
            std::mem::size_of::<[TreeItemOption<Entry>; 7_500_000]>(),
            480_000_000,
            "it should be as small as possible"
        );
    }

    #[test]
    fn size_of_pack_verify_data_structure() {
        use git_odb::pack;
        pub struct EntryWithDefault {
            _index_entry: pack::index::Entry,
            _kind: git_object::Kind,
            _object_size: u64,
            _decompressed_size: u64,
            _compressed_size: u64,
            _header_size: u16,
            _level: u16,
        }

        assert_eq!(
            std::mem::size_of::<[TreeItem<EntryWithDefault>; 7_500_000]>(),
            780_000_000
        );
    }
}
