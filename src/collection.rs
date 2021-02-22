use crate::error::{IsarError, Result};
use crate::index::Index;
use crate::link::Link;
use crate::lmdb::Key;
use crate::object::isar_object::{IsarObject, Property};
use crate::object::json_encode_decode::JsonEncodeDecode;
use crate::object::object_builder::ObjectBuilder;
use crate::object::object_info::ObjectInfo;
use crate::query::query_builder::QueryBuilder;
use crate::query::where_clause::WhereClause;
use crate::query::Sort;
use crate::txn::{Cursors, IsarTxn};
use crate::utils::{oid_from_bytes, oid_to_bytes, MAX_OID, MIN_OID};
use crate::watch::change_set::ChangeSet;
use serde_json::{json, Value};
use std::cell::Cell;
use std::ops::Add;

#[cfg(test)]
use {crate::utils::debug::dump_db, hashbrown::HashMap};

pub struct IsarCollection {
    id: u16,
    name: String,
    object_info: ObjectInfo,
    indexes: Vec<Index>,
    links: Vec<Link>,
    oid_counter: Cell<i64>,
}

unsafe impl Send for IsarCollection {}
unsafe impl Sync for IsarCollection {}

impl IsarCollection {
    pub(crate) fn new(
        id: u16,
        name: String,
        object_info: ObjectInfo,
        indexes: Vec<Index>,
        links: Vec<Link>,
    ) -> Self {
        IsarCollection {
            id,
            name,
            object_info,
            indexes,
            links,
            oid_counter: Cell::new(0),
        }
    }

    pub(crate) fn get_id(&self) -> u16 {
        self.id
    }

    pub(crate) fn get_indexes(&self) -> &[Index] {
        &self.indexes
    }

    pub(crate) fn update_oid_counter(&self, counter: i64) {
        if counter > self.oid_counter.get() {
            self.oid_counter.set(counter);
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_oid_property(&self) -> Property {
        self.object_info.get_oid_property()
    }

    pub fn get_properties(&self) -> &[(String, Property)] {
        self.object_info.get_properties()
    }

    pub fn new_object_builder(&self, buffer: Option<Vec<u8>>) -> ObjectBuilder {
        ObjectBuilder::new(&self.object_info, buffer)
    }

    pub fn new_query_builder(&self) -> QueryBuilder {
        QueryBuilder::new(self)
    }

    pub fn new_primary_where_clause(
        &self,
        lower_oid: Option<i64>,
        upper_oid: Option<i64>,
        sort: Sort,
    ) -> Result<WhereClause> {
        WhereClause::new_primary(
            self.id,
            lower_oid.unwrap_or(MIN_OID),
            upper_oid.unwrap_or(MAX_OID),
            sort,
        )
    }

    pub fn new_secondary_where_clause(
        &self,
        index_index: usize,
        skip_duplicates: bool,
        sort: Sort,
    ) -> Option<WhereClause> {
        self.indexes
            .get(index_index)
            .map(|i| i.new_where_clause(skip_duplicates, sort))
    }

    pub fn auto_increment(&self, _txn: &mut IsarTxn) -> Result<i64> {
        let counter = self.oid_counter.get().add(1);
        if counter <= MAX_OID {
            self.oid_counter.set(counter);
            Ok(counter)
        } else {
            Err(IsarError::AutoIncrementOverflow {})
        }
    }

    pub fn get<'txn>(&self, txn: &'txn mut IsarTxn, oid: i64) -> Result<Option<IsarObject<'txn>>> {
        let oid_bytes = oid_to_bytes(oid, self.id)?;
        txn.read(|c, _| {
            let object = c
                .primary
                .move_to(Key(&oid_bytes))?
                .map(|(_, v)| IsarObject::from_bytes(v));
            Ok(object)
        })
    }

    pub fn put(&self, txn: &mut IsarTxn, object: IsarObject) -> Result<()> {
        txn.write(|r_cursors, w_cursors, change_set| {
            self.put_internal(r_cursors, w_cursors, change_set, object)
        })
    }

    fn put_internal(
        &self,
        cursors: &mut Cursors,
        cursors2: &mut Cursors,
        mut change_set: Option<&mut ChangeSet>,
        object: IsarObject,
    ) -> Result<()> {
        let oid = object.read_long(self.get_oid_property());
        self.delete_object_internal(cursors, change_set.as_deref_mut(), oid)?;
        self.update_oid_counter(oid);

        if !self.object_info.verify_object(object) {
            return Err(IsarError::InvalidObject {});
        }

        for index in &self.indexes {
            if index.does_replace() {
                let wc = index.new_where_clause(false, Sort::Ascending);
                wc.iter(cursors, |_, _, oid| {
                    let (oid, _) = oid_from_bytes(oid);
                    self.delete_internal(cursors2, change_set.as_deref_mut(), oid)?;
                    Ok(true)
                })?;
            }
            index.create_for_object(cursors2, oid, object)?;
        }

        let oid_bytes = oid_to_bytes(oid, self.id)?;
        cursors.primary.put(Key(&oid_bytes), object.as_bytes())?;
        if let Some(change_set) = change_set.as_deref_mut() {
            change_set.register_change(self.id, oid, object);
        }
        Ok(())
    }

