use crate::async_txn::IsarAsyncTxn;
use crate::raw_object_set::{RawObject, RawObjectSend};
use isar_core::collection::IsarCollection;
use isar_core::error::Result;
use isar_core::txn::IsarTxn;
use std::ffi::CString;
use std::os::raw::c_char;

#[no_mangle]
pub unsafe extern "C" fn isar_get(
    collection: &IsarCollection,
    txn: &IsarTxn,
    object: &mut RawObject,
) -> i32 {
    isar_try! {
        let object_id = object.get_object_id(collection).unwrap();
        let result = collection.get(txn, object_id)?;
        if let Some(result) = result {
            object.set_object(result);
        } else {
            object.set_empty();
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_get_async(
    collection: &'static IsarCollection,
    txn: &IsarAsyncTxn,
    object: &'static mut RawObject,
) {
    let object = RawObjectSend(object);
    let oid = object.0.get_object_id(collection).unwrap();
    txn.exec(move |txn| -> Result<()> {
        let result = collection.get(txn, oid)?;
        if let Some(result) = result {
            object.0.set_object(result);
        } else {
            object.0.set_empty();
        }
        Ok(())
    });
}

#[no_mangle]
pub unsafe extern "C" fn isar_put(
    collection: &mut IsarCollection,
    txn: &mut IsarTxn,
    object: &mut RawObject,
) -> i32 {
    isar_try! {
        let oid = object.get_object_id(collection);
        let data = object.object_as_slice();
        let oid = collection.put(txn, oid, data)?;
        object.set_object_id(oid);
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_put_async(
    collection: &'static IsarCollection,
    txn: &IsarAsyncTxn,
    object: &'static mut RawObject,
) {
    let object = RawObjectSend(object);
    let oid = object.0.get_object_id(collection);
    txn.exec(move |txn| -> Result<()> {
        let data = object.0.object_as_slice();
        let oid = collection.put(txn, oid, data)?;
        object.0.set_object_id(oid);
        Ok(())
    });
}

#[no_mangle]
pub unsafe extern "C" fn isar_delete(
    collection: &IsarCollection,
    txn: &mut IsarTxn,
    object: &RawObject,
) -> i32 {
    isar_try! {
    let oid = object.get_object_id(collection).unwrap();
        collection.delete(txn, oid)?;
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_delete_async(
    collection: &'static IsarCollection,
    txn: &IsarAsyncTxn,
    object: &RawObject,
) {
    let oid = object.get_object_id(collection).unwrap();
    txn.exec(move |txn| collection.delete(txn, oid));
}

#[no_mangle]
pub unsafe extern "C" fn isar_delete_all(collection: &IsarCollection, txn: &mut IsarTxn) -> i32 {
    isar_try! {
        collection.delete_all(txn)?;
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_delete_all_async(
    collection: &'static IsarCollection,
    txn: &IsarAsyncTxn,
) {
    txn.exec(move |txn| collection.delete_all(txn));
}

#[no_mangle]
pub unsafe extern "C" fn isar_export_json(
    collection: &IsarCollection,
    txn: &IsarTxn,
    json: *mut *mut c_char,
    json_length: *mut u32,
) -> i32 {
    isar_try! {
        let exported_json = collection.export_json(txn)?.to_string();
        json_length.write(exported_json.len() as u32);
        let json_str = CString::new(exported_json).unwrap();
        json.write(json_str.into_raw());
    }
}

struct JsonStr(*mut *mut c_char);
unsafe impl Send for JsonStr {}

struct JsonLen(*mut u32);
unsafe impl Send for JsonLen {}

#[no_mangle]
pub unsafe extern "C" fn isar_export_json_async(
    collection: &'static IsarCollection,
    txn: &IsarAsyncTxn,
    json: *mut *mut c_char,
    json_length: *mut u32,
) {
    let json = JsonStr(json);
    let json_length = JsonLen(json_length);
    txn.exec(move |txn| -> Result<()> {
        let exported_json = collection.export_json(txn)?.to_string();
        json_length.0.write(exported_json.len() as u32);
        let json_str = CString::new(exported_json).unwrap();
        json.0.write(json_str.into_raw());
        Ok(())
    });
}

#[no_mangle]
pub unsafe extern "C" fn isar_free_json(json: *mut c_char) {
    CString::from_raw(json);
}
