use super::raw_object_set::RawObjectSet;
use crate::txn::IsarDartTxn;
use crate::{from_c_str, UintSend};
use isar_core::collection::IsarCollection;
use isar_core::error::illegal_arg;
use isar_core::key::IndexKey;
use isar_core::query::filter::Filter;
use isar_core::query::query_builder::QueryBuilder;
use isar_core::query::{Query, Sort};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn isar_qb_create(collection: &IsarCollection) -> *mut QueryBuilder {
    let builder = collection.new_query_builder();
    Box::into_raw(Box::new(builder))
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_add_id_where_clause(
    builder: &mut QueryBuilder,
    start_id: i64,
    end_id: i64,
) -> i32 {
    isar_try! {
        builder.add_id_where_clause(start_id, end_id)?;
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_add_index_where_clause(
    builder: &mut QueryBuilder,
    index_index: u32,
    start_key: *mut IndexKey,
    include_start: bool,
    end_key: *mut IndexKey,
    include_end: bool,
    skip_duplicates: bool,
) -> i32 {
    let start_key = *Box::from_raw(start_key);
    let end_key = *Box::from_raw(end_key);
    isar_try! {
        builder.add_index_where_clause(
            index_index as usize,
            start_key,
            include_start,
            end_key,
            include_end,
            skip_duplicates,
        )?;
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_set_filter(builder: &mut QueryBuilder, filter: *mut Filter) {
    let filter = *Box::from_raw(filter);
    builder.set_filter(filter);
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_add_sort_by(
    collection: &IsarCollection,
    builder: &mut QueryBuilder,
    property_index: u32,
    asc: bool,
) -> i32 {
    let property = collection.properties.get(property_index as usize);
    let sort = if asc {
        Sort::Ascending
    } else {
        Sort::Descending
    };
    isar_try! {
        if let Some(property) = property {
            builder.add_sort(*property, sort)?;
        } else {
            illegal_arg("Property does not exist.")?;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_add_distinct_by(
    collection: &IsarCollection,
    builder: &mut QueryBuilder,
    property_index: u32,
    case_sensitive: bool,
) -> i32 {
    let property = collection.properties.get(property_index as usize);
    isar_try! {
        if let Some(property) = property {
            builder.add_distinct(*property, case_sensitive);
        } else {
            illegal_arg("Property does not exist.")?;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_set_offset_limit(
    builder: &mut QueryBuilder,
    offset: u32,
    limit: u32,
) {
    builder.set_offset(offset as usize);
    builder.set_limit(limit as usize);
}

#[no_mangle]
pub unsafe extern "C" fn isar_qb_build(builder: *mut QueryBuilder) -> *mut Query {
    let query = Box::from_raw(builder).build();
    Box::into_raw(Box::new(query))
}

#[no_mangle]
pub unsafe extern "C" fn isar_q_free(query: *mut Query) {
    Box::from_raw(query);
}

#[no_mangle]
pub unsafe extern "C" fn isar_q_find(
    query: &'static Query,
    txn: &mut IsarDartTxn,
    result: &'static mut RawObjectSet,
    limit: u32,
) -> i32 {
    isar_try_txn!(txn, move |txn| {
        result.fill_from_query(query, txn, limit as usize)?;
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn isar_q_delete(
    query: &'static Query,
    collection: &'static IsarCollection,
    txn: &mut IsarDartTxn,
    limit: u32,
    count: &'static mut u32,
) -> i32 {
    let limit = limit as usize;
    let count = UintSend(count);
    isar_try_txn!(txn, move |txn| {
        let mut ids_to_delete = vec![];
        query.find_while(txn, |id, _| {
            ids_to_delete.push(id);
            ids_to_delete.len() <= limit
        })?;
        *count.0 = ids_to_delete.len() as u32;
        for id in ids_to_delete {
            collection.delete(txn, id)?;
        }
        Ok(())
    })
}

struct JsonBytes(*mut *mut u8);
unsafe impl Send for JsonBytes {}

struct JsonLen(*mut u32);
unsafe impl Send for JsonLen {}

#[no_mangle]
pub unsafe extern "C" fn isar_q_export_json(
    query: &'static Query,
    collection: &'static IsarCollection,
    txn: &mut IsarDartTxn,
    id_name: *const c_char,
    primitive_null: bool,
    json_bytes: *mut *mut u8,
    json_length: *mut u32,
) -> i32 {
    let id_name = if !id_name.is_null() {
        Some(from_c_str(id_name).unwrap())
    } else {
        None
    };
    let json = JsonBytes(json_bytes);
    let json_length = JsonLen(json_length);
    isar_try_txn!(txn, move |txn| {
        let json = json;
        let json_length = json_length;
        let exported_json = query.export_json(txn, collection, id_name, primitive_null, true)?;
        let bytes = serde_json::to_vec(&exported_json).unwrap();
        let mut bytes = bytes.into_boxed_slice();
        json_length.0.write(bytes.len() as u32);
        json.0.write(bytes.as_mut_ptr());
        std::mem::forget(bytes);
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn isar_free_json(json_bytes: *mut u8, json_length: u32) {
    Vec::from_raw_parts(json_bytes, json_length as usize, json_length as usize);
}