    pub fn delete(&self, txn: &mut IsarTxn, oid: i64) -> Result<bool> {
        txn.write(|_, cursors, change_set| self.delete_internal(cursors, change_set, oid))
    }

    pub(crate) fn delete_internal(
        &self,
        cursors: &mut Cursors,
        change_set: Option<&mut ChangeSet>,
        oid: i64,
    ) -> Result<bool> {
        if self.delete_object_internal(cursors, change_set, oid)? {
            for link in &self.links {
                link.delete_for_object(cursors, oid)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn delete_object_internal(
        &self,
        cursors: &mut Cursors,
        change_set: Option<&mut ChangeSet>,
        oid: i64,
    ) -> Result<bool> {
        let oid_bytes = oid_to_bytes(oid, self.id)?;
        if let Some((_, object)) = cursors.primary.move_to(Key(&oid_bytes))? {
            let object = IsarObject::from_bytes(object);
            for index in &self.indexes {
                index.delete_for_object(cursors, oid, object)?;
            }

            if let Some(change_set) = change_set {
                change_set.register_change(self.id, oid, object);
            }
            cursors.primary.delete_current()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn clear(&self, txn: &mut IsarTxn) -> Result<usize> {
        let mut counter = 0;
        self.new_query_builder()
            .build()
            .delete_while(txn, self, |_| {
                counter += 1;
                true
            })?;
        Ok(counter)
    }

    pub fn import_json(&self, txn: &mut IsarTxn, json: Value) -> Result<()> {
        txn.write(|r_cursors, w_cursors, mut change_set| {
            let array = json.as_array().ok_or(IsarError::InvalidJson {})?;
            let mut ob_result_cache = None;
            for value in array {
                let ob = JsonEncodeDecode::decode(self, value, ob_result_cache)?;
                let object = ob.finish();
                self.put_internal(r_cursors, w_cursors, change_set.as_deref_mut(), object)?;
                ob_result_cache = Some(ob.recycle());
            }
            Ok(())
        })
    }

    pub fn export_json(
        &self,
        txn: &mut IsarTxn,
        primitive_null: bool,
        byte_as_bool: bool,
    ) -> Result<Value> {
        let mut items = vec![];
        self.new_query_builder().build().find_while(txn, |object| {
            let entry = JsonEncodeDecode::encode(self, object, primitive_null, byte_as_bool);
            items.push(entry);
            true
        })?;
        Ok(json!(items))
    }

    #[cfg(test)]
    pub fn debug_dump(&self, txn: &mut IsarTxn) -> HashMap<i64, Vec<u8>> {
        txn.read(|cursors, _| {
            let map = dump_db(&mut cursors.primary, Some(&self.id.to_be_bytes()))
                .into_iter()
                .map(|(k, v)| (oid_from_bytes(&k).0, v))
                .collect();
            Ok(map)
        })
        .unwrap()
    }

    #[cfg(test)]
    pub fn debug_get_index(&self, index: usize) -> &Index {
        self.indexes.get(index).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::object::data_type::DataType;
    use crate::query::filter::LongBetweenCond;
    use crate::utils::oid_to_bytes;
    use crate::{col, ind, isar, map, set};
    use crossbeam_channel::unbounded;

    #[test]
    fn test_get() {
        isar!(isar, col => col!(oid => DataType::Long, field2 => DataType::Int));
        let mut txn = isar.begin_txn(true, false).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(123);
        builder.write_int(555);
        let object = builder.finish();
        col.put(&mut txn, object).unwrap();

        assert_eq!(col.get(&mut txn, 123).unwrap().unwrap(), object);

        assert_eq!(col.get(&mut txn, 321).unwrap(), None);
    }

    #[test]
    fn test_put_new() {
        isar!(isar, col => col!(field1 => DataType::Long));

        let mut txn = isar.begin_txn(true, false).unwrap();
        assert_eq!(col.oid_counter.get(), 0);

        let mut builder = col.new_object_builder(None);
        builder.write_long(123);
        let object1 = builder.finish();
        col.put(&mut txn, object1).unwrap();
        assert_eq!(col.oid_counter.get(), 123);

        let mut builder = col.new_object_builder(None);
        builder.write_long(100);
        let object2 = builder.finish();
        col.put(&mut txn, object2).unwrap();
        assert_eq!(col.oid_counter.get(), 123);

        assert_eq!(
            col.debug_dump(&mut txn),
            map![
                123 => object1.as_bytes().to_vec(),
                100 => object2.as_bytes().to_vec()
            ]
        );
    }

    #[test]
    fn test_put_existing() {
        isar!(isar, col => col!(field1 => DataType::Long, field2 => DataType::Int));
        let mut txn = isar.begin_txn(true, false).unwrap();
        assert_eq!(col.oid_counter.get(), 0);

        let mut builder = col.new_object_builder(None);
        builder.write_long(123);
        builder.write_int(1);
        let object1 = builder.finish();
        col.put(&mut txn, object1).unwrap();
        assert_eq!(col.oid_counter.get(), 123);

        let mut builder = col.new_object_builder(None);
        builder.write_long(123);
        builder.write_int(2);
        let object2 = builder.finish();
        col.put(&mut txn, object2).unwrap();
        assert_eq!(col.oid_counter.get(), 123);

        let mut builder = col.new_object_builder(None);
        builder.write_long(333);
        builder.write_int(3);
        let object3 = builder.finish();
        col.put(&mut txn, object3).unwrap();
        assert_eq!(col.oid_counter.get(), 333);

        assert_eq!(
            col.debug_dump(&mut txn),
            map![
                123 => object2.as_bytes().to_vec(),
                333 => object3.as_bytes().to_vec()
            ]
        );
    }

    #[test]
    fn test_put_creates_index() {
        isar!(isar, col => col!(field1 => DataType::Long, field2 => DataType::Int; ind!(field2)));

        let mut txn = isar.begin_txn(true, false).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(1);
        builder.write_int(1234);
        let object = builder.finish();
        col.put(&mut txn, object).unwrap();

        let index = &col.indexes[0];
        let key = index.debug_create_keys(object)[0].clone();
        assert_eq!(
            index.debug_dump(&mut txn),
            set![(key, oid_to_bytes(1, col.id).unwrap().to_vec())]
        );
    }

    #[test]
    fn test_put_clears_old_index() {
        isar!(isar, col => col!(field1 => DataType::Long, field2 => DataType::Int; ind!(field2)));

        let mut txn = isar.begin_txn(true, false).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(555);
        builder.write_int(1234);
        let object = builder.finish();
        col.put(&mut txn, object).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(555);
        builder.write_int(5678);
        let object2 = builder.finish();
        col.put(&mut txn, object2).unwrap();

        let index = &col.indexes[0];
        let key = index.debug_create_keys(object2)[0].clone();
        assert_eq!(
            index.debug_dump(&mut txn),
            set![(key, oid_to_bytes(555, col.id).unwrap().to_vec())],
        );
    }

    #[test]
    fn test_put_calls_notifiers() {
        isar!(isar, col => col!(field1 => DataType::Long));
        let p = col.get_properties().first().unwrap().1;

        let mut qb1 = col.new_query_builder();
        qb1.set_filter(LongBetweenCond::filter(p, 1, 1).unwrap());
        let q1 = qb1.build();

        let mut qb2 = col.new_query_builder();
        qb2.set_filter(LongBetweenCond::filter(p, 2, 2).unwrap());
        let q2 = qb2.build();

        let (tx1, rx1) = unbounded();
        let handle1 = isar.watch_query(col, q1, Box::new(move || tx1.send(true).unwrap()));

        let (tx2, rx2) = unbounded();
        let handle2 = isar.watch_query(col, q2, Box::new(move || tx2.send(true).unwrap()));

        let mut txn = isar.begin_txn(true, false).unwrap();
        let mut builder = col.new_object_builder(None);
        builder.write_long(1);
        col.put(&mut txn, builder.finish()).unwrap();
        txn.commit().unwrap();

        assert_eq!(rx1.len(), 1);
        assert_eq!(rx2.len(), 0);
        assert!(rx1.try_recv().unwrap());

        let mut txn = isar.begin_txn(true, false).unwrap();
        let mut builder = col.new_object_builder(None);
        builder.write_long(2);
        col.put(&mut txn, builder.finish()).unwrap();
        txn.commit().unwrap();

        assert_eq!(rx1.len(), 0);
        assert_eq!(rx2.len(), 1);
        handle1.stop();
        handle2.stop();
    }

    #[test]
    fn test_delete() {
        isar!(isar, col => col!(oid => DataType::Long, field => DataType::Int; ind!(field)));

        let mut txn = isar.begin_txn(true, false).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(1);
        builder.write_int(111);
        let object = builder.finish();
        col.put(&mut txn, object).unwrap();

        let mut builder = col.new_object_builder(None);
        builder.write_long(2);
        builder.write_int(222);
        let object2 = builder.finish();
        col.put(&mut txn, object2).unwrap();

        col.delete(&mut txn, 1).unwrap();

        assert_eq!(
            col.debug_dump(&mut txn),
            map![2 => object2.as_bytes().to_vec()],
        );

        let index = &col.indexes[0];
        let key = index.debug_create_keys(object2)[0].clone();
        assert_eq!(
            index.debug_dump(&mut txn),
            set![(key, oid_to_bytes(2, col.id).unwrap().to_vec())],
        );
    }

    #[test]
    fn test_delete_calls_notifiers() {
        isar!(isar, col => col!(field1 => DataType::Long));

        let (tx, rx) = unbounded();
        let handle = isar.watch_collection(col, Box::new(move || tx.send(true).unwrap()));

        let mut txn = isar.begin_txn(true, false).unwrap();
        let mut builder = col.new_object_builder(None);
        builder.write_long(1234);
        col.put(&mut txn, builder.finish()).unwrap();
        txn.commit().unwrap();

        assert_eq!(rx.len(), 1);
        assert!(rx.try_recv().unwrap());
        handle.stop();
    }
}
